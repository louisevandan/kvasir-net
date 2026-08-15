//! Where a reply is expected, held apart from the call that asked for it.
//!
//! CPS rule 5: a requester does not wait, it registers a handler to be called
//! with the response. That handler cannot live on a call stack, because the
//! call returns immediately and long work is split across tasks — so it lives
//! here, keyed by the route the reply will carry.
//!
//! This is what `ResponseSink` could not be. A sink borrowed from a stack
//! frame disappears when that frame does.

use std::collections::HashMap;
use std::sync::Mutex;

/// Called once, with whatever came back.
pub type Handler<T> = Box<dyn FnOnce(T) + Send + 'static>;

pub struct Continuations<T> {
    waiting: Mutex<HashMap<String, Handler<T>>>,
}

impl<T> Default for Continuations<T> {
    fn default() -> Self {
        Self {
            waiting: Mutex::new(HashMap::new()),
        }
    }
}

impl<T> Continuations<T> {
    /// Registers before sending, never after. Registering after would leave a
    /// window in which a fast reply arrives with nowhere to go.
    pub fn register(&self, route: impl Into<String>, handler: Handler<T>) {
        self.waiting
            .lock()
            .expect("continuation registry lock")
            .insert(route.into(), handler);
    }

    /// Takes the handler for a route and calls it. Returns whether anyone was
    /// waiting — a caller uses that to tell a stray reply from a handled one.
    pub fn resolve(&self, route: &str, value: T) -> bool {
        let handler = self
            .waiting
            .lock()
            .expect("continuation registry lock")
            .remove(route);
        match handler {
            Some(handler) => {
                handler(value);
                true
            }
            None => false,
        }
    }

    /// Drops a route's handler without calling it, for a request abandoned
    /// before any reply. The handler is discarded rather than invoked with a
    /// synthetic value, because a continuation that runs on cancellation would
    /// make cancelled and completed indistinguishable.
    pub fn forget(&self, route: &str) -> bool {
        self.waiting
            .lock()
            .expect("continuation registry lock")
            .remove(route)
            .is_some()
    }

    /// How many replies are still expected. A number that only grows is a
    /// route leak, which is one of the things a fleet run watches for.
    pub fn outstanding(&self) -> usize {
        self.waiting
            .lock()
            .expect("continuation registry lock")
            .len()
    }
}

#[cfg(test)]
mod tests;
