//! Backend-neutral P4 v2 node adapter boundary.
//!
//! Events remain opaque to P4. A concrete implementation interprets only the
//! content types registered for its adapter kind and publishes new events to
//! its own bounded completion mailbox.

mod event_cost;
mod mailbox;

pub use event_cost::{ResourceCostError, retained_event_bytes};

pub use mailbox::{
    COMPLETION_ENTRY_OVERHEAD_BYTES, CapacityListenError, CapacityRegistration, CompletionFront,
    CompletionMailbox, CompletionPublisher, CompletionQueueReservation, CompletionReservation,
    CompletionReservationGroup, CompletionStorageSnapshot, DeferredCompletionNotification,
    GroupReserveError, MAX_CAPACITY_LISTENERS, MailboxBuildError, OwnedPoll, PublishError,
    QueuePublishError, ReserveError, ReservedPublishError, ReservedPublishReason,
    RetainedCompletion, RetainedQueueTransferError, RetainedTransferError, completion_mailbox,
    completion_mailbox_with_budget, completion_mailbox_with_limits,
};
use p4_protocol::event::lifecycle::{LifecycleOperation, LifecycleStatus, ResourceState};
use p4_protocol::event::{Envelope, Event};
use std::task::{Context, Poll as TaskPoll};

// Every refusal returns ownership. Full may be retried; Closed requires an
// explicit failure disposition rather than silently consuming the Event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OfferError {
    /// The adapter has no room right now, and hands the event back.
    ///
    /// It carries the event because the caller's only correct response is to
    /// keep it and try again: dropping it loses work, and failing the node
    /// turns a busy adapter into a dead pipeline.
    Full(Event),
    /// The adapter can no longer accept this Event; the original is returned.
    Closed(Event),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Poll {
    Event(Event),
    Empty,
    Closed,
}

/// Stateful concrete runtime owned by one abstract P4 node.
///
/// Event movement operations are non-blocking by contract. Load, inference
/// and unload completion are output events, never return values from these
/// calls. Completion inspection never removes an event.
pub trait NodeAdapter: Send + Sync {
    fn kind(&self) -> &str;

    fn try_offer(&self, event: Event) -> Result<(), OfferError>;

    fn try_take(&self) -> Poll;

    /// Inspect only the next ordinary completion's envelope without consuming
    /// it. None also permits adapters that do not implement conditional drain;
    /// callers must not interpret it as a closed mailbox or look past its front.
    fn peek_completion(&self) -> Option<Envelope> {
        None
    }

    /// Remove the ordinary front only if its entire envelope still matches.
    /// This never scans ahead or consumes a reserved completion. The default
    /// leaves ownership untouched; it must not fall back to unconditional take.
    fn try_take_completion_matching(&self, _expected: &Envelope) -> Poll {
        Poll::Empty
    }

    /// Registers the node task for an adapter-completion wakeup. Concrete
    /// adapters with an asynchronous worker override this; the default keeps
    /// simple in-thread adapters valid.
    fn poll_take(&self, _context: &mut Context<'_>) -> TaskPoll<Poll> {
        match self.try_take() {
            Poll::Empty => TaskPoll::Pending,
            value => TaskPoll::Ready(value),
        }
    }

    /// Opaque, cheap and non-blocking status for monitoring.
    fn snapshot(&self) -> String {
        String::new()
    }
}

/// Backend-neutral owned storage that remains inside an adapter rather than
/// its completion mailbox. Counts and bytes name allocations, not wire work,
/// GPU memory, KV rows or completion delivery authority.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct AdapterRetainedStorage {
    pub count: usize,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct AdapterRetentionSnapshot {
    pub pending_requests: AdapterRetainedStorage,
    pub native_responses: AdapterRetainedStorage,
}

/// Backend-owned interpretation of one lifecycle terminal completion.
///
/// The Event and its retained claim stay with the caller. This value grants no
/// permission to remove a route or retire the completion: the agent supervisor
/// must still match source, causation, operation and node generation and prove
/// delivery/native cleanup. `ResourceState::Unknown` is never success.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct AdapterLifecycleCompletion {
    pub operation: LifecycleOperation,
    pub status: LifecycleStatus,
    pub resource_state: ResourceState,
    pub first_error: Option<String>,
    pub cleanup_error: Option<String>,
}

impl AdapterLifecycleCompletion {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.status != LifecycleStatus::Succeeded {
            return if self.first_error.as_deref().is_some_and(|v| !v.is_empty()) {
                Ok(())
            } else {
                Err("rejected or failed lifecycle completion requires first_error")
            };
        }
        if self.first_error.is_some() || self.cleanup_error.is_some() {
            return Err("successful lifecycle completion cannot contain an error");
        }
        match (self.operation, self.resource_state) {
            (LifecycleOperation::Load, ResourceState::Present)
            | (LifecycleOperation::Unload, ResourceState::Absent) => Ok(()),
            (LifecycleOperation::Load, _) => {
                Err("successful LOAD completion must prove present resources")
            }
            (LifecycleOperation::Unload, _) => {
                Err("successful UNLOAD completion must prove absent resources")
            }
        }
    }

    pub fn succeeded(&self) -> bool {
        self.status == LifecycleStatus::Succeeded && self.validate().is_ok()
    }
}

/// Explicit owned transport contract. There is deliberately no raw-Event
/// fallback: concrete producers and every consumer must migrate together.
/// A successful offer transfers responsibility for the original allocation
/// and its claim; it does not prove native execution or downstream acceptance.
pub trait RetainedNodeAdapter: Send + Sync {
    fn try_offer_retained(&self, completion: RetainedCompletion) -> Result<(), RetainedOfferError>;
    fn peek_retained_completion(&self) -> Option<CompletionFront>;
    fn try_take_retained_matching(&self, expected: &CompletionFront) -> OwnedPoll;
    fn poll_take_retained(&self, context: &mut Context<'_>) -> TaskPoll<OwnedPoll>;
    fn snapshot(&self) -> String;
    /// Unknown is not empty. Lifecycle deletion must reject without an
    /// authoritative observation of queued and held completion ownership.
    fn completion_storage_snapshot(&self) -> Option<CompletionStorageSnapshot> {
        None
    }
    /// Unknown is distinct from an adapter with no pending request or native
    /// response buffers. Implementations report their own allocation units.
    fn retention_snapshot(&self) -> Option<AdapterRetentionSnapshot> {
        None
    }

    /// Interpret a lifecycle terminal without consuming the ordinary
    /// completion front. EventNode remains the only owner that dequeues that
    /// front and transfers it through the broker into the bounded agent input.
    /// A concrete adapter must opt in; the default never infers completion from
    /// a content type, payload text or `snapshot()`.
    fn decode_lifecycle_completion(
        &self,
        _operation: LifecycleOperation,
        _event: &Event,
    ) -> Result<AdapterLifecycleCompletion, String> {
        Err("adapter does not provide typed lifecycle completion".into())
    }
}

#[derive(Debug)]
pub enum RetainedOfferError {
    Full(RetainedCompletion),
    Closed(RetainedCompletion),
}

#[cfg(test)]
mod tests;
