//! Non-blocking P4 v2 event routing.
//!
//! The broker reads only the target endpoint. A successful dispatch means the
//! event moved to one bounded queue; it never means the target completed work.

mod ledger;
mod receipt_memory;
mod retained;
pub use retained::{RetainedDispatchFailure, RetainedEventBroker};
pub use receipt_memory::{ReceiptMemorySnapshot, ReceiptStorageSnapshot};

use ledger::{EventLedger, LedgerVerdict};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc;

pub type EventSender = mpsc::Sender<Event>;
pub type EventReceiver = mpsc::Receiver<Event>;

pub fn bounded_queue(capacity: usize) -> (EventSender, EventReceiver) {
    assert!(capacity > 0, "event queue capacity must be positive");
    mpsc::channel(capacity)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    Enqueued(Delivery),
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    Agent,
    Node { node: String, generation: u64 },
    Outer,
    Outbound(Address),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchError {
    Invalid(String),
    ConflictingDuplicate,
    SequenceRegression {
        previous: u64,
        incoming: u64,
    },
    UnknownNode(String),
    StaleNode {
        node: String,
        current_generation: u64,
        incoming_generation: u64,
    },
    /// Temporary destination pressure. The enclosing DispatchFailure returns
    /// the original Event, as it does for every permanent dispatch refusal.
    Full(Delivery),
    Closed(Delivery),
    /// Individually impossible retained allocation; retrying after a capacity
    /// notification cannot make this input fit the destination's configured limit.
    StorageTooLarge { delivery: Delivery, required: usize, limit: usize },
    Poisoned,
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for DispatchError {}

/// A refused dispatch never consumes its input. The failure owns the exact
/// original allocation, not a reserialized/cloned replacement. It is not
/// Clone: callers explicitly retry, retain, or terminate its ownership.
///
/// This is a local failure-ownership boundary, not a retained-byte claim or
/// proof of remote delivery. Registration errors have no input Event and
/// continue to return DispatchError directly.
#[derive(Debug, PartialEq, Eq)]
pub struct DispatchFailure {
    pub error: DispatchError,
    pub event: Box<Event>,
}

impl DispatchFailure {
    fn new(error: DispatchError, event: Event) -> Self {
        Self {
            error,
            event: Box::new(event),
        }
    }
}

impl std::fmt::Display for DispatchFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Do not write a failed user's complete payload to operational logs.
        std::fmt::Display::fmt(&self.error, formatter)
    }
}

impl std::error::Error for DispatchFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

pub struct EventBroker<S = EventSender> {
    own: Address,
    agent: S,
    outer: S,
    outbound: S,
    nodes: RwLock<HashMap<String, NodeRoute<S>>>,
    node_generations: Mutex<HashMap<String, u64>>,
    ledger: Mutex<EventLedger>,
}

#[derive(Clone)]
struct NodeRoute<S> {
    generation: u64,
    sender: S,
    admission_paused: Arc<std::sync::atomic::AtomicBool>,
}

/// Temporary local lifecycle fence. Drop resumes the same registration.
/// This grants no execution, cancellation or replay authority.
pub struct NodeAdmissionPause(Arc<std::sync::atomic::AtomicBool>);
impl Drop for NodeAdmissionPause {
    fn drop(&mut self) { self.0.store(false, std::sync::atomic::Ordering::Release); }
}

/// A synchronous front-dispatch ticket. It is never kept across an await or
/// used as a future native-result/remote-receiver credit. Existing receipts
/// are pinned so exact duplicate inspection still precedes destination Full.
pub(crate) struct CompletionDispatch {
    envelope: Envelope,
    kind: CompletionDispatchKind,
}

enum CompletionDispatchKind {
    Existing(Arc<receipt_memory::Receipt>),
    Destination {
        delivery: Delivery,
        sender: EventSender,
        permit: mpsc::OwnedPermit<Event>,
    },
}

