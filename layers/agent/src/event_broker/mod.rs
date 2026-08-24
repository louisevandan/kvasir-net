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
    Node(String),
    Outer,
    Outbound(Address),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchError {
    Invalid(String),
    ConflictingDuplicate,
    SequenceRegression { previous: u64, incoming: u64 },
    UnknownNode(String),
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

pub struct EventBroker {
    own: Address,
    agent: EventSender,
    outer: EventSender,
    outbound: EventSender,
    nodes: RwLock<HashMap<String, EventSender>>,
    ledger: Mutex<EventLedger>,
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
            ledger: Mutex::new(EventLedger::new(duplicate_window)),
        }
    }

    pub fn register_node(
        &self,
        node: impl Into<String>,
        sender: EventSender,
    ) -> Result<(), DispatchError> {
        let node = node.into();
        if node.is_empty() {
            return Err(DispatchError::Invalid("node id cannot be empty".into()));
        }
        self.nodes
            .write()
            .map_err(|_| DispatchError::Poisoned)?
            .insert(node, sender);
        Ok(())
    }

    pub fn unregister_node(&self, node: &str) -> Result<bool, DispatchError> {
        Ok(self
            .nodes
            .write()
            .map_err(|_| DispatchError::Poisoned)?
            .remove(node)
            .is_some())
    }

    pub fn dispatch(&self, event: Event) -> Result<DispatchOutcome, DispatchError> {
        event
            .validate()
            .map_err(|error| DispatchError::Invalid(error.to_string()))?;
        let mut ledger = self.ledger.lock().map_err(|_| DispatchError::Poisoned)?;
        match ledger.inspect(&event)? {
            LedgerVerdict::Duplicate => return Ok(DispatchOutcome::Duplicate),
            LedgerVerdict::New => {}
        }

        let (delivery, sender) = self.destination(&event.envelope.target)?;
        match sender.try_send(event.clone()) {
            Ok(()) => {
                ledger.commit(event);
                Ok(DispatchOutcome::Enqueued(delivery))
            }
            Err(mpsc::error::TrySendError::Full(_)) => Err(DispatchError::Full(delivery)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(DispatchError::Closed(delivery)),
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
            Endpoint::Node { node, .. } => {
                let sender = self
                    .nodes
                    .read()
                    .map_err(|_| DispatchError::Poisoned)?
                    .get(node)
                    .cloned()
                    .ok_or_else(|| DispatchError::UnknownNode(node.clone()))?;
                Ok((Delivery::Node(node.clone()), sender))
            }
        }
    }
}

#[cfg(test)]
mod tests;
