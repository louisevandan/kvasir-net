//! Reactive v2 node: move inbound events into one concrete adapter and move
//! adapter completions back to the agent broker. Neither direction executes
//! adapter business logic or waits for a business response.

use crate::event_broker::{DispatchError, EventBroker, EventReceiver};
use p4_adapter::node_adapter::{NodeAdapter, OfferError, Poll};
use p4_protocol::event::Event;
use std::future::poll_fn;
use std::sync::Arc;
use std::time::Duration;

/// How long the node waits before offering a held event again when the
/// adapter or broker destination is full. This bounds idle retry frequency;
/// it still incurs timer work and is not a capacity-notification mechanism.
const HELD_RETRY_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventNodeError {
    AdapterFull,
    AdapterClosed,
    CompletionClosed(String),
    Broker(DispatchError),
}

pub struct EventNode {
    adapter: Arc<dyn NodeAdapter>,
    inbound: EventReceiver,
    broker: Arc<EventBroker>,
}

impl EventNode {
    pub fn new(
        adapter: Arc<dyn NodeAdapter>,
        inbound: EventReceiver,
        broker: Arc<EventBroker>,
    ) -> Self {
        Self {
            adapter,
            inbound,
            broker,
        }
    }

    pub async fn run(mut self) -> Result<(), EventNodeError> {
        // Retain at most one event in each direction. Waiting exclusively on
        // an outbound Full deadlocks two nodes whose own inbound queues need
        // draining to make room for one another. Neither Full commits the
        // broker ledger nor consumes the adapter input; retry the exact event.
        // This is not a general proof for a fully saturated cyclic network:
        // adapters may also be Full. End-to-end credits remain a separate gate.
        let mut held_input: Option<Event> = None;
        let mut held_output: Option<Event> = None;
        let mut input_closed = false;
        loop {
            if let Some(event) = held_output.take() {
                match self.broker.dispatch(event) {
                    Ok(_) => {}
                    Err(DispatchError::Full(_, returned)) => held_output = Some(*returned),
                    Err(error) => return Err(EventNodeError::Broker(error)),
                }
            }
            if let Some(event) = held_input.take() {
                match self.adapter.try_offer(event) {
                    Ok(()) => {}
                    Err(OfferError::Full(event)) => held_input = Some(event),
                    Err(OfferError::Closed) => return Err(EventNodeError::AdapterClosed),
                }
            }
            if input_closed && held_input.is_none() && held_output.is_none() {
                // Preserve the existing input-close contract, but never drop
                // an event already held here. Adapter-wide graceful drain is
                // not implied by this local transport completion.
                return Ok(());
            }
            tokio::select! {
                inbound = self.inbound.recv(), if !input_closed && held_input.is_none() => {
                    match inbound {
                        Some(event) => held_input = Some(event),
                        None => input_closed = true,
                    }
                }
                completion = poll_fn(|context| self.adapter.poll_take(context)), if held_output.is_none() => {
                    match completion {
                        Poll::Event(event) => held_output = Some(event),
                        // Correct mailbox implementations return Pending and
                        // register a waker. Defensively avoid spinning on an
                        // adapter that instead reports Ready(Empty).
                        Poll::Empty => tokio::time::sleep(HELD_RETRY_INTERVAL).await,
                        Poll::Closed => {
                            return Err(EventNodeError::CompletionClosed(self.adapter.snapshot()));
                        }
                    }
                }
                () = tokio::time::sleep(HELD_RETRY_INTERVAL),
                    if held_input.is_some() || held_output.is_some() => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
