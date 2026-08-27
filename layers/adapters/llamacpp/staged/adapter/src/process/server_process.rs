use super::*;

pub struct ServerProcess<C> {
    control: C,
    state: ProcessState,
    ready: Option<ReadyInfo>,
}

impl<C> ServerProcess<C> {
    pub fn new(control: C) -> Self {
        Self {
            control,
            state: ProcessState::Created,
            ready: None,
        }
    }

    pub fn state(&self) -> ProcessState {
        self.state
    }

    pub fn ready_info(&self) -> Option<&ReadyInfo> {
        self.ready.as_ref()
    }
}

impl<C: ServerControl> ServerProcess<C> {
    pub fn start_and_wait_ready(&mut self, timeout: Duration) -> Result<&ReadyInfo, ProcessError> {
        if self.state != ProcessState::Created {
            return Err(ProcessError::InvalidState(self.state));
        }
        self.state = ProcessState::Starting;
        if let Err(error) = self.control.start() {
            self.state = ProcessState::Crashed;
            return Err(ProcessError::StartFailed(error));
        }
        self.state = ProcessState::AwaitingReady;
        let deadline = Instant::now() + timeout;
        loop {
            match self.control.wait_ready(deadline) {
                Ok(Some(info)) => {
                    self.ready = Some(info);
                    self.state = ProcessState::Ready;
                    return Ok(self.ready.as_ref().expect("ready state stores info"));
                }
                Ok(None) if Instant::now() >= deadline => {
                    self.state = ProcessState::TimedOut;
                    return Err(ProcessError::ReadyTimeout(timeout));
                }
                Ok(None) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    std::thread::sleep(remaining.min(Duration::from_millis(50)));
                }
                Err(error) => {
                    self.state = ProcessState::Crashed;
                    return Err(ProcessError::ReadyFailed(error));
                }
            }
        }
    }

    pub fn unload(&mut self) -> Result<(), ProcessError> {
        if self.state != ProcessState::Ready {
            return Err(ProcessError::InvalidState(self.state));
        }
        self.state = ProcessState::Stopping;
        match self.control.shutdown() {
            Ok(()) => {
                self.state = ProcessState::Exited;
                self.ready = None;
                Ok(())
            }
            Err(error) => {
                self.state = ProcessState::Crashed;
                Err(ProcessError::ShutdownFailed(error))
            }
        }
    }

    pub fn request(&mut self, request: Frame) -> Result<Frame, ProcessError> {
        if self.state != ProcessState::Ready {
            return Err(ProcessError::InvalidState(self.state));
        }
        self.control
            .request(request)
            .map_err(ProcessError::RequestFailed)
    }
}
