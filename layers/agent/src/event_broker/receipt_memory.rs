//! Exact receipt allocations, separately from the delivered Event and indexes.
//! A front ticket may keep an evicted receipt alive. These are observations,
//! not admission credit, queue ownership, or process RSS.
use p4_adapter::node_adapter::retained_event_bytes;
use p4_protocol::event::Event;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptStorageSnapshot {
    pub events: usize,
    pub event_bytes: Option<usize>,
    pub payload_capacity_bytes: Option<usize>,
    pub unmeasured_events: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptMemorySnapshot {
    pub duplicate_window: usize,
    pub indexed: ReceiptStorageSnapshot,
    /// Removed from the index but pinned by a synchronous front ticket.
    pub retired: ReceiptStorageSnapshot,
    pub allocated: ReceiptStorageSnapshot,
    pub peak_allocated_event_bytes: Option<usize>,
    pub committed_events: Option<usize>,
    pub evicted_events: Option<usize>,
    pub freed_events: Option<usize>,
    pub event_index_capacity: usize,
    pub order_capacity: usize,
    pub sequence_entries: usize,
    pub sequence_capacity: usize,
}

#[derive(Default)]
struct Storage {
    events: usize,
    known_event_bytes: u128,
    payload_capacity_bytes: u128,
    unmeasured_events: usize,
}

impl Storage {
    fn add(&mut self, bytes: Option<usize>, payload: usize) {
        self.events += 1;
        self.known_event_bytes += bytes.unwrap_or(0) as u128;
        self.payload_capacity_bytes += payload as u128;
        self.unmeasured_events += usize::from(bytes.is_none());
    }
    fn remove(&mut self, bytes: Option<usize>, payload: usize) {
        self.events -= 1;
        self.known_event_bytes -= bytes.unwrap_or(0) as u128;
        self.payload_capacity_bytes -= payload as u128;
        self.unmeasured_events -= usize::from(bytes.is_none());
    }
    fn snapshot(&self) -> ReceiptStorageSnapshot {
        ReceiptStorageSnapshot {
            events: self.events,
            event_bytes: (self.unmeasured_events == 0)
                .then(|| usize::try_from(self.known_event_bytes).ok())
                .flatten(),
            payload_capacity_bytes: usize::try_from(self.payload_capacity_bytes).ok(),
            unmeasured_events: self.unmeasured_events,
        }
    }
}

#[derive(Default)]
pub(super) struct ReceiptMemory {
    indexed: Storage,
    retired: Storage,
    allocated: Storage,
    peak_bytes: u128,
    peak_unknown: bool,
    committed: u128,
    evicted: u128,
    freed: u128,
}

impl ReceiptMemory {
    pub(super) fn snapshot(
        &self,
        window: usize,
        event_capacity: usize,
        order_capacity: usize,
        sequence_entries: usize,
        sequence_capacity: usize,
    ) -> ReceiptMemorySnapshot {
        ReceiptMemorySnapshot {
            duplicate_window: window,
            indexed: self.indexed.snapshot(),
            retired: self.retired.snapshot(),
            allocated: self.allocated.snapshot(),
            peak_allocated_event_bytes: (!self.peak_unknown)
                .then(|| usize::try_from(self.peak_bytes).ok())
                .flatten(),
            committed_events: usize::try_from(self.committed).ok(),
            evicted_events: usize::try_from(self.evicted).ok(),
            freed_events: usize::try_from(self.freed).ok(),
            event_index_capacity: event_capacity,
            order_capacity,
            sequence_entries,
            sequence_capacity,
        }
    }
}

pub(super) struct Receipt {
    event: Option<Event>,
    bytes: Option<usize>,
    payload_capacity: usize,
    retired: AtomicBool,
    memory: Arc<Mutex<ReceiptMemory>>,
}

impl Receipt {
    pub(super) fn new(event: Event, memory: Arc<Mutex<ReceiptMemory>>) -> Self {
        // Measure this exact independent clone, not the delivered original's
        // spare allocation. No serialization or additional payload copy.
        let bytes = retained_event_bytes(&event).ok();
        let payload_capacity = event.payload.capacity();
        {
            let mut state = memory.lock().unwrap_or_else(|error| error.into_inner());
            state.indexed.add(bytes, payload_capacity);
            state.allocated.add(bytes, payload_capacity);
            state.committed = state.committed.saturating_add(1);
            state.peak_bytes = state.peak_bytes.max(state.allocated.known_event_bytes);
            state.peak_unknown |= bytes.is_none();
        }
        Self {
            event: Some(event),
            bytes,
            payload_capacity,
            retired: AtomicBool::new(false),
            memory,
        }
    }

    pub(super) fn event(&self) -> &Event {
        self.event.as_ref().expect("live receipt owns its event")
    }

    pub(super) fn retire(&self) {
        let mut state = self
            .memory
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.retired.swap(true, Ordering::Relaxed) {
            state.indexed.remove(self.bytes, self.payload_capacity);
            state.retired.add(self.bytes, self.payload_capacity);
            state.evicted = state.evicted.saturating_add(1);
        }
    }
}

impl Drop for Receipt {
    fn drop(&mut self) {
        // Only the last Arc destroys the exact Event. Drop its buffers before
        // reporting freed storage, including when the broker itself is gone.
        drop(self.event.take());
        let mut state = self
            .memory
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.retired.load(Ordering::Relaxed) {
            state.retired.remove(self.bytes, self.payload_capacity);
        } else {
            state.indexed.remove(self.bytes, self.payload_capacity);
        }
        state.allocated.remove(self.bytes, self.payload_capacity);
        state.freed = state.freed.saturating_add(1);
    }
}

#[cfg(test)]
impl std::fmt::Debug for Receipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.event().fmt(formatter)
    }
}
