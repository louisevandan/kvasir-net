use super::Poll;
use p4_protocol::event::Event;
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll as TaskPoll, Waker};

#[derive(Clone)]
pub struct CompletionPublisher {
    sender: mpsc::SyncSender<Event>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl CompletionPublisher {
    pub fn try_publish(&self, event: Event) -> Result<(), Event> {
        match self.sender.try_send(event) {
            Ok(()) => {
                if let Ok(mut waker) = self.waker.lock()
                    && let Some(waker) = waker.take()
                {
                    waker.wake();
                }
                Ok(())
            }
            Err(mpsc::TrySendError::Full(event) | mpsc::TrySendError::Disconnected(event)) => {
                Err(event)
            }
        }
    }
}

pub struct CompletionMailbox {
    receiver: Mutex<mpsc::Receiver<Event>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl CompletionMailbox {
    pub fn try_take(&self) -> Poll {
        let Ok(receiver) = self.receiver.lock() else {
            return Poll::Closed;
        };
        match receiver.try_recv() {
            Ok(event) => Poll::Event(event),
            Err(mpsc::TryRecvError::Empty) => Poll::Empty,
            Err(mpsc::TryRecvError::Disconnected) => Poll::Closed,
        }
    }

    pub fn poll_take(&self, context: &mut Context<'_>) -> TaskPoll<Poll> {
        match self.try_take() {
            Poll::Event(event) => return TaskPoll::Ready(Poll::Event(event)),
            Poll::Closed => return TaskPoll::Ready(Poll::Closed),
            Poll::Empty => {}
        }
        if let Ok(mut waker) = self.waker.lock() {
            *waker = Some(context.waker().clone());
        } else {
            return TaskPoll::Ready(Poll::Closed);
        }
        // Publishing between the first read and waker registration cannot be
        // missed: inspect once more after installing the waker.
        match self.try_take() {
            Poll::Empty => TaskPoll::Pending,
            value => TaskPoll::Ready(value),
        }
    }
}

pub fn completion_mailbox(capacity: usize) -> (CompletionPublisher, Arc<CompletionMailbox>) {
    assert!(capacity > 0, "completion mailbox capacity must be positive");
    let (sender, receiver) = mpsc::sync_channel(capacity);
    let waker = Arc::new(Mutex::new(None));
    (
        CompletionPublisher {
            sender,
            waker: Arc::clone(&waker),
        },
        Arc::new(CompletionMailbox {
            receiver: Mutex::new(receiver),
            waker,
        }),
    )
}
