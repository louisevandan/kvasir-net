use p4_adapter::node_adapter::{AdapterRetainedStorage, AdapterRetentionSnapshot};
use std::ops::Deref;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub(in crate::v2::node) struct RetentionTracker(Arc<Mutex<AdapterRetentionSnapshot>>);

impl RetentionTracker {
    pub(in crate::v2::node) fn snapshot(&self) -> AdapterRetentionSnapshot {
        *self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(super) fn set_pending(&self, count: usize, bytes: usize) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending_requests = AdapterRetainedStorage { count, bytes };
    }

    pub(super) fn hold_native(&self, body: Vec<u8>) -> Result<NativeResponse, String> {
        let bytes = body.capacity();
        let mut state = self
            .0
            .lock()
            .map_err(|_| "adapter retention snapshot is poisoned")?;
        if state.native_responses.count != 0 {
            return Err("a native response buffer is already retained".into());
        }
        state.native_responses = AdapterRetainedStorage { count: 1, bytes };
        drop(state);
        Ok(NativeResponse {
            body,
            tracker: self.clone(),
        })
    }
}

#[derive(Debug)]
pub(super) struct NativeResponse {
    body: Vec<u8>,
    tracker: RetentionTracker,
}

impl NativeResponse {
    pub(super) fn into_vec(mut self) -> Vec<u8> {
        std::mem::take(&mut self.body)
    }
}

impl Deref for NativeResponse {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

impl Drop for NativeResponse {
    fn drop(&mut self) {
        self.tracker
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .native_responses = AdapterRetainedStorage::default();
    }
}
