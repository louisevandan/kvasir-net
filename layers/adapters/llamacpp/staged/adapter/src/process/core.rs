use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessState {
    Created,
    Starting,
    AwaitingReady,
    Ready,
    Stopping,
    Exited,
    Crashed,
    TimedOut,
}

impl ProcessState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Exited | Self::Crashed | Self::TimedOut)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadyInfo {
    pub protocol_revision: u16,
    pub server_id: String,
    pub transactions: bool,
    pub physical_batch: bool,
    pub equal_sequence_ubatch: bool,
    pub max_atomic_sequences: usize,
    pub atomic_batch_exclusive: bool,
    pub n_ctx: usize,
    pub n_batch: usize,
    pub n_ubatch: usize,
    pub n_seq_max: u32,
    /// Which llama.cpp this stage was built from: the upstream commit and
    /// the patch queue applied on top of it. The commit alone does not
    /// identify a build - two stages can share it and differ in every
    /// behaviour the queue touches - so both travel and both are compared.
    pub upstream_commit: String,
    pub patch_set: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ProcessError {
    InvalidState(ProcessState),
    StartFailed(String),
    ReadyFailed(String),
    ReadyTimeout(Duration),
    ShutdownFailed(String),
    RequestFailed(String),
    Crashed(String),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

pub trait ServerControl {
    fn start(&mut self) -> Result<(), String>;
    fn wait_ready(&mut self, deadline: Instant) -> Result<Option<ReadyInfo>, String>;
    fn request(&mut self, request: Frame) -> Result<Frame, String> {
        let _ = request;
        Err("stage server request channel is unavailable".into())
    }
    fn shutdown(&mut self) -> Result<(), String>;
}

/// One stage-server process; the adapter forwards OUTER's opaque plan unchanged.
#[derive(Clone, Debug)]
pub struct ServerLaunch {
    pub binary: PathBuf,
    pub args: Vec<OsString>,
    pub environment: Vec<(OsString, OsString)>,
    pub endpoint: SocketAddr,
    pub plan: Vec<u8>,
    pub ready_timeout: Duration,
    pub io_timeout: Duration,
    pub protocol_limits: ProtocolLimits,
}

impl ServerLaunch {
    pub fn new(binary: impl Into<PathBuf>, endpoint: SocketAddr, plan: Vec<u8>) -> Self {
        Self {
            binary: binary.into(),
            args: Vec::new(),
            environment: Vec::new(),
            endpoint,
            plan,
            ready_timeout: Duration::from_secs(30),
            io_timeout: Duration::from_secs(5),
            protocol_limits: ProtocolLimits::default(),
        }
    }
}

pub struct ProcessServerControl {
    launch: ServerLaunch,
    child: Option<Child>,
    plan_stdin: Option<ChildStdin>,
    stream: Option<TcpStream>,
    server_id: Option<String>,
}

impl ProcessServerControl {
    pub fn new(launch: ServerLaunch) -> Self {
        Self {
            launch,
            child: None,
            plan_stdin: None,
            stream: None,
            server_id: None,
        }
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    fn child_alive(&mut self) -> Result<(), String> {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(status)) => Err(format!("stage server exited while starting: {status}")),
                Ok(None) => Ok(()),
                Err(error) => Err(format!("cannot poll stage server: {error}")),
            },
            None => Err("stage server process is not running".into()),
        }
    }

