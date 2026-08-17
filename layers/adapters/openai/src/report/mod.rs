//! What this backend says it is doing.
//!
//! Its own file because it answers a different question than the rest: not how
//! a completion is driven, but what an operator or a caller elsewhere can find
//! out without reading the server's log or the machine's socket table. Those
//! two were the only way to diagnose this layer under load, and neither is
//! available from anywhere else.

use super::OpenAi;
use std::sync::atomic::{AtomicU64, Ordering};

impl OpenAi {
    /// What this backend is doing, in a line.
    ///
    /// Chosen for what could not be seen while diagnosing this layer under
    /// load: how many streams are open right now, how many had to be reached
    /// more than once, and how many could not be reached at all. Every one of
    /// those came from the machine's socket table or the server's own log,
    /// which is exactly what a caller elsewhere cannot read.
    ///
    /// Counters rather than a history, because a status request must not cost
    /// more than the thing it is asking about.
    pub(super) fn state(&self) -> String {
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        format!(
            "{} open={} opened={} reopened={} refused={} finished={}",
            self.flavour.name(),
            self.sessions.lock().expect("sessions lock").len(),
            load(&self.opened),
            load(&self.reopened),
            load(&self.refused),
            load(&self.finished),
        )
    }
}
