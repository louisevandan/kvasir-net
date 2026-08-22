//! Shared test doubles used by more than one module's test file. Kept out of
//! `transport::fake` because that module is about the wire, not about what a
//! caller above the client does with admitted events.

#![cfg(test)]

use crate::contract::Event;
use p4_adapter::deployment::Sink;
use std::sync::{Arc, Mutex};

/// Records every event the client hands it, in arrival order, for a test to
/// assert against.
#[derive(Default)]
pub struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingSink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn events(&self) -> Vec<Event> {
        self.events.lock().expect("events lock").clone()
    }

    pub fn len(&self) -> usize {
        self.events.lock().expect("events lock").len()
    }

    #[allow(dead_code)] // exists to satisfy clippy::len_without_is_empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Sink for RecordingSink {
    fn raise(&self, event: Event) {
        self.events.lock().expect("events lock").push(event);
    }
}
