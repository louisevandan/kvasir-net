//! Concrete-adapter ownership of one server object per model load.

use std::time::Duration;

use crate::process::{ProcessError, ServerControl, ServerProcess};
use crate::protocol::Frame;

#[derive(Debug, Eq, PartialEq)]
pub enum LifecycleError {
    InvalidState(LoadState),
    Process(ProcessError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadState {
    Empty,
    Loading,
    Loaded,
    Unloading,
    Unloaded,
    Failed,
}

impl LoadState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Unloaded | Self::Failed)
    }
}

pub struct LlamaLifecycle<C> {
    state: LoadState,
    server: Option<ServerProcess<C>>,
}

impl<C> Default for LlamaLifecycle<C> {
    fn default() -> Self {
        Self {
            state: LoadState::Empty,
            server: None,
        }
    }
}

impl<C> LlamaLifecycle<C> {
    pub fn state(&self) -> LoadState {
        self.state
    }
    pub fn has_server(&self) -> bool {
        self.server.is_some()
    }

    pub fn transaction_capable(&self) -> bool {
        self.server
            .as_ref()
            .and_then(|server| server.ready_info())
            .is_some_and(|ready| ready.transactions)
    }

    pub fn physical_batch_capable(&self) -> bool {
        self.server
            .as_ref()
            .and_then(|server| server.ready_info())
            .is_some_and(|ready| ready.physical_batch)
    }

    pub fn ready_info(&self) -> Option<&crate::process::ReadyInfo> {
        self.server.as_ref().and_then(|server| server.ready_info())
    }
}

impl<C: ServerControl> LlamaLifecycle<C> {
    pub fn load(&mut self, control: C, timeout: Duration) -> Result<(), LifecycleError> {
        if !matches!(
            self.state,
            LoadState::Empty | LoadState::Unloaded | LoadState::Failed
        ) {
            return Err(LifecycleError::InvalidState(self.state));
        }
        // Dropping a failed owner invokes its ServerControl cleanup before a
        // replacement is installed. Reload never shares a child or socket.
        self.server.take();
        self.state = LoadState::Loading;
        let mut server = ServerProcess::new(control);
        if let Err(error) = server.start_and_wait_ready(timeout) {
            self.state = LoadState::Failed;
            self.server = Some(server);
            return Err(LifecycleError::Process(error));
        }
        self.server = Some(server);
        self.state = LoadState::Loaded;
        Ok(())
    }

    pub fn unload(&mut self) -> Result<(), LifecycleError> {
        if self.state != LoadState::Loaded {
            return Err(LifecycleError::InvalidState(self.state));
        }
        self.state = LoadState::Unloading;
        let result = self
            .server
            .as_mut()
            .expect("loaded lifecycle owns server")
            .unload();
        match result {
            Ok(()) => {
                self.server = None;
                self.state = LoadState::Unloaded;
                Ok(())
            }
            Err(error) => {
                self.state = LoadState::Failed;
                Err(LifecycleError::Process(error))
            }
        }
    }

    /// Attempt cleanup after LOAD or UNLOAD entered a failed state.
    /// The server remains owned on failure so callers cannot report absence.
    pub fn cleanup_failed(&mut self) -> Result<(), LifecycleError> {
        if self.state != LoadState::Failed {
            return Err(LifecycleError::InvalidState(self.state));
        }
        self.state = LoadState::Unloading;
        let result = self
            .server
            .as_mut()
            .expect("failed lifecycle owns server")
            .cleanup_failed();
        match result {
            Ok(()) => {
                self.server = None;
                self.state = LoadState::Unloaded;
                Ok(())
            }
            Err(error) => {
                self.state = LoadState::Failed;
                Err(LifecycleError::Process(error))
            }
        }
    }

