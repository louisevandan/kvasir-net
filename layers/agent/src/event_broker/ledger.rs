use super::DispatchError;
use p4_protocol::event::{Endpoint, Event};
use std::collections::{HashMap, VecDeque};

pub(super) enum LedgerVerdict {
    New,
    Duplicate,
}

pub(super) struct EventLedger {
    limit: usize,
    order: VecDeque<String>,
    events: HashMap<String, Event>,
    sequences: HashMap<(Endpoint, String), u64>,
}

impl EventLedger {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            order: VecDeque::with_capacity(limit),
            events: HashMap::with_capacity(limit),
            sequences: HashMap::new(),
        }
    }

    pub(super) fn inspect(&self, event: &Event) -> Result<LedgerVerdict, DispatchError> {
        if let Some(existing) = self.events.get(&event.envelope.event_id) {
            return if existing == event {
                Ok(LedgerVerdict::Duplicate)
            } else {
                Err(DispatchError::ConflictingDuplicate)
            };
        }
        let key = (
            event.envelope.source.clone(),
            event.envelope.correlation_id.clone(),
        );
        if let Some(previous) = self.sequences.get(&key)
            && event.envelope.sequence <= *previous
        {
            return Err(DispatchError::SequenceRegression {
                previous: *previous,
                incoming: event.envelope.sequence,
            });
        }
        Ok(LedgerVerdict::New)
    }

    pub(super) fn commit(&mut self, event: Event) {
        let sequence_key = (
            event.envelope.source.clone(),
            event.envelope.correlation_id.clone(),
        );
        self.sequences.insert(sequence_key, event.envelope.sequence);
        self.order.push_back(event.envelope.event_id.clone());
        self.events.insert(event.envelope.event_id.clone(), event);
        while self.order.len() > self.limit {
            if let Some(expired) = self.order.pop_front() {
                if let Some(event) = self.events.remove(&expired) {
                    let key = (event.envelope.source, event.envelope.correlation_id);
                    if self.sequences.get(&key) == Some(&event.envelope.sequence) {
                        self.sequences.remove(&key);
                    }
                }
            }
        }
    }
}
