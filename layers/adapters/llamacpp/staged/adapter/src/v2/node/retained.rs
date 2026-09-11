//! Explicit owned adapter transport. Raw callers cannot consume its output.
use super::*;
use p4_adapter::node_adapter::{
    CompletionFront, CompletionMailbox, MailboxBuildError, OwnedPoll, RetainedCompletion,
    RetainedNodeAdapter, RetainedOfferError, completion_mailbox_with_limits,
};

pub struct RetainedLlamaNodeAdapter {
    inner: LlamaNodeAdapter,
    stopped: Arc<AtomicBool>,
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
        if input_capacity == 0 {
            return Err(MailboxBuildError::InvalidCapacity);
        }
        let (publisher, mailbox) =
            completion_mailbox_with_limits(completion_capacity, retained_capacity, retained_bytes)?;
        let (sender, receiver) = mpsc::sync_channel(input_capacity);
        let snapshot = Arc::new(Mutex::new("empty".to_owned()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker = Worker::new(
            endpoint,
            receiver,
            publisher,
            snapshot.clone(),
            shutdown.clone(),
        );
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
            _remainder: remainder,
        }
    }
}

impl RetainedNodeAdapter for RetainedLlamaNodeAdapter {
    fn completion_storage_snapshot(&self) -> Option<p4_adapter::node_adapter::CompletionStorageSnapshot> {
        Some(self.inner.mailbox.storage_snapshot())
    }
    fn try_offer_retained(&self, completion: RetainedCompletion) -> Result<(), RetainedOfferError> {
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
}

#[cfg(test)]
mod tests;