    fn connect_and_hello(&mut self, deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        if self.stream.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let stream = match TcpStream::connect_timeout(&self.launch.endpoint, remaining) {
                Ok(stream) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => return Ok(None),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                    return Ok(None);
                }
                Err(error) => return Err(format!("cannot connect to stage server: {error}")),
            };
            let local_endpoint = stream
                .local_addr()
                .map_err(|error| format!("cannot read stage socket local endpoint: {error}"))?;
            let peer_endpoint = stream
                .peer_addr()
                .map_err(|error| format!("cannot read stage socket peer endpoint: {error}"))?;
            // A dynamic-range target can echo HELLO through a self-connection.
            if is_self_connection(local_endpoint, peer_endpoint) {
                return Ok(None);
            }
            stream
                .set_read_timeout(Some(self.launch.io_timeout))
                .map_err(|error| format!("cannot configure stage socket: {error}"))?;
            stream
                .set_write_timeout(Some(self.launch.io_timeout))
                .map_err(|error| format!("cannot configure stage socket: {error}"))?;
            self.stream = Some(stream);
        }

        let stream = self.stream.as_mut().expect("stream was installed");
        let hello = Frame::new(Operation::Hello, PROTOCOL_REVISION.to_le_bytes().to_vec())
            .map_err(|error| error.to_string())?;
        if let Err(error) = hello.write_to(stream, self.launch.protocol_limits) {
            self.stream = None;
            if is_retryable_io(&error) {
                return Ok(None);
            }
            return Err(error.to_string());
        }
        let response = match Frame::read_from(stream, self.launch.protocol_limits) {
            Ok(frame) => frame,
            Err(error) => {
                self.stream = None;
                if is_retryable_io(&error) {
                    return Ok(None);
                }
                return Err(error.to_string());
            }
        };
        if response.header.operation != Operation::Hello {
            return Err(format!(
                "stage server readiness replied with {:?}, expected HELLO",
                response.header.operation
            ));
        }
        let ready = decode_hello(&response.body).map_err(|error| {
            let pid = self
                .child
                .as_ref()
                .map(|child| child.id().to_string())
                .unwrap_or_else(|| "none".to_owned());
            format!(
                "{error}; endpoint={}; child_pid={pid}; binary={}",
                self.launch.endpoint,
                self.launch.binary.display()
            )
        })?;
        self.server_id = Some(ready.server_id.clone());
        Ok(Some(ready))
    }
}

impl ServerControl for ProcessServerControl {
    fn start(&mut self) -> Result<(), String> {
        if self.child.is_some() {
            return Err("stage server is already started".into());
        }
        let mut command = Command::new(&self.launch.binary);
        let stderr = if env::var_os("P4_STAGED_LLAMA_INHERIT_STDERR").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
        command
            .args(&self.launch.args)
            .envs(self.launch.environment.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(stderr);
        windowless(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", self.launch.binary.display()))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "stage server stdin was not piped".to_owned())?;
        let plan_len = u32::try_from(self.launch.plan.len())
            .map_err(|_| "stage server plan exceeds u32 length prefix".to_owned())?;
        if let Err(error) = stdin
            .write_all(&plan_len.to_le_bytes())
            .and_then(|_| stdin.write_all(&self.launch.plan))
            .and_then(|_| stdin.flush())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("cannot send stage server plan: {error}"));
        }
        self.plan_stdin = Some(stdin);
        self.child = Some(child);
        Ok(())
    }

    fn wait_ready(&mut self, deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        self.child_alive()?;
        self.connect_and_hello(deadline)
    }

    fn request(&mut self, request: Frame) -> Result<Frame, String> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| "stage server is not connected".to_owned())?;
        request
            .write_to(stream, self.launch.protocol_limits)
            .map_err(|error| format!("cannot send stage server request: {error}"))?;
        Frame::read_from(stream, self.launch.protocol_limits)
            .map_err(|error| format!("cannot receive stage server response: {error}"))
    }

    fn shutdown(&mut self) -> Result<(), String> {
        let mut graceful_error = None;
        if let Some(stream) = self.stream.as_mut() {
            match Frame::new(Operation::Unload, Vec::new())
                .map_err(|error| error.to_string())
                .and_then(|frame| {
                    frame
                        .write_to(stream, self.launch.protocol_limits)
                        .map_err(|error| error.to_string())
                }) {
                Ok(()) => {}
                Err(error) => graceful_error = Some(error),
            }
        }
        self.stream.take();
        self.plan_stdin.take();

        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + self.launch.io_timeout;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Ok(None) => {
                        child
                            .kill()
                            .map_err(|error| format!("cannot terminate stage server: {error}"))?;
                        child
                            .wait()
                            .map_err(|error| format!("cannot reap stage server: {error}"))?;
                        break;
                    }
                    Err(error) => {
                        return Err(format!("cannot poll stage server shutdown: {error}"));
                    }
                }
            }
        }
        self.server_id = None;
        graceful_error.map_or(Ok(()), Err)
    }
}

