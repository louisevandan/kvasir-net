use super::Poll;
use p4_protocol::event::Event;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::task::{Context, Poll as TaskPoll, Waker};

#[derive(Clone)]
pub struct CompletionPublisher {
    sender: Option<mpsc::SyncSender<Event>>,
    waker: Arc<Mutex<Option<Waker>>>,
    capacity: Arc<Mutex<CapacityState>>,
}

/// Bound on simultaneously registered capacity waiters, not on publishers,
/// events, bytes or pipeline stages. Registrations are normally one per actor.
pub const MAX_CAPACITY_LISTENERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapacityListenError {
    Closed,
    /// All listener slots are occupied, or the registration ID space ended.
    Exhausted,
}

#[derive(Default, Debug)]
struct CapacityState {
    closed: bool,
    next_id: u64,
    listeners: BTreeMap<u64, Arc<Waker>>,
}

/// Persistent, bounded registration. Dropping it removes only this listener.
/// A notification already taken by a concurrent drainer can still wake once;
/// the waker remains owned for that call. Cancellation never moves an event.
#[derive(Debug)]
pub struct CapacityRegistration {
    state: Weak<Mutex<CapacityState>>,
    id: u64,
}

impl Drop for CapacityRegistration {
    fn drop(&mut self) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let removed = state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .listeners
            .remove(&self.id);
        // Dropping a caller-provided waker can itself run caller code.
        drop(removed);
    }
}

fn notify_capacity(state: &Mutex<CapacityState>, closed: bool) {
    let listeners = {
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed |= closed;
        if closed {
            std::mem::take(&mut state.listeners)
                .into_values()
                .collect::<Vec<_>>()
        } else {
            // Clone Arc, not an arbitrary RawWaker, while holding our lock.
            state.listeners.values().cloned().collect::<Vec<_>>()
        }
    };
    for waker in listeners {
        waker.wake_by_ref();
    }
}

fn wake_reader(slot: &Mutex<Option<Waker>>) {
    let waker = slot
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    if let Some(waker) = waker {
        waker.wake();
    }
}

fn clear_reader(slot: &Mutex<Option<Waker>>) {
    let waker = slot
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    drop(waker);
}

/// Why a completion could not be put in the mailbox, with the event back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// No room now. Worth waiting for; the reader will drain it.
    Full(Event),
    /// Nothing will read this mailbox again.
    Closed(Event),
}

impl CompletionPublisher {
    /// Register BEFORE trying to publish, and retain the registration while
    /// waiting. A wake means "retry": it reserves no capacity, and another
    /// producer may win the freed slot. Receiver closure also wakes listeners;
    /// the retry returns `PublishError::Closed` with the original event.
    ///
    /// To wait on this and other actor inputs, capture the actor wake epoch
    /// before testing all predicates and sleep only if that epoch is unchanged.
    /// This API does not provide an actor loop, shutdown drain or KV credit.
    /// Wakers run synchronously and must be short, nonblocking and nonpanicking.
    /// A panic propagates outside our mutexes; recovery from caller panics is
    /// not an event-delivery or shutdown guarantee of this API.
    pub fn capacity_listener(
        &self,
        waker: &Waker,
    ) -> Result<CapacityRegistration, CapacityListenError> {
        let waker = Arc::new(waker.clone());
        let mut state = self
            .capacity
            .lock()
            .map_err(|_| CapacityListenError::Closed)?;
        if state.closed {
            return Err(CapacityListenError::Closed);
        }
        if state.listeners.len() >= MAX_CAPACITY_LISTENERS {
            return Err(CapacityListenError::Exhausted);
        }
        let id = state
            .next_id
            .checked_add(1)
            .ok_or(CapacityListenError::Exhausted)?;
        state.next_id = id;
        state.listeners.insert(id, waker);
        Ok(CapacityRegistration {
            state: Arc::downgrade(&self.capacity),
            id,
        })
    }

    /// Puts a completion in the mailbox, or hands it back with the reason.
    ///
    /// The reason matters: a full queue is worth waiting on and a closed one
    /// never will be, and returning one `Err(Event)` for both left the only
    /// caller unable to do anything but drop the event and fail.
    pub fn try_publish(&self, event: Event) -> Result<(), PublishError> {
        let Some(sender) = &self.sender else {
            return Err(PublishError::Closed(event));
        };
        match sender.try_send(event) {
            Ok(()) => {
                wake_reader(&self.waker);
                Ok(())
            }
            Err(mpsc::TrySendError::Full(event)) => Err(PublishError::Full(event)),
            Err(mpsc::TrySendError::Disconnected(event)) => Err(PublishError::Closed(event)),
        }
    }
}

