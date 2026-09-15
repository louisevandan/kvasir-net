//! Explicit owned adapter transport. Raw callers cannot consume its output.
use super::*;
use crate::v2::resource_profile::RuntimeResourceProbe;
use crate::v2::{ERROR_CONTENT_TYPE, LOADED_CONTENT_TYPE, UNLOADED_CONTENT_TYPE};
use p4_adapter::node_adapter::{
    AdapterLifecycleCompletion, CompletionFront, CompletionMailbox, MailboxBuildError, OwnedPoll,
    RetainedCompletion, RetainedNodeAdapter, RetainedOfferError, completion_mailbox_with_limits,
};
use p4_protocol::event::lifecycle::LifecycleOperation;

pub struct RetainedLlamaNodeAdapter {
    inner: LlamaNodeAdapter,
    stopped: Arc<AtomicBool>,
    retention: worker::retention::RetentionTracker,
    // A stopped worker's original inputs/state/effects remain owned here.
    // Dropping this adapter is explicit local abandonment, not successful
    // drain, native reconciliation, remote acceptance or permission to replay.
    _remainder: Arc<Mutex<Option<worker::WorkerRemainder>>>,
}

impl RetainedLlamaNodeAdapter {
    pub fn new(
        endpoint: Endpoint,
        input_capacity: usize,
        completion_capacity: usize,
        retained_capacity: usize,
        retained_bytes: usize,
    ) -> Result<Self, MailboxBuildError> {
        Self::new_inner(
            endpoint,
            input_capacity,
            completion_capacity,
            retained_capacity,
            retained_bytes,
            None,
        )
    }

    pub fn new_with_runtime_resource_probe(
        endpoint: Endpoint,
        input_capacity: usize,
        completion_capacity: usize,
        retained_capacity: usize,
        retained_bytes: usize,
        probe: RuntimeResourceProbe,
    ) -> Result<Self, MailboxBuildError> {
        Self::new_inner(
            endpoint,
            input_capacity,
            completion_capacity,
            retained_capacity,
            retained_bytes,
            Some(probe),
        )
    }

    fn new_inner(
        endpoint: Endpoint,
        input_capacity: usize,
        completion_capacity: usize,
        retained_capacity: usize,
        retained_bytes: usize,
        probe: Option<RuntimeResourceProbe>,
    ) -> Result<Self, MailboxBuildError> {
        if input_capacity == 0 {
            return Err(MailboxBuildError::InvalidCapacity);
        }
        let (publisher, mailbox) =
            completion_mailbox_with_limits(completion_capacity, retained_capacity, retained_bytes)?;
        let (sender, receiver) = mpsc::sync_channel(input_capacity);
        let snapshot = Arc::new(Mutex::new("empty".to_owned()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut worker = Worker::new(
            endpoint,
            receiver,
            publisher,
            snapshot.clone(),
            shutdown.clone(),
        );
        if let Some(probe) = probe {
            worker = worker.with_runtime_resource_probe(probe);
        }
        Ok(Self::spawn_worker(
            sender, mailbox, snapshot, shutdown, worker,
        ))
    }

    pub(super) fn spawn_worker(
        sender: mpsc::SyncSender<WorkerInput>,
        mailbox: Arc<CompletionMailbox>,
        snapshot: Arc<Mutex<String>>,
        shutdown: Arc<AtomicBool>,
        worker: Worker,
    ) -> Self {
        let retention = worker.retention_tracker();
        let stopped = Arc::new(AtomicBool::new(false));
        let stopped_on_exit = stopped.clone();
        let remainder = Arc::new(Mutex::new(None));
        let retain_on_exit = remainder.clone();
        let thread = std::thread::Builder::new()
            .name("p4-llamacpp-owned-node".into())
            .spawn(move || {
                let remainder = worker.run_owned();
                stopped_on_exit.store(true, Ordering::Release);
                *retain_on_exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(remainder);
            })
            .expect("llama owned worker thread must start");
        Self {
            inner: LlamaNodeAdapter {
                sender: Some(sender),
                mailbox,
                snapshot,
                shutting_down: shutdown,
                worker: Mutex::new(Some(thread)),
            },
            stopped,
            retention,
            _remainder: remainder,
        }
    }
}

impl RetainedNodeAdapter for RetainedLlamaNodeAdapter {
    fn completion_storage_snapshot(
        &self,
    ) -> Option<p4_adapter::node_adapter::CompletionStorageSnapshot> {
        Some(self.inner.mailbox.storage_snapshot())
    }
    fn retention_snapshot(&self) -> Option<p4_adapter::node_adapter::AdapterRetentionSnapshot> {
        Some(self.retention.snapshot())
    }
    fn try_offer_retained(&self, completion: RetainedCompletion) -> Result<(), RetainedOfferError> {
        if completion.event().validate().is_err() {
            return Err(RetainedOfferError::Closed(completion));
        }
        if self.stopped.load(Ordering::Acquire) || self.inner.shutting_down.load(Ordering::Acquire)
        {
            return Err(RetainedOfferError::Closed(completion));
        }
        let Some(sender) = &self.inner.sender else {
            return Err(RetainedOfferError::Closed(completion));
        };
        // The received claim continues to account for this SAME allocation
        // in the bounded std channel, worker call or ACK obstruction. Parsing
        // copies and future native output require their own separate budgets.
        match sender.try_send(WorkerInput::Retained(completion)) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(WorkerInput::Retained(completion))) => {
                Err(RetainedOfferError::Full(completion))
            }
            Err(mpsc::TrySendError::Disconnected(WorkerInput::Retained(completion))) => {
                Err(RetainedOfferError::Closed(completion))
            }
            Err(_) => unreachable!("owned offer sent an owned input"),
        }
    }
    fn peek_retained_completion(&self) -> Option<CompletionFront> {
        self.inner.mailbox.peek_owned_front()
    }
    fn try_take_retained_matching(&self, expected: &CompletionFront) -> OwnedPoll {
        self.inner.mailbox.try_take_owned_matching(expected)
    }
    fn poll_take_retained(&self, context: &mut Context<'_>) -> TaskPoll<OwnedPoll> {
        self.inner.mailbox.poll_take_owned(context)
    }
    fn snapshot(&self) -> String {
        self.inner.snapshot()
    }
    fn decode_lifecycle_completion(
        &self,
        operation: LifecycleOperation,
        event: &Event,
    ) -> Result<AdapterLifecycleCompletion, String> {
        let allowed = match operation {
            LifecycleOperation::Load => {
                event.envelope.payload_content_type == LOADED_CONTENT_TYPE
                    || event.envelope.payload_content_type == ERROR_CONTENT_TYPE
            }
            LifecycleOperation::Unload => {
                event.envelope.payload_content_type == UNLOADED_CONTENT_TYPE
                    || event.envelope.payload_content_type == ERROR_CONTENT_TYPE
            }
        };
        if !allowed {
            return Err("unexpected llama lifecycle completion content type".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid llama lifecycle result: {error}"))?;
        let completion: AdapterLifecycleCompletion = serde_json::from_value(
            value
                .get("lifecycle")
                .cloned()
                .ok_or("llama lifecycle result omits typed completion")?,
        )
        .map_err(|error| format!("invalid llama typed lifecycle result: {error}"))?;
        if completion.operation != operation {
            return Err("llama lifecycle completion operation mismatch".into());
        }
        completion.validate().map_err(str::to_owned)?;
        Ok(completion)
    }
}

#[cfg(test)]
mod tests;