impl EventBroker {
    /// Reserve the actual destination before the adapter relinquishes an
    /// independent ordinary completion. No broker lock survives this call.
    pub(crate) fn reserve_completion(
        &self,
        envelope: &Envelope,
    ) -> Result<CompletionDispatch, DispatchError> {
        envelope.validate().map_err(|error| DispatchError::Invalid(error.to_string()))?;
        let existing = self.ledger.lock().map_err(|_| DispatchError::Poisoned)?
            .inspect_completion_header(envelope)?;
        let kind = if let Some(receipt) = existing {
            CompletionDispatchKind::Existing(receipt)
        } else {
            let (delivery, sender) = self.destination(&envelope.target)?;
            let permit = sender.clone().try_reserve_owned().map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => DispatchError::Full(delivery.clone()),
                mpsc::error::TrySendError::Closed(_) => DispatchError::Closed(delivery.clone()),
            })?;
            CompletionDispatchKind::Destination { delivery, sender, permit }
        };
        Ok(CompletionDispatch { envelope: envelope.clone(), kind })
    }

    pub(crate) fn dispatch_completion(
        &self,
        ticket: CompletionDispatch,
        event: Event,
    ) -> Result<DispatchOutcome, DispatchFailure> {
        if event.envelope != ticket.envelope {
            return Err(DispatchFailure::new(
                DispatchError::Invalid("completion front changed after reservation".into()), event));
        }
        if let Err(error) = event.validate() {
            return Err(DispatchFailure::new(DispatchError::Invalid(error.to_string()), event));
        }
        let (delivery, sender, permit) = match ticket.kind {
            CompletionDispatchKind::Existing(receipt) => {
                // The independent receipt was present when this synchronous
                // operation began. Pinning it prevents concurrent window eviction
                // from changing a Duplicate into a new delivery or requiring space.
                return if receipt.event() == &event {
                    Ok(DispatchOutcome::Duplicate)
                } else {
                    Err(DispatchFailure::new(DispatchError::ConflictingDuplicate, event))
                };
            }
            CompletionDispatchKind::Destination { delivery, sender, permit } => (delivery, sender, permit),
        };
        let mut ledger = match self.ledger.lock() {
            Ok(ledger) => ledger,
            Err(_) => return Err(DispatchFailure::new(DispatchError::Poisoned, event)),
        };
        match ledger.inspect(&event) {
            Ok(LedgerVerdict::Duplicate) => return Ok(DispatchOutcome::Duplicate),
            Ok(LedgerVerdict::New) => {}
            Err(error) => return Err(DispatchFailure::new(error, event)),
        }
        // A slot is not routing authority: registration may have changed
        // between the front probe and this commit, even without an await.
        let current = match self.destination(&event.envelope.target) {
            Ok((current_delivery, current_sender))
                if current_delivery == delivery && current_sender.same_channel(&sender) => current_sender,
            Ok(_) => return Err(DispatchFailure::new(
                DispatchError::Invalid("completion destination changed after reservation".into()), event)),
            Err(error) => return Err(DispatchFailure::new(error, event)),
        };
        drop(current);
        let receipt = event.clone();
        permit.send(event);
        ledger.commit(receipt);
        Ok(DispatchOutcome::Enqueued(delivery))
    }

    pub fn dispatch(&self, event: Event) -> Result<DispatchOutcome, DispatchFailure> {
        if let Err(error) = event.validate() {
            return Err(DispatchFailure::new(
                DispatchError::Invalid(error.to_string()),
                event,
            ));
        }
        let mut ledger = match self.ledger.lock() {
            Ok(ledger) => ledger,
            Err(_) => return Err(DispatchFailure::new(DispatchError::Poisoned, event)),
        };
        match ledger.inspect(&event) {
            Ok(LedgerVerdict::Duplicate) => return Ok(DispatchOutcome::Duplicate),
            Ok(LedgerVerdict::New) => {}
            Err(error) => return Err(DispatchFailure::new(error, event)),
        }

        let (delivery, sender) = match self.destination(&event.envelope.target) {
            Ok(destination) => destination,
            Err(error) => return Err(DispatchFailure::new(error, event)),
        };
        // Obtain this actual destination slot before making the successful
        // delivery's independent ledger copy. Full must return the original owned
        // value, including spare allocation capacity, not a freshly cloned
        // substitute. This short synchronous permit is not a future-result,
        // retained-byte or remote-receiver reservation.
        match sender.try_reserve() {
            Ok(permit) => {
                // Successful delivery moves the same allocation that Full
                // returns on refusal. The long-lived exact-deduplication
                // receipt owns a separate copy, not the sender's spare
                // allocation or its future retained-storage permission.
                // This is still the raw queue API: count/byte claims and
                // notification outside the ledger lock remain to be wired.
                let receipt = event.clone();
                permit.send(event);
                ledger.commit(receipt);
                Ok(DispatchOutcome::Enqueued(delivery))
            }
            Err(mpsc::error::TrySendError::Full(())) => {
                Err(DispatchFailure::new(DispatchError::Full(delivery), event))
            }
            Err(mpsc::error::TrySendError::Closed(())) => {
                Err(DispatchFailure::new(DispatchError::Closed(delivery), event))
            }
        }
    }


}

