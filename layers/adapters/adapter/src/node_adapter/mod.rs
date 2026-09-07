//! Backend-neutral P4 v2 node adapter boundary.
//!
//! Events remain opaque to P4. A concrete implementation interprets only the
//! content types registered for its adapter kind and publishes new events to
//! its own bounded completion mailbox.

mod event_cost;
mod mailbox;

pub use event_cost::{ResourceCostError, retained_event_bytes};

pub use mailbox::{
    COMPLETION_ENTRY_OVERHEAD_BYTES, CapacityListenError, CapacityRegistration, CompletionMailbox,
    CompletionPublisher, CompletionReservation, CompletionReservationGroup,
    CompletionStorageSnapshot, DeferredCompletionNotification, GroupReserveError,
    MAX_CAPACITY_LISTENERS, MailboxBuildError, OwnedPoll, PublishError, ReserveError,
    ReservedPublishError, ReservedPublishReason, RetainedCompletion, RetainedTransferError,
    completion_mailbox, completion_mailbox_with_budget, completion_mailbox_with_limits,
};
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

#[cfg(test)]
mod tests;
