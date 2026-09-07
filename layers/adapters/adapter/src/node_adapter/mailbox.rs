use super::{Poll, retained_event_bytes};
use p4_protocol::event::Event;
use std::collections::{BTreeMap, VecDeque};
#[cfg(test)]
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll as TaskPoll, Waker};

#[path = "mailbox_group.rs"]
mod group;
pub use group::{CompletionReservationGroup, GroupReserveError};

pub struct CompletionPublisher {
    receiver: Arc<Mutex<Storage>>,
    budget: Arc<Mutex<Budget>>,
    waker: Arc<Mutex<Option<Waker>>>,
    capacity: Arc<Mutex<CapacityState>>,
}

/// Listener slots, not event, byte, publisher or pipeline-stage capacity.
pub const MAX_CAPACITY_LISTENERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapacityListenError {
    Closed,
    Exhausted,
}

#[derive(Default, Debug)]
struct CapacityState {
    closed: bool,
    next_id: u64,
    listeners: BTreeMap<u64, Arc<Waker>>,
}

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
            .unwrap_or_else(|e| e.into_inner())
            .listeners
            .remove(&self.id);
        drop(removed); // Caller RawWaker destructor is outside the mutex.
    }
}

fn notify_capacity(state: &Mutex<CapacityState>, closed: bool) {
    let listeners = {
        let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed |= closed;
        if closed {
            std::mem::take(&mut state.listeners)
                .into_values()
                .collect::<Vec<_>>()
        } else {
            state.listeners.values().cloned().collect::<Vec<_>>()
        }
    };
    for waker in listeners {
        waker.wake_by_ref();
    }
}

fn wake_reader(slot: &Mutex<Option<Waker>>) {
    let waker = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(waker) = waker {
        waker.wake();
    }
}

