use super::DispatchError;
use super::receipt_memory::{Receipt, ReceiptMemory, ReceiptMemorySnapshot};
use p4_protocol::event::{Endpoint, Envelope, Event};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

pub(super) enum LedgerVerdict {
    New,
    Duplicate,
}

pub(super) struct EventLedger {
    limit: usize,
    order: VecDeque<String>,
    events: HashMap<String, Arc<Receipt>>,
    sequences: HashMap<(Endpoint, String), u64>,
    memory: Arc<Mutex<ReceiptMemory>>,
}

impl EventLedger {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            order: VecDeque::with_capacity(limit),
            events: HashMap::with_capacity(limit),
            sequences: HashMap::new(),
            memory: Arc::new(Mutex::new(ReceiptMemory::default())),
        }
    }

    pub(super) fn inspect(&self, event: &Event) -> Result<LedgerVerdict, DispatchError> {
        if let Some(existing) = self.inspect_completion_header(&event.envelope)? {
            return if existing.event() == event {
                Ok(LedgerVerdict::Duplicate)
            } else {
                Err(DispatchError::ConflictingDuplicate)
            };
        }
        Ok(LedgerVerdict::New)
    }

    /// Pin the exact already-delivered receipt for a synchronous front probe.
    /// No producer storage permission is shared with this independent copy.
    pub(super) fn inspect_completion_header(
        &self,
        envelope: &Envelope,
    ) -> Result<Option<Arc<Receipt>>, DispatchError> {
        if let Some(existing) = self.events.get(&envelope.event_id) {
            return Ok(Some(Arc::clone(existing)));
        }
        let key = (envelope.source.clone(), envelope.correlation_id.clone());
        if let Some(previous) = self.sequences.get(&key)
            && envelope.sequence <= *previous
        {
            return Err(DispatchError::SequenceRegression {
                previous: *previous,
                incoming: envelope.sequence,
            });
        }
        Ok(None)
    }

    pub(super) fn commit(&mut self, event: Event) {
        let sequence_key = (
            event.envelope.source.clone(),
            event.envelope.correlation_id.clone(),
        );
        self.sequences.insert(sequence_key, event.envelope.sequence);
        self.order.push_back(event.envelope.event_id.clone());
        self.events.insert(
            event.envelope.event_id.clone(),
            Arc::new(Receipt::new(event, Arc::clone(&self.memory))),
        );
        while self.order.len() > self.limit {
            if let Some(expired) = self.order.pop_front() {
                if let Some(event) = self.events.remove(&expired) {
                    event.retire();
                    let key = (
                        event.event().envelope.source.clone(),
                        event.event().envelope.correlation_id.clone(),
                    );
                    if self.sequences.get(&key) == Some(&event.event().envelope.sequence) {
                        self.sequences.remove(&key);
                    }
                }
            }
        }
    }

    pub(super) fn receipt_snapshot(&self) -> ReceiptMemorySnapshot {
        self.memory
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot(
                self.limit,
                self.events.capacity(),
                self.order.capacity(),
                self.sequences.len(),
                self.sequences.capacity(),
            )
    }
}

#[cfg(test)]
impl std::fmt::Debug for EventLedger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Semantic-ledger refusal oracles intentionally exclude diagnostics.
        formatter
            .debug_struct("EventLedger")
            .field("limit", &self.limit)
            .field("order", &self.order)
            .field("events", &self.events)
            .field("sequences", &self.sequences)
            .finish()
    }
}
