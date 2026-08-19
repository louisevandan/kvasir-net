//! Where an adapter raises its events.
//!
//! One trait, and it should stay one. This is the property that keeps a hop's
//! duration out of any worker's time, so it changes only if that property
//! changes.

use crate::event::report::Event;

/// Implemented by the node's queue. It never blocks and never answers, so an
/// adapter cannot come to depend on being heard synchronously.
pub trait EventSink: Send + Sync {
    fn raise(&self, event: Event);

    /// Whether the node's deadline fence has cancelled the current
    /// operation. Adapters may poll this between backend calls; the default
    /// keeps existing adapters source-compatible and non-cancellable.
    fn cancelled(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
