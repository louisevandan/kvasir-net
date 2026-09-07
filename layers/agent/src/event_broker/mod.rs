//! Non-blocking P4 v2 event routing.
//!
//! The broker reads only the target endpoint. A successful dispatch means the
//! event moved to one bounded queue; it never means the target completed work.

mod ledger;

use ledger::{EventLedger, LedgerVerdict};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event};
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
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

pub struct EventBroker {
    own: Address,
    agent: EventSender,
    outer: EventSender,
    outbound: EventSender,
    nodes: RwLock<HashMap<String, NodeRoute>>,
    node_generations: Mutex<HashMap<String, u64>>,
    ledger: Mutex<EventLedger>,
}

#[derive(Clone)]
struct NodeRoute {
    generation: u64,
    sender: EventSender,
}

impl EventBroker {
    pub fn new(
        own: Address,
        agent: EventSender,
        outer: EventSender,
        outbound: EventSender,
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
        sender: EventSender,
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
        nodes.insert(node.clone(), NodeRoute { generation, sender });
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
        nodes.remove(node);
        Ok(true)
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

    fn destination(&self, target: &Endpoint) -> Result<(Delivery, EventSender), DispatchError> {
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
