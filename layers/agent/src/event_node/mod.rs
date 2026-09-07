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

/// Terminal node failure together with every event still owned by this task.
///
/// A refusal is not retirement: the caller now owns these unchanged values
/// and must account for their outcome. This boundary preserves existing Event
/// ownership, not count/byte reservations, graceful drain or remote delivery.
#[derive(Debug)]
pub struct EventNodeFailure {
    pub error: EventNodeError,
    pub held_input: Option<Box<Event>>,
    pub held_output: Option<Box<Event>>,
    /// At most one independent front can be removed after its destination is
    /// reserved. A terminal validation/routing race retains it alongside the
    /// older blocked output; it is never another ordinary holding queue.
    pub completion_at_failure: Option<Box<Event>>,
}

pub struct EventNode {
    adapter: Arc<dyn NodeAdapter>,
    inbound: EventReceiver,
    broker: Arc<EventBroker>,
}

impl EventNode {
    /// This helper is synchronous: no destination permit or receipt pin may
    /// cross an await. It neither executes adapter work nor reads its payload.
    fn forward_independent_front(&self, blocked: &Event) -> Result<(), (EventNodeError, Option<Box<Event>>)> {
        let Some(front) = self.adapter.peek_completion() else { return Ok(()); };
        if front.source == blocked.envelope.source
            && front.correlation_id == blocked.envelope.correlation_id {
            return Ok(());
        }
        let reserved = self.broker.reserve_completion(&front);
        if matches!(&reserved, Err(DispatchError::Full(_))) {
            // The original remains in its real mailbox. Do not create a
            // second held output just to discover another full destination.
            return Ok(());
        }
        match self.adapter.try_take_completion_matching(&front) {
            Poll::Empty => Ok(()), // a changed front never consumes its replacement
            Poll::Closed => Err((EventNodeError::CompletionClosed(self.adapter.snapshot()), None)),
            Poll::Event(event) => match reserved {
                Ok(ticket) => self.broker.dispatch_completion(ticket, event)
                    .map(|_| ())
                    .map_err(|failure| (EventNodeError::Broker(failure.error), Some(failure.event))),
                Err(error) => Err((EventNodeError::Broker(error), Some(Box::new(event)))),
            },
        }
    }

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

    pub async fn run(mut self) -> Result<(), EventNodeFailure> {
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
                    Err(failure) => {
                        held_output = Some(*failure.event);
                        match failure.error {
                            DispatchError::Full(_) => {}
                            error => {
                                return Err(EventNodeFailure {
                                    error: EventNodeError::Broker(error),
                                    held_input: held_input.map(Box::new),
                                    held_output: held_output.map(Box::new),
                                    completion_at_failure: None,
                                });
                            }
                        }
                    }
                }
            }
            if let Some(event) = held_input.take() {
                match self.adapter.try_offer(event) {
                    Ok(()) => {}
                    Err(OfferError::Full(event)) => held_input = Some(event),
                    Err(OfferError::Closed(event)) => {
                        held_input = Some(event);
                        return Err(EventNodeFailure {
                            error: EventNodeError::AdapterClosed,
                            held_input: held_input.map(Box::new),
                            held_output: held_output.map(Box::new),
                            completion_at_failure: None,
                        });
                    }
                }
            }
            if let Some(blocked) = held_output.as_ref()
                && let Err((error, completion_at_failure)) = self.forward_independent_front(blocked) {
                return Err(EventNodeFailure {
                    error,
                    held_input: held_input.map(Box::new),
                    held_output: held_output.map(Box::new),
                    completion_at_failure,
                });
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
                            return Err(EventNodeFailure {
                                error: EventNodeError::CompletionClosed(self.adapter.snapshot()),
                                held_input: held_input.map(Box::new),
                                held_output: held_output.map(Box::new),
                                completion_at_failure: None,
                            });
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