fn clear_reader(slot: &Mutex<Option<Waker>>) {
    let waker = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    drop(waker);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    Full(Event),
    Closed(Event),
    TooLarge {
        event: Event,
        required: usize,
        limit: usize,
    },
    CostOverflow(Event),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReserveError {
    Full,
    Closed,
    TooLarge { required: usize, limit: usize },
    InvalidCount,
    CostOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxBuildError {
    InvalidCapacity,
    StorageOverflow,
    AllocationFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservedPublishReason {
    Full,
    Closed,
    WrongMailbox,
    TooSmall { required: usize, reserved: usize },
    CostOverflow,
}

#[derive(Debug)]
pub struct ReservedPublishError {
    pub event: Event,
    pub reservation: CompletionReservation,
    pub reason: ReservedPublishReason,
}

#[derive(Debug)]
pub struct RetainedTransferError {
    pub completion: RetainedCompletion,
    pub reservation: CompletionReservation,
    pub reason: ReservedPublishReason,
}

struct Storage {
    queue: VecDeque<Entry>,
    queue_capacity: usize,
    closed: bool,
    publishers: usize,
    backing_bytes: usize,
}

struct Budget {
    // Includes queued Events, held completions and unpublished reservations.
    capacity: usize,
    byte_limit: Option<usize>,
    used_count: usize,
    used_bytes: usize,
    closed: bool,
}

// Event drops before Claim: a capacity wake must not advertise memory that is
// still owned by this entry. No Entry is destroyed while holding Storage.
struct Entry {
    event: Event,
    claim: Claim,
    reserved: bool,
}

/// Added once to the caller's Event footprint per reserved/queued/held entry.
/// The queue's preallocated backing is reported separately. Neither number
/// includes allocator metadata, arbitrary Wakers, native allocations or RSS.
pub const COMPLETION_ENTRY_OVERHEAD_BYTES: usize =
    std::mem::size_of::<Entry>() - std::mem::size_of::<Event>();

struct Claim {
    budget: Arc<Mutex<Budget>>,
    capacity: Arc<Mutex<CapacityState>>,
    bytes: usize,
    active: bool,
}

impl Claim {
    // Group cleanup retires all of its owned storage before the one callback.
    // The explicit state also prevents field Drop from returning it twice if
    // that callback unwinds. Normal single-claim Drop keeps its existing wake.
    fn release_quiet(&mut self) -> bool {
        if !self.active {
            return false;
        }
        let mut budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        let next_count = budget
            .used_count
            .checked_sub(1)
            .expect("claim owns one entry");
        let next_bytes = budget
            .used_bytes
            .checked_sub(self.bytes)
            .expect("claim owns its bytes");
        budget.used_count = next_count;
        budget.used_bytes = next_bytes;
        self.active = false;
        true
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        if self.release_quiet() && !std::thread::panicking() {
            notify_capacity(&self.capacity, false);
        }
    }
}

/// Move-only storage permission, NOT source authentication, wire identity,
/// a future native-result budget, or proof that any KV operation completed.
/// Dropping an unused reservation cancels it and returns its exact claim.
pub struct CompletionReservation {
    claim: Claim,
}

impl std::fmt::Debug for CompletionReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompletionReservation")
            .field("retained_bytes", &self.claim.bytes)
            .finish()
    }
}

impl CompletionReservation {
    pub fn retained_bytes(&self) -> usize {
        self.claim.bytes
    }
}

/// Move-only completion of an already accepted enqueue. Call `notify` exactly
/// once, after releasing all caller locks, to wake the destination reader and
/// any source-capacity waiters. A successful deferred transfer keeps its old
/// source claim until this receipt is consumed or dropped; destination storage
/// already owns the Event and its independent claim.
///
/// Dropping this receipt quietly retires the old source claim but deliberately
/// sends no notification and does not undo enqueue. Omitted notification can
/// stall a reader and is a caller liveness error, not a storage leak. The receipt
/// captures only a weak reader slot, not a caller Waker; Drop neither clears a
/// later registration nor executes its destructor. Notifications are local
/// storage signals, not delivery, source authentication or KV completion.
/// Deferral does not hide the accepted Event: an independently polling reader
/// may dequeue it before `notify`. A callback panic cannot undo acceptance and
/// must not be interpreted by the caller as permission to resend the Event.
///
/// ```compile_fail
/// use p4_adapter::node_adapter::DeferredCompletionNotification;
/// fn duplicate(receipt: DeferredCompletionNotification) {
///     let _copy = receipt.clone();
/// }
/// ```
#[must_use = "an accepted deferred enqueue must be notified after caller locks are released"]
pub struct DeferredCompletionNotification {
    reader: Weak<Mutex<Option<Waker>>>,
    source_claim: Option<Claim>,
}

impl std::fmt::Debug for DeferredCompletionNotification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeferredCompletionNotification")
            .field("source_claim_pending", &self.source_claim.is_some())
            .finish()
    }
}

impl DeferredCompletionNotification {
    /// Return all old-source accounting before the first callback. Callbacks
    /// retain the existing nonpanicking contract: a first panic propagates,
    /// with no second callback or duplicate accounting return during unwind.
    pub fn notify(mut self) {
        let source_capacity = self
            .source_claim
            .as_ref()
            .map(|claim| Arc::clone(&claim.capacity));
        self.release_source_quiet();
        if let Some(reader) = self.reader.upgrade() {
            wake_reader(&reader);
        }
        if let Some(capacity) = source_capacity {
            notify_capacity(&capacity, false);
        }
    }

    fn release_source_quiet(&mut self) {
        if let Some(mut claim) = self.source_claim.take() {
            claim.release_quiet();
            drop(claim); // Inactive: Drop cannot run a capacity callback.
        }
    }
}

impl Drop for DeferredCompletionNotification {
    fn drop(&mut self) {
        self.release_source_quiet();
    }
}

/// Owns the immutable Event and its charge even after dequeue. There is no
/// uncharged into_event escape. retire destroys the Event; transfer_to
/// releases this claim only after the next real queue accepted its own claim.
pub struct RetainedCompletion {
    event: Option<Event>,
    claim: Option<Claim>,
}