    pub fn request(&mut self, request: Frame) -> Result<Frame, LifecycleError> {
        if self.state != LoadState::Loaded {
            return Err(LifecycleError::InvalidState(self.state));
        }
        self.server
            .as_mut()
            .expect("loaded lifecycle owns server")
            .request(request)
            .map_err(LifecycleError::Process)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[derive(Default)]
    struct Fake {
        crash: bool,
    }
    impl ServerControl for Fake {
        fn start(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn wait_ready(
            &mut self,
            _deadline: Instant,
        ) -> Result<Option<crate::process::ReadyInfo>, String> {
            if self.crash {
                Err("crashed".into())
            } else {
                Ok(Some(crate::process::ReadyInfo {
                    physical_identity_revision: 1,
                    protocol_revision: 1,
                    server_id: "fake".into(),
                    transactions: false,
                    physical_batch: true,
                    equal_sequence_ubatch: false,
                    max_atomic_sequences: 1,
                    atomic_batch_exclusive: false,
                    n_ctx: 512,
                    n_batch: 64,
                    n_ubatch: 64,
                    n_seq_max: 1,
                    physical_result_payload_bytes: 0,
                    physical_result_tensor_count: 0,
                    max_physical_result_bytes: 33_554_432,
                    upstream_commit: "fixture-upstream".into(),
                    patch_set: "fixture-patch-set".into(),
                    backend_inventory: "fixture-backend".into(),
                    stage_wire_abi: "unknown".into(),
                }))
            }
        }
        fn shutdown(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn load_creates_one_server_and_unload_removes_it() {
        let mut lifecycle = LlamaLifecycle::default();
        lifecycle
            .load(Fake::default(), Duration::from_millis(10))
            .unwrap();
        assert_eq!(lifecycle.state(), LoadState::Loaded);
        assert!(lifecycle.has_server());
        lifecycle.unload().unwrap();
        assert_eq!(lifecycle.state(), LoadState::Unloaded);
        assert!(!lifecycle.has_server());
        assert!(lifecycle.unload().is_err());
    }

    #[test]
    fn crash_during_ready_is_terminal_and_keeps_failed_ownership_for_cleanup() {
        let mut lifecycle = LlamaLifecycle::default();
        assert!(
            lifecycle
                .load(Fake { crash: true }, Duration::from_millis(10))
                .is_err()
        );
        assert_eq!(lifecycle.state(), LoadState::Failed);
        assert!(lifecycle.state().is_terminal());
        assert!(lifecycle.has_server());
        lifecycle
            .load(Fake::default(), Duration::from_millis(10))
            .unwrap();
        assert_eq!(lifecycle.state(), LoadState::Loaded);
    }

    #[test]
    fn failed_load_cleanup_separates_absent_from_unknown_resources() {
        let mut cleaned = LlamaLifecycle::default();
        assert!(
            cleaned
                .load(Fake { crash: true }, Duration::from_millis(10))
                .is_err()
        );
        cleaned.cleanup_failed().unwrap();
        assert_eq!(cleaned.state(), LoadState::Unloaded);
        assert!(!cleaned.has_server());

        struct CleanupFailure;
        impl ServerControl for CleanupFailure {
            fn start(&mut self) -> Result<(), String> {
                Ok(())
            }
            fn wait_ready(
                &mut self,
                _: Instant,
            ) -> Result<Option<crate::process::ReadyInfo>, String> {
                Err("ready failed".into())
            }
            fn shutdown(&mut self) -> Result<(), String> {
                Err("cleanup failed".into())
            }
        }
        let mut unknown = LlamaLifecycle::default();
        assert!(
            unknown
                .load(CleanupFailure, Duration::from_millis(10))
                .is_err()
        );
        assert!(unknown.cleanup_failed().is_err());
        assert_eq!(unknown.state(), LoadState::Failed);
        assert!(unknown.has_server());
    }

    #[test]
    fn unloaded_runtime_can_be_loaded_again_as_a_fresh_server() {
        let mut lifecycle = LlamaLifecycle::default();
        lifecycle
            .load(Fake::default(), Duration::from_millis(10))
            .unwrap();
        lifecycle.unload().unwrap();
        lifecycle
            .load(Fake::default(), Duration::from_millis(10))
            .unwrap();
        assert_eq!(lifecycle.state(), LoadState::Loaded);
    }
}