pub struct CompletionMailbox {
    receiver: Mutex<Option<mpsc::Receiver<Event>>>,
    waker: Arc<Mutex<Option<Waker>>>,
    capacity: Arc<Mutex<CapacityState>>,
}

impl CompletionMailbox {
    pub fn try_take(&self) -> Poll {
        let result = {
            let Ok(receiver) = self.receiver.lock() else {
                return Poll::Closed;
            };
            let Some(receiver) = receiver.as_ref() else {
                return Poll::Closed;
            };
            match receiver.try_recv() {
                Ok(event) => Poll::Event(event),
                Err(mpsc::TryRecvError::Empty) => Poll::Empty,
                Err(mpsc::TryRecvError::Disconnected) => Poll::Closed,
            }
        };
        if matches!(result, Poll::Event(_)) {
            // Neither receiver nor listener mutex is held in caller wake code.
            notify_capacity(&self.capacity, false);
        }
        result
    }

    /// One logical polling reader is supported; a later poll replaces its
    /// previous waker. Concurrent nonblocking drains remain supported.
    pub fn poll_take(&self, context: &mut Context<'_>) -> TaskPoll<Poll> {
        self.poll_take_before_register(context, || {})
    }

    // The no-op hook in production permits barrier-controlled tests of the
    // empty-read/register race without sleeps or a duplicate polling model.
    fn poll_take_before_register(
        &self,
        context: &mut Context<'_>,
        before_register: impl FnOnce(),
    ) -> TaskPoll<Poll> {
        match self.try_take() {
            Poll::Event(event) => {
                clear_reader(&self.waker);
                return TaskPoll::Ready(Poll::Event(event));
            }
            Poll::Closed => {
                clear_reader(&self.waker);
                return TaskPoll::Ready(Poll::Closed);
            }
            Poll::Empty => {}
        }
        before_register();
        let next_waker = context.waker().clone();
        let previous = match self.waker.lock() {
            Ok(mut waker) => waker.replace(next_waker),
            Err(_) => return TaskPoll::Ready(Poll::Closed),
        };
        drop(previous);
        // Arrival or last-sender closure between the first read and reader
        // registration cannot be missed: inspect again after registration.
        match self.try_take() {
            Poll::Empty => TaskPoll::Pending,
            value => {
                clear_reader(&self.waker);
                TaskPoll::Ready(value)
            }
        }
    }
}

pub fn completion_mailbox(capacity: usize) -> (CompletionPublisher, Arc<CompletionMailbox>) {
    assert!(capacity > 0, "completion mailbox capacity must be positive");
    let (sender, receiver) = mpsc::sync_channel(capacity);
    let waker = Arc::new(Mutex::new(None));
    let capacity = Arc::new(Mutex::new(CapacityState::default()));
    (
        CompletionPublisher {
            sender: Some(sender),
            waker: Arc::clone(&waker),
            capacity: Arc::clone(&capacity),
        },
        Arc::new(CompletionMailbox {
            receiver: Mutex::new(Some(receiver)),
            waker,
            capacity,
        }),
    )
}

impl Drop for CompletionPublisher {
    fn drop(&mut self) {
        // Disconnect first. Waking before sender destruction can let a reader
        // register again against an empty, still-connected channel and sleep
        // forever. Non-final sender drops may cause harmless extra wakeups.
        drop(self.sender.take());
        wake_reader(&self.waker);
    }
}

#[cfg(test)]
#[path = "mailbox_tests.rs"]
mod tests;

impl Drop for CompletionMailbox {
    fn drop(&mut self) {
        let receiver = self
            .receiver
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        drop(receiver);
        // Publishers share the slot but must not retain a departed reader task.
        // Do not run the last Waker destructor under the registration mutex.
        clear_reader(&self.waker);
        // A listener may reenter try_publish from its wake callback: the
        // receiver must already be disconnected so it gets Closed, not Full.
        notify_capacity(&self.capacity, true);
    }
}