impl<S: Clone> EventBroker<S> {
    /// O(1) snapshot of exact duplicate receipts, excluding destination storage,
    /// index/Arc/allocator overhead, native buffers and process RSS. No payload
    /// contents, adapter vocabulary, eviction or reservation policy is changed.
    pub fn receipt_snapshot(&self) -> Result<ReceiptMemorySnapshot, DispatchError> {
        Ok(self.ledger.lock().map_err(|_| DispatchError::Poisoned)?.receipt_snapshot())
    }

    pub fn new(
        own: Address,
        agent: S,
        outer: S,
        outbound: S,
        duplicate_window: usize,
    ) -> Self {
        assert!(duplicate_window > 0, "duplicate window must be positive");
        Self {
            own,
            agent,
            outer,
            outbound,
            nodes: RwLock::new(HashMap::new()),
            node_generations: Mutex::new(HashMap::new()),
            ledger: Mutex::new(EventLedger::new(duplicate_window)),
        }
    }

    pub fn register_node(
        &self,
        node: impl Into<String>,
        generation: u64,
        sender: S,
    ) -> Result<(), DispatchError> {
        let node = node.into();
        if node.is_empty() || generation == 0 {
            return Err(DispatchError::Invalid(
                "node id and generation are required".into(),
            ));
        }
        let mut generations = self
            .node_generations
            .lock()
            .map_err(|_| DispatchError::Poisoned)?;
        if let Some(current) = generations.get(&node)
            && generation <= *current
        {
            return Err(DispatchError::StaleNode {
                node,
                current_generation: *current,
                incoming_generation: generation,
            });
        }
        let mut nodes = self.nodes.write().map_err(|_| DispatchError::Poisoned)?;
        if nodes.contains_key(&node) {
            return Err(DispatchError::Invalid("node is already registered".into()));
        }
        nodes.insert(node.clone(), NodeRoute { generation, sender,
            admission_paused: Arc::new(std::sync::atomic::AtomicBool::new(false)) });
        generations.insert(node, generation);
        Ok(())
    }

    pub fn unregister_node(&self, node: &str, generation: u64) -> Result<bool, DispatchError> {
        let mut nodes = self.nodes.write().map_err(|_| DispatchError::Poisoned)?;
        let Some(route) = nodes.get(node) else {
            return Ok(false);
        };
        if route.generation != generation {
            return Err(DispatchError::StaleNode {
                node: node.to_owned(),
                current_generation: route.generation,
                incoming_generation: generation,
            });
        }
        let removed = nodes.remove(node);
        drop(nodes);
        // A retained publisher may wake caller code on Drop.
        drop(removed);
        Ok(true)
    }

    fn destination(&self, target: &Endpoint) -> Result<(Delivery, S), DispatchError> {
        if target.agent_address() != &self.own {
            let address = target.agent_address().clone();
            return Ok((Delivery::Outbound(address), self.outbound.clone()));
        }
        match target {
            Endpoint::Agent(_) => Ok((Delivery::Agent, self.agent.clone())),
            Endpoint::Outer(_) => Ok((Delivery::Outer, self.outer.clone())),
            Endpoint::Node {
                node, generation, ..
            } => {
                let nodes = self.nodes.read().map_err(|_| DispatchError::Poisoned)?;
                let route = nodes
                    .get(node)
                    .ok_or_else(|| DispatchError::UnknownNode(node.clone()))?;
                if route.generation != *generation {
                    return Err(DispatchError::StaleNode {
                        node: node.clone(),
                        current_generation: route.generation,
                        incoming_generation: *generation,
                    });
                }
                if route.admission_paused.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(DispatchError::Full(Delivery::Node { node: node.clone(), generation: *generation }));
                }
                Ok((
                    Delivery::Node {
                        node: node.clone(),
                        generation: *generation,
                    },
                    route.sender.clone(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests;