impl std::fmt::Debug for RetainedCompletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedCompletion")
            .field("event", &self.event)
            .field("retained_bytes", &self.retained_bytes())
            .finish()
    }
}

impl RetainedCompletion {
    pub fn event(&self) -> &Event {
        self.event.as_ref().expect("retained Event is owned")
    }
    pub fn retained_bytes(&self) -> usize {
        self.claim.as_ref().expect("retained claim is owned").bytes
    }
    pub fn retire(self) {
        drop(self);
    }
    pub fn transfer_to(
        self,
        destination: &CompletionPublisher,
        reservation: CompletionReservation,
    ) -> Result<(), RetainedTransferError> {
        self.transfer_to_deferred(destination, reservation)?
            .notify();
        Ok(())
    }

    /// Commit only destination ownership here. The success receipt retains
    /// the old claim until notification outside caller locks (or quiet Drop).
    /// Every refusal returns the exact Event plus both original claims.
    pub fn transfer_to_deferred(
        mut self,
        destination: &CompletionPublisher,
        reservation: CompletionReservation,
    ) -> Result<DeferredCompletionNotification, RetainedTransferError> {
        let event = self.event.take().expect("retained Event is owned");
        match destination.publish_reserved_deferred(event, reservation) {
            Ok(mut notification) => {
                // The receiver owns Event+new claim before ours may retire.
                notification.source_claim = self.claim.take();
                Ok(notification)
            }
            Err(error) => {
                self.event = Some(error.event);
                Err(RetainedTransferError {
                    completion: self,
                    reservation: error.reservation,
                    reason: error.reason,
                })
            }
        }
    }
}

impl Drop for RetainedCompletion {
    fn drop(&mut self) {
        drop(self.event.take());
        drop(self.claim.take());
    }
}

#[derive(Debug)]
pub enum OwnedPoll {
    Event(RetainedCompletion),
    Empty,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionStorageSnapshot {
    /// Retained storage count, including reservations and owned dequeues.
    pub capacity: usize,
    /// Delivery queue slots; these can be fewer than retained storage claims.
    pub queue_capacity: usize,
    pub byte_limit: Option<usize>,
    pub retained_count: usize,
    pub retained_bytes: usize,
    pub queued_count: usize,
    pub queue_backing_bytes: usize,
    pub closed: bool,
}

impl Clone for CompletionPublisher {
    fn clone(&self) -> Self {
        {
            let mut storage = self.receiver.lock().unwrap_or_else(|e| e.into_inner());
            storage.publishers = storage
                .publishers
                .checked_add(1)
                .expect("publisher count overflow");
        }
        Self {
            receiver: Arc::clone(&self.receiver),
            budget: Arc::clone(&self.budget),
            waker: Arc::clone(&self.waker),
            capacity: Arc::clone(&self.capacity),
        }
    }
}

impl CompletionPublisher {
    /// Register before testing capacity and retain while waiting. Wake means
    /// retry, not reservation. Callbacks must be short/nonblocking/nonpanicking.
    /// A violating callback's first panic propagates; claim destruction during
    /// unwind still returns storage but does not invoke capacity callbacks again.
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

