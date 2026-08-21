//! Where an adapter raises its events.
//!
//! One trait, and it should stay one. This is the property that keeps a hop's
//! duration out of any worker's time, so it changes only if that property
//! changes.

use crate::event::report::Event;

/// Implemented by the node's queue. It never answers, so an adapter cannot
/// come to depend on a raised event carrying a reply back.
///
/// It is not guaranteed non-blocking: the node's implementation sends into a
/// bounded channel and can block when that channel is full. Every adapter
/// today calls it from a thread that is already allowed to block (the one
/// running the backend's own blocking work), which is what makes this
/// survivable — an adapter must not assume it can raise an event from a
/// context that must not block. A dedicated event dispatcher is planned to
/// take over this path and make the non-blocking guarantee real; until then
/// this is what actually happens, not what was originally documented here.
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
