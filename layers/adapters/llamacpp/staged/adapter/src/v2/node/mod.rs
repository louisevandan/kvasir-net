use self::worker::{Worker, WorkerInput};
use p4_adapter::node_adapter::{NodeAdapter, OfferError, Poll, completion_mailbox};
use p4_protocol::event::{Endpoint, Event};
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
        let worker = std::thread::Builder::new()
            .name("p4-llamacpp-node".into())
            .spawn(move || Worker::new(endpoint, receiver, publisher, worker_snapshot).run())
            .expect("llama adapter worker thread must start");
        Self {
            sender: Some(sender),
            mailbox,
            snapshot,
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
        self.sender.take();
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }
}
