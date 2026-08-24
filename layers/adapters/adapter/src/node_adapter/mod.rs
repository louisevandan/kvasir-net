//! Backend-neutral P4 v2 node adapter boundary.
//!
//! Events remain opaque to P4. A concrete implementation interprets only the
//! content types registered for its adapter kind and publishes new events to
//! its own bounded completion mailbox.

mod mailbox;

pub use mailbox::{CompletionMailbox, CompletionPublisher, completion_mailbox};
use p4_protocol::event::Event;
use std::task::{Context, Poll as TaskPoll};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfferError {
    Full,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Poll {
    Event(Event),
    Empty,
    Closed,
}

/// Stateful concrete runtime owned by one abstract P4 node.
///
/// `try_offer` and `try_take` are the only event movement operations. They are
/// non-blocking by contract. Load, inference and unload completion are output
/// events, never return values from these calls.
pub trait NodeAdapter: Send + Sync {
    fn kind(&self) -> &str;

    fn try_offer(&self, event: Event) -> Result<(), OfferError>;

    fn try_take(&self) -> Poll;

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