    /// Reserve exactly one retained storage claim and the supplied Event
    /// footprint plus COMPLETION_ENTRY_OVERHEAD_BYTES. The same budget backs
    /// ordinary publications, reservations and dequeued owned completions.
    /// This does not reserve an immediate delivery queue slot.
    pub fn try_reserve(
        &self,
        count: usize,
        retained_bytes: usize,
    ) -> Result<CompletionReservation, ReserveError> {
        if count != 1 {
            return Err(ReserveError::InvalidCount);
        }
        let bytes = retained_bytes
            .checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES)
            .ok_or(ReserveError::CostOverflow)?;
        {
            let mut budget = self.budget.lock().map_err(|_| ReserveError::Closed)?;
            if budget.closed {
                return Err(ReserveError::Closed);
            }
            if let Some(limit) = budget.byte_limit.filter(|&limit| bytes > limit) {
                return Err(ReserveError::TooLarge {
                    required: bytes,
                    limit,
                });
            }
            // An individually valid cost can become feasible after other claims
            // retire; aggregate arithmetic exhaustion is therefore temporary Full.
            let next_count = budget.used_count.checked_add(1).ok_or(ReserveError::Full)?;
            let next_bytes = budget
                .used_bytes
                .checked_add(bytes)
                .ok_or(ReserveError::Full)?;
            if next_count > budget.capacity
                || budget.byte_limit.is_some_and(|limit| next_bytes > limit)
            {
                return Err(ReserveError::Full);
            }
            budget.used_count = next_count;
            budget.used_bytes = next_bytes;
        }
        Ok(CompletionReservation {
            claim: Claim {
                budget: Arc::clone(&self.budget),
                capacity: Arc::clone(&self.capacity),
                bytes,
                active: true,
            },
        })
    }

    /// All ordinary messages also use the actual reservation-backed store.
    /// Legacy dequeue releases the queue charge; it does not track the Event
    /// after returning it to a legacy caller. An individually impossible cost
    /// is permanent TooLarge/CostOverflow, never a capacity wait disguised as Full.
    pub fn try_publish(&self, event: Event) -> Result<(), PublishError> {
        self.try_publish_deferred(event)?.notify();
        Ok(())
    }

