use self::worker::{Worker, WorkerInput};
use p4_adapter::node_adapter::{NodeAdapter, OfferError, Poll, completion_mailbox};
use p4_protocol::event::{Endpoint, Event};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll as TaskPoll};
use std::thread::JoinHandle;

// Visible to the crate so the open-batch ledger can be tested on the real
// state machine rather than on a copy of its arithmetic.
pub(crate) mod state;
mod worker;

pub struct LlamaNodeAdapter {
    sender: Option<mpsc::SyncSender<WorkerInput>>,
    mailbox: Arc<p4_adapter::node_adapter::CompletionMailbox>,
    snapshot: Arc<Mutex<String>>,
    /// Set before the worker is joined, so a worker waiting for room in a
    /// full completion mailbox stops waiting.
    ///
    /// Without it the two wait on each other: `Drop` closes the inbound
    /// channel and joins, but the mailbox receiver is a field of this struct
    /// and so outlives `drop`, so the worker never sees `Closed` and retries
    /// for ever. Closing the receiver first would work too and is worse - a
    /// completion still in the mailbox would be dropped by a reader that had
    /// gone, where this way the worker abandons only the one it is holding
    /// and says so in its snapshot.
    shutting_down: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl LlamaNodeAdapter {
    pub fn new(endpoint: Endpoint, queue_capacity: usize, completion_capacity: usize) -> Self {
        assert!(
            queue_capacity > 0,
            "llama adapter queue capacity must be positive"
        );
        let (sender, receiver) = mpsc::sync_channel(queue_capacity);
        let (publisher, mailbox) = completion_mailbox(completion_capacity);
        let snapshot = Arc::new(Mutex::new("empty".to_owned()));
        let worker_snapshot = Arc::clone(&snapshot);
        let shutting_down = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutting_down);
        let worker = std::thread::Builder::new()
            .name("p4-llamacpp-node".into())
            .spawn(move || {
                Worker::new(endpoint, receiver, publisher, worker_snapshot, worker_shutdown)
                    .run()
            })
            .expect("llama adapter worker thread must start");
        Self {
            sender: Some(sender),
            mailbox,
            snapshot,
            shutting_down,
            worker: Mutex::new(Some(worker)),
        }
    }
}

impl NodeAdapter for LlamaNodeAdapter {
    fn kind(&self) -> &str {
        "llamacpp"
    }

    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        let Some(sender) = &self.sender else {
            return Err(OfferError::Closed);
        };
        match sender.try_send(WorkerInput::Event(event)) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(WorkerInput::Event(event))) => {
                Err(OfferError::Full(event))
            }
            Err(mpsc::TrySendError::Full(_)) => Err(OfferError::Closed),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(OfferError::Closed),
        }
    }

    fn try_take(&self) -> Poll {
        self.mailbox.try_take()
    }

    fn poll_take(&self, context: &mut Context<'_>) -> TaskPoll<Poll> {
        self.mailbox.poll_take(context)
    }

    fn snapshot(&self) -> String {
        self.snapshot
            .lock()
            .map_or_else(|_| "poisoned".into(), |value| value.clone())
    }
}

impl Drop for LlamaNodeAdapter {
    fn drop(&mut self) {
        // Before the join, not after: a worker waiting for mailbox room has
        // to be told to stop waiting, or the join never returns.
        self.shutting_down.store(true, Ordering::SeqCst);
        self.sender.take();
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}
