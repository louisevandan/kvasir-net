#[cfg(windows)]
mod windows;

use crate::ipc;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    process::{Child, ChildStdin, ChildStdout, Command},
    task::JoinHandle,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub protocol: u32,
    pub entry: String,
    pub files: BTreeMap<String, String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub python: String,
    pub bundle: PathBuf,
    pub bundle_sha256: String,
    pub config: Value,
    pub identity: Value,
    pub frame_bytes: usize,
    pub scratch_bytes: usize,
    pub stderr_bytes: usize,
    pub timeout_ms: u64,
}

pub struct Worker {
    child: Child,
    #[cfg(windows)]
    group: windows::Group,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_task: JoinHandle<()>,
    pub launch: Launch,
    // A malformed completion is retained until explicit abort/owner abandonment.
    pub last_response: Vec<u8>,
}

pub fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bounded_file(path: &Path, cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > cap {
        return Err("bundle file exceeds byte budget".into());
    }
    Ok(bytes)
}

pub fn verify(launch: &Launch) -> Result<PathBuf, String> {
    if !(256..=ipc::FRAME_LIMIT).contains(&launch.frame_bytes)
        || launch.scratch_bytes
            < launch
                .frame_bytes
                .checked_mul(4)
                .ok_or("scratch overflow")?
        || launch.scratch_bytes > 512 * 1024 * 1024
        || launch.stderr_bytes == 0
        || launch.stderr_bytes > 65536
        || !(1..=120000).contains(&launch.timeout_ms)
    {
        return Err("invalid finite IPC/scratch/stderr/deadline budget".into());
    }
    let bytes = bounded_file(&launch.bundle, ipc::HEADER_LIMIT)?;
    if bytes.len() > ipc::HEADER_LIMIT || sha(&bytes) != launch.bundle_sha256 {
        return Err("bundle manifest identity mismatch".into());
    }
    let bundle: Bundle = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if bundle.protocol != 2
        || bundle.files.is_empty()
        || bundle.files.len() > 512
        || !bundle.files.contains_key(&bundle.entry)
    {
        return Err("incompatible bundle protocol/entry".into());
    }
    let root = launch
        .bundle
        .parent()
        .ok_or("bundle has no parent")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    for (relative, expected) in &bundle.files {
        let path = root
            .join(relative)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if Path::new(relative).is_absolute() || !path.starts_with(&root) {
            return Err("bundle path escapes root".into());
        }
        let data = bounded_file(&path, 16 * 1024 * 1024)?;
        if sha(&data) != *expected {
            return Err(format!("bundle file mismatch: {relative}"));
        }
    }
    Ok(root.join(bundle.entry))
}

pub struct StartError {
    pub detail: String,
    pub cleanup_error: Option<String>,
    pub worker: Option<Worker>,
}
impl From<String> for StartError {
    fn from(detail: String) -> Self {
        Self {
            detail,
            cleanup_error: None,
            worker: None,
        }
    }
}

impl Worker {
    pub async fn start(launch: Launch) -> Result<(Self, Value), StartError> {
        let entry = verify(&launch)?;
        let init = json!({"op":"initialize", "protocol":2, "bundle_sha256":launch.bundle_sha256,
            "identity":launch.identity, "config":launch.config, "frame_bytes":launch.frame_bytes});
        let packet = ipc::pack(&init, &[], launch.frame_bytes)?;
        let mut command = Command::new(&launch.python);
        command
            .arg("-I")
            .arg("-B")
            .arg(&entry)
            .current_dir(entry.parent().unwrap())
            .env("PYTHONNOUSERSITE", "1")
            .env_remove("PYTHONPATH")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000 | 0x00000004);
        #[cfg(windows)]
        let group = windows::Group::new()?;
        let mut child = command.spawn().map_err(|e| format!("worker spawn: {e}"))?;
        #[cfg(windows)]
        if let Err(error) = group.attach_and_resume(&child) {
            let _ = group.kill();
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(format!("worker containment: {error}").into());
        }
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut pipe = child.stderr.take().unwrap();
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let sink = stderr.clone();
        let cap = launch.stderr_bytes;
        let stderr_task = tokio::spawn(async move {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                let mut bytes = sink.lock().unwrap();
                let keep = n.min(cap - bytes.len());
                bytes.extend_from_slice(&chunk[..keep]);
            }
        });
        let mut worker = Self {
            child,
            #[cfg(windows)]
            group,
            stdin,
            stdout,
            stderr,
            stderr_task,
            launch,
            last_response: Vec::new(),
        };
        let result = worker.exchange(&packet).await.and_then(|bytes| {
            let (ready, body) = ipc::unpack(&bytes)?;
            if !body.is_empty()
                || ready["op"] != "ready"
                || ready["protocol"] != 2
                || ready["bundle_sha256"] != worker.launch.bundle_sha256
                || ready["identity"] != worker.launch.identity
            {
                return Err("worker readiness identity mismatch".into());
            }
            Ok(ready)
        });
        match result {
            Ok(ready) => Ok((worker, ready)),
            Err(error) => {
                let cleanup = worker.stop(true).await.err();
                let detail = format!("{error}; stderr={}", worker.diagnostic());
                let retained = if cleanup.is_some() {
                    Some(worker)
                } else {
                    None
                };
                Err(StartError {
                    detail,
                    cleanup_error: cleanup,
                    worker: retained,
                })
            }
        }
    }
    pub async fn exchange(&mut self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        let deadline = Duration::from_millis(self.launch.timeout_ms);
        tokio::time::timeout(deadline, async {
            ipc::write(&mut self.stdin, bytes, self.launch.frame_bytes).await?;
            ipc::read(
                &mut self.stdout,
                self.launch.frame_bytes,
                &mut self.last_response,
            )
            .await
        })
        .await
        .map_err(|_| "worker timeout; execution uncertain".to_string())?
    }
    pub fn diagnostic(&self) -> String {
        String::from_utf8_lossy(&self.stderr.lock().unwrap()).into_owned()
    }
    pub async fn stop(&mut self, kill: bool) -> Result<(), String> {
        if kill {
            #[cfg(windows)]
            self.group.kill()?;
            self.child.start_kill().map_err(|e| e.to_string())?;
        }
        let result = tokio::time::timeout(
            Duration::from_secs(if kill { 10 } else { 5 }),
            self.child.wait(),
        )
        .await;
        let status = match result {
            Ok(value) => value.map_err(|e| e.to_string())?,
            Err(_) => {
                #[cfg(windows)]
                self.group.kill()?;
                self.child.start_kill().map_err(|e| e.to_string())?;
                tokio::time::timeout(Duration::from_secs(10), self.child.wait())
                    .await
                    .map_err(|_| "child cleanup timeout")?
                    .map_err(|e| e.to_string())?;
                return Err("idle worker did not exit within 5 seconds".into());
            }
        };
        #[cfg(windows)]
        self.group.drained().await?;
        self.stderr_task.abort();
        if !kill && !status.success() {
            return Err(format!("worker exit {status}"));
        }
        Ok(())
    }
}