    /// Uses the same actual enqueue as ordinary publication, but does not
    /// invoke the reader until the success receipt is explicitly notified.
    pub fn try_publish_deferred(
        &self,
        event: Event,
    ) -> Result<DeferredCompletionNotification, PublishError> {
        let bytes = match retained_event_bytes(&event) {
            Ok(bytes) => bytes,
            Err(_) => return Err(PublishError::CostOverflow(event)),
        };
        let Some(claim_bytes) = bytes.checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES) else {
            return Err(PublishError::CostOverflow(event));
        };
        let notification = {
            let Ok(mut storage) = self.receiver.lock() else {
                return Err(PublishError::Closed(event));
            };
            if storage.closed {
                return Err(PublishError::Closed(event));
            }
            {
                let Ok(budget) = self.budget.lock() else {
                    return Err(PublishError::Closed(event));
                };
                if budget.closed {
                    return Err(PublishError::Closed(event));
                }
                if let Some(limit) = budget.byte_limit.filter(|&limit| claim_bytes > limit) {
                    return Err(PublishError::TooLarge {
                        event,
                        required: claim_bytes,
                        limit,
                    });
                }
            }
            // An impossible Event stays a permanent rejection even if another
            // Event currently occupies every delivery slot.
            if storage.queue.len() >= storage.queue_capacity {
                return Err(PublishError::Full(event));
            }
            // Storage -> Budget is also the snapshot/close lock order. Reserve
            // only after delivery admission, while that slot cannot be taken.
            // A temporary claim dropped on Full would wake this same rejected
            // publisher, turning a retry into a self-wake loop.
            let reservation = match self.try_reserve(1, bytes) {
                Ok(reservation) => reservation,
                Err(ReserveError::Closed) => return Err(PublishError::Closed(event)),
                Err(ReserveError::Full) => return Err(PublishError::Full(event)),
                Err(ReserveError::TooLarge { required, limit }) => {
                    return Err(PublishError::TooLarge {
                        event,
                        required,
                        limit,
                    });
                }
                Err(ReserveError::CostOverflow | ReserveError::InvalidCount) => {
                    return Err(PublishError::CostOverflow(event));
                }
            };
            self.enqueue_locked(&mut storage, event, reservation, false)
        };
        Ok(notification)
    }

    /// The reservation retains storage while delivery queue slots are busy.
    /// Full, closure, wrong mailbox or a too-large Event return both the exact
    /// original Event and its original permission for retry or explicit abort.
    pub fn publish_reserved(
        &self,
        event: Event,
        reservation: CompletionReservation,
    ) -> Result<(), ReservedPublishError> {
        self.publish_reserved_deferred(event, reservation)?.notify();
        Ok(())
    }

    /// Accept into the real bounded queue without invoking caller code. The
    /// returned receipt must be notified outside all caller-held locks.
    pub fn publish_reserved_deferred(
        &self,
        event: Event,
        reservation: CompletionReservation,
    ) -> Result<DeferredCompletionNotification, ReservedPublishError> {
        let reason = if !Arc::ptr_eq(&self.budget, &reservation.claim.budget) {
            Some(ReservedPublishReason::WrongMailbox)
        } else {
            match retained_event_bytes(&event)
                .ok()
                .and_then(|n| n.checked_add(COMPLETION_ENTRY_OVERHEAD_BYTES))
            {
                None => Some(ReservedPublishReason::CostOverflow),
                Some(required) if required > reservation.claim.bytes => {
                    Some(ReservedPublishReason::TooSmall {
                        required,
                        reserved: reservation.claim.bytes,
                    })
                }
                Some(_) => None,
            }
        };
        if let Some(reason) = reason {
            return Err(ReservedPublishError {
                event,
                reservation,
                reason,
            });
        }
        let notification = {
            let Ok(mut storage) = self.receiver.lock() else {
                return Err(ReservedPublishError {
                    event,
                    reservation,
                    reason: ReservedPublishReason::Closed,
                });
            };
            if storage.closed {
                return Err(ReservedPublishError {
                    event,
                    reservation,
                    reason: ReservedPublishReason::Closed,
                });
            }
            if storage.queue.len() >= storage.queue_capacity {
                return Err(ReservedPublishError {
                    event,
                    reservation,
                    reason: ReservedPublishReason::Full,
                });
            }
            self.enqueue_locked(&mut storage, event, reservation, true)
        };
        Ok(notification)
    }

    // Both publication modes reach this one actual store after all rejection
    // checks. Delivery admission and push share Storage; no callback, Waker
    // clone/drop or recoverable fallible step follows before it is unlocked.
    fn enqueue_locked(
        &self,
        storage: &mut Storage,
        event: Event,
        reservation: CompletionReservation,
        reserved: bool,
    ) -> DeferredCompletionNotification {
        debug_assert!(storage.queue.len() < storage.queue_capacity);
        debug_assert!(storage.queue.len() < storage.queue.capacity());
        storage.queue.push_back(Entry {
            event,
            claim: reservation.claim,
            reserved,
        });
        DeferredCompletionNotification {
            reader: Arc::downgrade(&self.waker),
            source_claim: None,
        }
    }
}

pub struct CompletionMailbox {
    receiver: Arc<Mutex<Storage>>,
    budget: Arc<Mutex<Budget>>,
    waker: Arc<Mutex<Option<Waker>>>,
    capacity: Arc<Mutex<CapacityState>>,
}

impl CompletionMailbox {
    /// Legacy queue-only ownership. A reserved front is NOT removed, skipped
    /// or converted to a raw Event. Use the owned API for reserved messages.
    /// There is one logical polling reader; mixing modes is not a scheduler.
    pub fn try_take(&self) -> Poll {
        let entry = {
            let Ok(mut storage) = self.receiver.lock() else {
                return Poll::Closed;
            };
            if storage.queue.front().is_some_and(|entry| entry.reserved) {
                return Poll::Empty;
            }
            match storage.queue.pop_front() {
                Some(entry) => entry,
                None if storage.closed || storage.publishers == 0 => return Poll::Closed,
                None => return Poll::Empty,
            }
        };
        let Entry { event, claim, .. } = entry;
        drop(claim); // Caller-waker code runs outside Storage/Budget locks.
        Poll::Event(event)
    }

