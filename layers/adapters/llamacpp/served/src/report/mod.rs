//! What this backend says it is doing.
//!
//! Its own file because it answers a different question than the rest: not how
//! a completion is driven, but what an operator or a caller elsewhere can find
//! out without reading the server's log or the machine's socket table. Those
//! two were the only way to diagnose this layer under load, and neither is
//! available from anywhere else.

use super::Served;
use std::sync::atomic::{AtomicU64, Ordering};

impl Served {
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
            "{} {} open={} opened={} reopened={} refused={} finished={}",
            self.flavour.name(),
            self.backend(),
            self.sessions.lock().expect("sessions lock").len(),
            load(&self.opened),
            load(&self.reopened),
            load(&self.refused),
            load(&self.finished),
        )
    }

    /// Whose process is answering, and what this node has done to it.
    ///
    /// The distinction a reader cannot otherwise make. Two nodes reporting the
    /// same counters mean different things if one of them started the server
    /// and the other found it: only the first is a node whose declared share is
    /// backed by a placement anybody chose, and only the first releases a card
    /// when it unloads. `started`/`stopped` are there because a restart is
    /// invisible in a snapshot otherwise — a node on its fourth backend looks
    /// exactly like one on its first.
    fn backend(&self) -> String {
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        let held = self.backend.lock().expect("backend lock");
        let counts = format!(
            "started={} stopped={}",
            load(&self.started),
            load(&self.stopped)
        );
        match held.as_ref() {
            // The pid joins what the protocol says to what the machine shows.
            // Without it a node claiming a card and a process holding one are
            // two facts with nothing connecting them, which is exactly the gap
            // that made a stray backend from a previous run look like a memory
            // error in the next one.
            Some(running) => format!("backend=owned pid={} {counts}", running.pid()),
            None if load(&self.started) > 0 => format!("backend=released {counts}"),
            None => "backend=attached".into(),
        }
    }
}