impl Drop for ProcessServerControl {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

pub(super) fn decode_hello(body: &[u8]) -> Result<ReadyInfo, String> {
    if body.len() < 2 {
        return Err(format!(
            "stage server HELLO body is truncated; body_bytes={}",
            body.len()
        ));
    }
    let revision = u16::from_le_bytes([body[0], body[1]]);
    if revision != PROTOCOL_REVISION {
        return Err(format!(
            "stage server protocol revision {revision} is unsupported"
        ));
    }
    let id = if body.len() == 2 { &[][..] } else { &body[2..] };
    let text = String::from_utf8(id.to_vec()).map_err(|error| {
        format!(
            "stage server HELLO id is not UTF-8; body_bytes={}; valid_up_to={}",
            body.len(),
            error.utf8_error().valid_up_to()
        )
    })?;
    let transactions = text.split(';').any(|field| field == "transactions=1");
    let physical_batch = text.split(';').any(|field| field == "physical_batch=1");
    let equal_sequence_ubatch = text
        .split(';')
        .any(|field| field == "equal_sequence_ubatch=1");
    let atomic_batch_exclusive = text
        .split(';')
        .any(|field| field == "atomic_batch_exclusive=1");
    Ok(ReadyInfo {
        protocol_revision: PROTOCOL_REVISION,
        n_ctx: capability_number(&text, "n_ctx")?,
        n_batch: capability_number(&text, "n_batch")?,
        n_ubatch: capability_number(&text, "n_ubatch")?,
        n_seq_max: capability_number(&text, "n_seq_max")?,
        max_atomic_sequences: capability_number(&text, "max_atomic_sequences")?,
        upstream_commit: capability_text(&text, "upstream"),
        patch_set: capability_text(&text, "patch_set"),
        server_id: text,
        transactions,
        physical_batch,
        equal_sequence_ubatch,
        atomic_batch_exclusive,
    })
}

/// A named capability field, or `unknown` when the stage did not report one.
/// Absence is reported rather than refused: an older stage server predates
/// the field, and refusing it here would turn a provenance gap into a load
/// failure at the wrong layer.
fn capability_text(text: &str, name: &str) -> String {
    let prefix = format!("{name}=");
    text.split(';')
        .find_map(|field| field.strip_prefix(&prefix))
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn capability_number<T>(text: &str, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    let prefix = format!("{name}=");
    text.split(';')
        .find_map(|field| field.strip_prefix(&prefix))
        .ok_or_else(|| {
            format!(
                "stage server HELLO omits {name}; hello_id_bytes={}; server_id={text:?}",
                text.len()
            )
        })?
        .parse()
        .map_err(|_| {
            format!(
                "stage server HELLO has invalid {name}; hello_id_bytes={}; server_id={text:?}",
                text.len()
            )
        })
}

pub(super) fn is_self_connection(local_endpoint: SocketAddr, peer_endpoint: SocketAddr) -> bool {
    local_endpoint == peer_endpoint
}

fn is_retryable_io(error: &FrameIoError) -> bool {
    matches!(error, FrameIoError::Io(error) if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::UnexpectedEof
    ))
}

#[cfg(windows)]
fn windowless(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(unix)]
fn windowless(_command: &mut Command) {}

#[cfg(not(any(windows, unix)))]
fn windowless(_command: &mut Command) {}