    pub fn try_take_owned(&self) -> OwnedPoll {
        let (entry, queue_was_full, queue_capacity) = {
            let Ok(mut storage) = self.receiver.lock() else {
                return OwnedPoll::Closed;
            };
            let queue_was_full = storage.queue.len() == storage.queue_capacity;
            let queue_capacity = storage.queue_capacity;
            match storage.queue.pop_front() {
                Some(entry) => (entry, queue_was_full, queue_capacity),
                None if storage.closed || storage.publishers == 0 => return OwnedPoll::Closed,
                None => return OwnedPoll::Empty,
            }
        };
        // With separate limits a reserved publisher may only lack a queue
        // slot. Dequeue frees that slot, but keeps the Event's storage claim.
        // Equal-limit compatibility mailboxes cannot have such a waiter when
        // the queue is full: all retained claims are already queued. Their
        // capacity notification remains tied to claim retirement.
        let queue_waiter_can_progress = queue_was_full
            && self
                .budget
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .capacity
                > queue_capacity;
        if queue_waiter_can_progress {
            notify_capacity(&self.capacity, false);
        }
        // A nonblocking owned drainer may reveal an ordinary front to the one
        // registered legacy reader. This is separate from the capacity wake.
        wake_reader(&self.waker);
        OwnedPoll::Event(RetainedCompletion {
            event: Some(entry.event),
            claim: Some(entry.claim),
        })
    }

    pub fn storage_snapshot(&self) -> CompletionStorageSnapshot {
        let storage = self.receiver.lock().unwrap_or_else(|e| e.into_inner());
        let budget = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        CompletionStorageSnapshot {
            capacity: budget.capacity,
            queue_capacity: storage.queue_capacity,
            byte_limit: budget.byte_limit,
            retained_count: budget.used_count,
            retained_bytes: budget.used_bytes,
            queued_count: storage.queue.len(),
            queue_backing_bytes: storage.backing_bytes,
            closed: storage.closed,
        }
    }

    pub fn poll_take(&self, context: &mut Context<'_>) -> TaskPoll<Poll> {
        self.poll_take_before_register(context, || {})
    }

    fn register_reader(&self, context: &Context<'_>) -> Result<(), ()> {
        let next = context.waker().clone();
        let previous = match self.waker.lock() {
            Ok(mut waker) => waker.replace(next),
            Err(_) => return Err(()),
        };
        drop(previous);
        Ok(())
    }

    fn poll_take_before_register(
        &self,
        context: &mut Context<'_>,
        before_register: impl FnOnce(),
    ) -> TaskPoll<Poll> {
        match self.try_take() {
            Poll::Empty => {}
            value => {
                clear_reader(&self.waker);
                return TaskPoll::Ready(value);
            }
        }
        before_register();
        if self.register_reader(context).is_err() {
            return TaskPoll::Ready(Poll::Closed);
        }
        match self.try_take() {
            Poll::Empty => TaskPoll::Pending,
            value => {
                clear_reader(&self.waker);
                TaskPoll::Ready(value)
            }
        }
    }

    pub fn poll_take_owned(&self, context: &mut Context<'_>) -> TaskPoll<OwnedPoll> {
        self.poll_take_owned_before_register(context, || {})
    }

    fn poll_take_owned_before_register(
        &self,
        context: &mut Context<'_>,
        before_register: impl FnOnce(),
    ) -> TaskPoll<OwnedPoll> {
        match self.try_take_owned() {
            OwnedPoll::Empty => {}
            value => {
                clear_reader(&self.waker);
                return TaskPoll::Ready(value);
            }
        }
        before_register();
        if self.register_reader(context).is_err() {
            return TaskPoll::Ready(OwnedPoll::Closed);
        }
        match self.try_take_owned() {
            OwnedPoll::Empty => TaskPoll::Pending,
            value => {
                clear_reader(&self.waker);
                TaskPoll::Ready(value)
            }
        }
    }
}

/// Count-only compatibility constructor. It does not declare a retained-byte
/// bound. Owned dequeue still retains the count claim until retire/transfer.
pub fn completion_mailbox(capacity: usize) -> (CompletionPublisher, Arc<CompletionMailbox>) {
    build_mailbox(capacity, capacity, None)
        .expect("completion mailbox capacity/allocation must be valid")
}

