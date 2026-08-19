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

/// Configuration for one stage-server process. The adapter receives the
/// opaque plan from OUTER and forwards its bytes unchanged to this process.
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

/// The concrete OS process/socket owner used by the staged adapter.
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
        let (server_id, transactions) = decode_hello(&response.body)?;
        self.server_id = Some(server_id.clone());
        Ok(Some(ReadyInfo {
            protocol_revision: PROTOCOL_REVISION,
            server_id,
            transactions,
        }))
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
        // Closing stdin is the abnormal-parent signal only after UNLOAD has
        // been sent. It also guarantees cleanup if the server has no response.
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

fn decode_hello(body: &[u8]) -> Result<(String, bool), String> {
    if body.len() < 2 {
        return Err("stage server HELLO body is truncated".into());
    }
    let revision = u16::from_le_bytes([body[0], body[1]]);
    if revision != PROTOCOL_REVISION {
        return Err(format!(
            "stage server protocol revision {revision} is unsupported"
        ));
    }
    let id = if body.len() == 2 { &[][..] } else { &body[2..] };
    let text = String::from_utf8(id.to_vec())
        .map_err(|_| "stage server HELLO id is not UTF-8".to_owned())?;
    let transactions = text.split(';').any(|field| field == "transactions=1");
    Ok((text, transactions))
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
