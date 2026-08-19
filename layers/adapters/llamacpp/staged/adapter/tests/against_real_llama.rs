use p4_llamacpp_staged_adapter::process::{ProcessServerControl, ServerControl, ServerLaunch};
use p4_llamacpp_staged_adapter::{
    Frame, HopPayload, HopPhase, KvPayload, KvReceipt, KvReceiptState, KvResult, Operation,
    PROTOCOL_REVISION, ProtocolLimits, SequencePayload,
};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SERVER_BINARY_ENV: &str = "P4_STAGED_LLAMA_SERVER_BINARY";
const MODEL_ENV: &str = "P4_STAGED_LLAMA_MODEL";
const LAYER_BEGIN_ENV: &str = "P4_STAGED_LLAMA_LAYER_BEGIN";
const LAYER_END_ENV: &str = "P4_STAGED_LLAMA_LAYER_END";
const KV_ROOT_ENV: &str = "P4_STAGED_LLAMA_KV_ROOT";
const REQUEST_OPTIONS: &str = r#"{"temperature":0,"request_tag":"gate5-options"}"#;

#[test]
#[ignore = "real llama.cpp model E2E; set P4_STAGED_LLAMA_SERVER_BINARY and P4_STAGED_LLAMA_MODEL, then pass --ignored"]
fn process_control_round_trips_real_llama_plan_hello_hop_kv_and_unload() {
    let Some(binary) = required_file(SERVER_BINARY_ENV, "C++ llama staged server binary") else {
        return;
    };
    let Some(model) = required_file(MODEL_ENV, "GGUF model") else {
        return;
    };

    let layer_begin = env_i32(LAYER_BEGIN_ENV, 0);
    // The tail is required for sampled token/text evidence. The default is
    // the small Qwen2.5-1.5B GGUF's full 28-layer range; override it only when
    // testing a model with a different layer count.
    let layer_end = env_i32(LAYER_END_ENV, 28);
    if layer_begin < 0 || layer_end <= layer_begin {
        println!("SKIP: {LAYER_BEGIN_ENV}/{LAYER_END_ENV} must describe a non-empty layer range");
        return;
    }

    let kv_root = KvRoot::from_environment();
    let model_identity = model.to_string_lossy().into_owned();
    let extra_args = env::var("P4_STAGED_LLAMA_PLAN_EXTRA_ARGS").unwrap_or_default();
    let plan = format!(
        "--model {} --layer-begin {layer_begin} --layer-end {layer_end} \
        --n-seq-max 1 --ctx-size 512 --temp 0 --seed 1 --model-identity {} --kv-root {} {}",
        quote_plan_value(&model_identity),
        quote_plan_value(&model_identity),
        quote_plan_value(&kv_root.path.to_string_lossy()),
        extra_args,
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve staged server endpoint");
    let endpoint = listener.local_addr().expect("read staged server endpoint");
    drop(listener);

    let limits = ProtocolLimits::default();
    let child_environment = runtime_environment(&binary);
    let mut launch = ServerLaunch::new(binary, endpoint, plan.into_bytes());
    launch.args = vec![
        "--port".into(),
        endpoint.port().to_string().into(),
        "--bind".into(),
        "127.0.0.1".into(),
    ];
    launch.ready_timeout = Duration::from_secs(120);
    launch.io_timeout = Duration::from_secs(60);
    launch.environment = child_environment;

    let restart_launch = launch.clone();
    let mut control = ProcessServerControl::new(launch);
    control.start().expect("start real C++ llama staged server");
    let ready = wait_ready(&mut control, Duration::from_secs(120));
    assert_eq!(ready.protocol_revision, PROTOCOL_REVISION);
    assert!(ready.server_id.starts_with("READY;"), "{ready:?}");
    println!(
        "REAL_E2E READY pid={} model={} endpoint={} capabilities={}",
        control_pid_label(control.pid()),
        model.display(),
        endpoint,
        ready.server_id
    );

    let sequence_id = "real-llama-e2e".to_owned();
    let hop_body = HopPayload {
        phase: HopPhase::Prefill,
        sequences: vec![SequencePayload {
            sequence_id: sequence_id.clone(),
            descriptors: Vec::new(),
            payloads: Vec::new(),
            n_tokens: None,
            prompt: Some("Reply with one short word: hello".into()),
            initial_tokens: None,
            position: Some(0),
            options: REQUEST_OPTIONS.into(),
            outcome: None,
        }],
        legacy: false,
    }
    .encode(limits)
    .expect("encode prompt HOP");
    let hop_response = request(&mut control, Operation::Hop, hop_body);
    assert_success_operation(&hop_response, Operation::HopResult);
    let hop_result = HopPayload::decode(&hop_response.body, limits).expect("decode HOP_RESULT");
    assert_eq!(hop_result.sequences.len(), 1);
    assert_eq!(hop_result.sequences[0].sequence_id, sequence_id);
    assert_eq!(
        hop_result.sequences[0].descriptors.len(),
        hop_result.sequences[0].payloads.len()
    );
    assert_eq!(hop_result.sequences[0].options, REQUEST_OPTIONS);
    println!(
        "REAL_E2E HOP phase=prefill sequence={} output_descriptors={}",
        sequence_id,
        hop_result.sequences[0].descriptors.len()
    );

    let prefill = hop_result.sequences.into_iter().next().unwrap();
    let prefill_position = prefill
        .n_tokens
        .expect("prefill HOP reports the logical token position");
    if env::var_os("P4_STAGED_LLAMA_PREFILL_ONLY").is_some() {
        let unload_response = control
            .request(Frame::new(Operation::Unload, Vec::new()).expect("create UNLOAD frame"))
            .expect("UNLOAD response");
        assert_success_operation(&unload_response, Operation::Unload);
        assert_eq!(unload_response.body, b"UNLOADED");
        let cleanup = control.shutdown();
        assert!(control.pid().is_none(), "server child was not reaped");
        if let Err(error) = cleanup {
            println!("REAL_E2E cleanup_after_prefill_only={error}");
        }
        println!("REAL_E2E REQUEST_OPTIONS_PREFILL_ONLY round_trip=ok");
        return;
    }
    let decode_body = HopPayload {
        phase: HopPhase::Decode,
        sequences: vec![SequencePayload {
            sequence_id: sequence_id.clone(),
            // A decode lap starts at stage zero. The preceding tail cut-set
            // is not re-fed into the same full-range context; the node outcome
            // layer drops that wrapper before the next lap.
            descriptors: Vec::new(),
            payloads: Vec::new(),
            n_tokens: Some(1),
            prompt: None,
            initial_tokens: None,
            position: Some(prefill_position),
            options: REQUEST_OPTIONS.into(),
            outcome: None,
        }],
        legacy: false,
    }
    .encode(limits)
    .expect("encode decode HOP");
    let decode_response = request(&mut control, Operation::Hop, decode_body);
    assert_success_operation(&decode_response, Operation::HopResult);
    let decode_result =
        HopPayload::decode(&decode_response.body, limits).expect("decode sampled HOP_RESULT");
    let outcome = decode_result.sequences[0]
        .outcome
        .as_ref()
        .expect("tail HOP includes sampled outcome metadata");
    assert_eq!(decode_result.sequences[0].options, REQUEST_OPTIONS);
    assert!(outcome.token >= 0, "tail HOP includes sampled token id");
    assert_eq!(outcome.position, prefill_position + 1);
    assert!(
        !outcome.text.is_empty() || outcome.stop.is_some(),
        "tail HOP includes token text or an end-of-generation reason"
    );
    println!(
        "REAL_E2E HOP phase=decode token={} text={:?} position={} stop={:?}",
        outcome.token, outcome.text, outcome.position, outcome.stop
    );

    if capability_is_enabled(&ready.server_id, "kv=1")
        && env::var_os("P4_STAGED_LLAMA_SKIP_KV").is_none()
    {
        let kv_request = KvPayload {
            sequence_id: sequence_id.clone(),
            cache_key: "real-llama-e2e".into(),
            model_identity: model_identity.clone(),
            stage_begin: layer_begin,
            stage_end: layer_end,
            flags: 0,
            expected_checksum: String::new(),
            operation_id: "real-llama-kv".into(),
        };
        let save = kv_request_frame(Operation::KvSave, &kv_request, limits);
        let save_response = control.request(save).expect("KV_SAVE response");
        assert_success_operation(&save_response, Operation::KvResult);
        let saved = KvResult::decode(&save_response.body, limits).expect("decode KV_SAVE result");
        assert_eq!(saved.sequence_id, sequence_id);
        assert_eq!(saved.cache_key, kv_request.cache_key);
        assert!(saved.bytes > 0, "KV_SAVE returned zero bytes");
        assert_eq!(saved.checksum.len(), 64);

        let restore = kv_request_frame(Operation::KvRestore, &kv_request, limits);
        let restore_response = control.request(restore).expect("KV_RESTORE response");
        assert_success_operation(&restore_response, Operation::KvResult);
        let restored =
            KvResult::decode(&restore_response.body, limits).expect("decode KV_RESTORE result");
        assert_eq!(restored, saved);

        let transaction_payload = KvPayload {
            sequence_id: sequence_id.clone(),
            cache_key: "real-llama-transaction".into(),
            model_identity: model_identity.clone(),
            stage_begin: layer_begin,
            stage_end: layer_end,
            flags: 1,
            expected_checksum: String::new(),
            operation_id: "real-llama-transaction".into(),
        };
        let prepare = control
            .request(kv_request_frame(
                Operation::KvPrepare,
                &transaction_payload,
                limits,
            ))
            .expect("KV_PREPARE response");
        assert_success_operation(&prepare, Operation::KvReceipt);
        let prepared = KvReceipt::decode(&prepare.body, limits).expect("decode prepare receipt");
        assert_eq!(prepared.state, KvReceiptState::Prepared);

        let mut transaction_control = transaction_payload.clone();
        transaction_control.flags = 0;
        let commit = control
            .request(kv_request_frame(
                Operation::KvCommit,
                &transaction_control,
                limits,
            ))
            .expect("KV_COMMIT response");
        assert_success_operation(&commit, Operation::KvReceipt);
        let committed = KvReceipt::decode(&commit.body, limits).expect("decode commit receipt");
        assert_eq!(committed.state, KvReceiptState::Committed);

        let reconcile = control
            .request(kv_request_frame(
                Operation::KvReconcile,
                &transaction_control,
                limits,
            ))
            .expect("KV_RECONCILE response");
        assert_success_operation(&reconcile, Operation::KvReceipt);
        let reconciled =
            KvReceipt::decode(&reconcile.body, limits).expect("decode committed reconcile receipt");
        assert_eq!(reconciled.state, KvReceiptState::Committed);

        let crashed_pid = control.pid().expect("running stage pid before restart");
        force_kill_stage(crashed_pid);
        let _ = control.shutdown();
        control = ProcessServerControl::new(restart_launch.clone());
        control
            .start()
            .expect("restart real C++ llama staged server after simulated crash");
        let restarted = wait_ready(&mut control, Duration::from_secs(120));
        assert_eq!(restarted.protocol_revision, PROTOCOL_REVISION);
        assert!(restarted.server_id.starts_with("READY;"), "{restarted:?}");
        let after_restart = control
            .request(kv_request_frame(
                Operation::KvReconcile,
                &transaction_control,
                limits,
            ))
            .expect("KV_RECONCILE response after stage restart");
        assert_success_operation(&after_restart, Operation::KvReceipt);
        let after_restart_receipt = KvReceipt::decode(&after_restart.body, limits)
            .expect("decode post-restart reconcile receipt");
        assert_eq!(after_restart_receipt.state, KvReceiptState::Committed);
        println!("REAL_E2E KV_RESTART_RECONCILE state=committed");

        fs::remove_file(kv_root.path.join("real-llama-transaction.lkv"))
            .expect("remove transaction KV state for fault injection");
        let damaged_reconcile = control
            .request(kv_request_frame(
                Operation::KvReconcile,
                &transaction_control,
                limits,
            ))
            .expect("damaged KV_RECONCILE response");
        assert_success_operation(&damaged_reconcile, Operation::KvReceipt);
        let damaged = KvReceipt::decode(&damaged_reconcile.body, limits)
            .expect("decode damaged reconcile receipt");
        assert_eq!(damaged.state, KvReceiptState::Inconsistent);

        let drop_request = kv_request_frame(Operation::KvDrop, &kv_request, limits);
        let drop_response = control.request(drop_request).expect("KV_DROP response");
        assert_success_operation(&drop_response, Operation::KvResult);
        println!(
            "REAL_E2E KV save_restore_drop bytes={} checksum={}",
            saved.bytes, saved.checksum
        );
    } else {
        println!("SKIP: real C++ server reported kv=0 in HELLO capabilities");
    }

    let unload_response = control
        .request(Frame::new(Operation::Unload, Vec::new()).expect("create UNLOAD frame"))
        .expect("UNLOAD response");
    assert_success_operation(&unload_response, Operation::Unload);
    assert_eq!(unload_response.body, b"UNLOADED");
    println!("REAL_E2E UNLOAD round_trip=ok");

    // The explicit UNLOAD above is the tested round trip. shutdown() is still
    // needed to reap the child because ProcessServerControl owns that child.
    let cleanup = control.shutdown();
    assert!(control.pid().is_none(), "server child was not reaped");
    if let Err(error) = cleanup {
        println!("REAL_E2E cleanup_after_unload={error}");
    }
}

fn required_file(name: &str, description: &str) -> Option<PathBuf> {
    match env::var_os(name).filter(|value| !value.is_empty()) {
        Some(value) => {
            let path = PathBuf::from(value);
            if path.is_file() {
                Some(path)
            } else {
                println!(
                    "SKIP: {description} from {name} is not a file: {}",
                    path.display()
                );
                None
            }
        }
        None => {
            println!("SKIP: set {name} to the {description} path");
            None
        }
    }
}

fn env_i32(name: &str, default: i32) -> i32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn quote_plan_value(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn force_kill_stage(pid: u32) {
    #[cfg(windows)]
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()
        .expect("force-kill stage server");
    #[cfg(unix)]
    let status = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status()
        .expect("force-kill stage server");
    assert!(status.success(), "force-kill stage server failed: {status}");
}

fn runtime_environment(binary: &Path) -> Vec<(OsString, OsString)> {
    let mut path_entries = vec![
        binary.parent().unwrap_or(binary).to_path_buf(),
        binary
            .parent()
            .and_then(|parent| parent.parent())
            .map(|parent| parent.join("bin").join("Release"))
            .unwrap_or_default(),
    ];
    path_entries.retain(|path| path.is_dir());
    if let Some(path) = env::var_os("PATH") {
        path_entries.extend(env::split_paths(&path));
    }
    let path = env::join_paths(path_entries).expect("compose child PATH");
    let mut environment = vec![(OsString::from("PATH"), path)];
    for key in ["P4_STAGED_TRACE_KV_TRANSACTION"] {
        if let Some(value) = env::var_os(key) {
            environment.push((OsString::from(key), value));
        }
    }
    if let Some(device_mask) = env::var_os("P4_STAGED_LLAMA_CUDA_VISIBLE_DEVICES") {
        environment.push((OsString::from("CUDA_VISIBLE_DEVICES"), device_mask));
    }
    environment
}

fn request(control: &mut ProcessServerControl, operation: Operation, body: Vec<u8>) -> Frame {
    control
        .request(Frame::new(operation, body).expect("create staged request frame"))
        .unwrap_or_else(|error| panic!("{operation:?} response failed: {error}"))
}

fn wait_ready(
    control: &mut ProcessServerControl,
    timeout: Duration,
) -> p4_llamacpp_staged_adapter::process::ReadyInfo {
    let deadline = Instant::now() + timeout;
    loop {
        match control.wait_ready(deadline) {
            Ok(Some(ready)) => return ready,
            Ok(None) if Instant::now() >= deadline => {
                panic!("real C++ server did not become ready within {timeout:?}")
            }
            Ok(None) => std::thread::yield_now(),
            Err(error) => panic!("HELLO exchange failed with real C++ server: {error}"),
        }
    }
}

fn kv_request_frame(operation: Operation, payload: &KvPayload, limits: ProtocolLimits) -> Frame {
    let body = payload.encode(limits).expect("encode KV request");
    Frame::new(operation, body).expect("create KV request frame")
}

fn assert_success_operation(response: &Frame, expected: Operation) {
    assert_eq!(
        response.header.operation,
        expected,
        "staged server returned {:?}: {}",
        response.header.operation,
        String::from_utf8_lossy(&response.body)
    );
}

fn capability_is_enabled(server_id: &str, capability: &str) -> bool {
    server_id.split(';').any(|field| field == capability)
}

fn control_pid_label(pid: Option<u32>) -> String {
    pid.map_or_else(|| "unknown".into(), |value| value.to_string())
}

struct KvRoot {
    path: PathBuf,
    owned: bool,
}

impl KvRoot {
    fn from_environment() -> Self {
        if let Some(path) = env::var_os(KV_ROOT_ENV).filter(|value| !value.is_empty()) {
            return Self {
                path: PathBuf::from(path),
                owned: false,
            };
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let path =
            env::temp_dir().join(format!("p4-staged-llama-kv-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("create temporary staged KV root");
        Self { path, owned: true }
    }
}

impl Drop for KvRoot {
    fn drop(&mut self) {
        if self.owned {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