/// Event-footprint plus entry-overhead claim budget, not allocator/RSS/native
/// memory. The fixed preallocated queue backing is reported separately.
pub fn completion_mailbox_with_budget(
    capacity: usize,
    retained_bytes: usize,
) -> Result<(CompletionPublisher, Arc<CompletionMailbox>), MailboxBuildError> {
    build_mailbox(capacity, capacity, Some(retained_bytes))
}

/// Separate bounded delivery slots from retained storage claims. This permits
/// several pre-reserved results to pass through a smaller queue one at a time;
/// Full leaves both Event and reservation with the publisher. The retained
/// byte bound covers Event footprints and entry overhead, not allocator/RSS/
/// native memory. Fixed queue backing is reported separately.
pub fn completion_mailbox_with_limits(
    queue_capacity: usize,
    retained_capacity: usize,
    retained_bytes: usize,
) -> Result<(CompletionPublisher, Arc<CompletionMailbox>), MailboxBuildError> {
    build_mailbox(queue_capacity, retained_capacity, Some(retained_bytes))
}

fn build_mailbox(
    queue_capacity: usize,
    retained_capacity: usize,
    byte_limit: Option<usize>,
) -> Result<(CompletionPublisher, Arc<CompletionMailbox>), MailboxBuildError> {
    if queue_capacity == 0 || retained_capacity == 0 {
        return Err(MailboxBuildError::InvalidCapacity);
    }
    queue_capacity
        .checked_mul(std::mem::size_of::<Entry>())
        .ok_or(MailboxBuildError::StorageOverflow)?;
    let mut queue = VecDeque::new();
    queue
        .try_reserve_exact(queue_capacity)
        .map_err(|_| MailboxBuildError::AllocationFailed)?;
    let backing_bytes = queue
        .capacity()
        .checked_mul(std::mem::size_of::<Entry>())
        .ok_or(MailboxBuildError::StorageOverflow)?;
    let receiver = Arc::new(Mutex::new(Storage {
        queue,
        queue_capacity,
        closed: false,
        publishers: 1,
        backing_bytes,
    }));
    let budget = Arc::new(Mutex::new(Budget {
        capacity: retained_capacity,
        byte_limit,
        used_count: 0,
        used_bytes: 0,
        closed: false,
    }));
    let waker = Arc::new(Mutex::new(None));
    let capacity = Arc::new(Mutex::new(CapacityState::default()));
    Ok((
        CompletionPublisher {
            receiver: Arc::clone(&receiver),
            budget: Arc::clone(&budget),
            waker: Arc::clone(&waker),
            capacity: Arc::clone(&capacity),
        },
        Arc::new(CompletionMailbox {
            receiver,
            budget,
            waker,
            capacity,
        }),
    ))
}

impl Drop for CompletionPublisher {
    fn drop(&mut self) {
        {
            let mut storage = self.receiver.lock().unwrap_or_else(|e| e.into_inner());
            debug_assert!(storage.publishers > 0);
            storage.publishers -= 1;
        }
        // Actual disconnect precedes callback; buffered entries remain readable.
        wake_reader(&self.waker);
    }
}

impl Drop for CompletionMailbox {
    fn drop(&mut self) {
        let entries = {
            let mut storage = self.receiver.lock().unwrap_or_else(|e| e.into_inner());
            storage.closed = true;
            self.budget.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
            std::mem::take(&mut storage.queue)
        };
        clear_reader(&self.waker);
        notify_capacity(&self.capacity, true);
        drop(entries); // Claim and Event destruction never run under Storage.
    }
}

#[cfg(test)]
#[path = "mailbox_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mailbox_reservation_tests.rs"]
mod reservation_tests;

#[cfg(test)]
#[path = "mailbox_queue_storage_tests.rs"]
mod queue_storage_tests;

#[cfg(test)]
#[path = "mailbox_deferred_tests.rs"]
mod deferred_tests;
