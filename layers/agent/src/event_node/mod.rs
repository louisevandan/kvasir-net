//! Reactive v2 node: move inbound events into one concrete adapter and move
//! adapter completions back to the agent broker. Neither direction executes
//! adapter business logic or waits for a business response.

use crate::event_broker::{DispatchError, EventBroker, EventReceiver};
use p4_adapter::node_adapter::{NodeAdapter, OfferError, Poll};
use p4_protocol::event::Event;
use std::future::poll_fn;
use std::sync::Arc;

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
        // An event the adapter had no room for. While one is held the node
        // stops reading inbound and only drains completions - which is what
        // frees the adapter's room - so the pressure stays upstream instead of
        // arriving here as a dead node. A full adapter used to end the task
        // with `AdapterFull`, turning a busy pipeline into a stopped one.
        let mut held: Option<Event> = None;
        loop {
            if let Some(event) = held.take() {
                match self.adapter.try_offer(event) {
                    Ok(()) => {}
                    Err(OfferError::Full(event)) => {
                        held = Some(event);
                        // Take one completion, then try again. `Empty` means the
                        // waker is registered and nothing is ready, so yield
                        // rather than spin on it.
                        match poll_fn(|context| self.adapter.poll_take(context)).await {
                            Poll::Event(event) => {
                                self.broker.dispatch(event).map_err(EventNodeError::Broker)?;
                            }
                            Poll::Empty => tokio::task::yield_now().await,
                            Poll::Closed => {
                                return Err(EventNodeError::CompletionClosed(
                                    self.adapter.snapshot(),
                                ));
                            }
                        }
                        continue;
                    }
                    Err(OfferError::Closed) => return Err(EventNodeError::AdapterClosed),
                }
            }
            tokio::select! {
                inbound = self.inbound.recv() => {
                    let Some(event) = inbound else { return Ok(()); };
                    match self.adapter.try_offer(event) {
                        Ok(()) => {}
                        Err(OfferError::Full(event)) => held = Some(event),
                        Err(OfferError::Closed) => return Err(EventNodeError::AdapterClosed),
                    }
                }
                completion = poll_fn(|context| self.adapter.poll_take(context)) => {
                    match completion {
                        Poll::Event(event) => {
                            self.broker.dispatch(event).map_err(EventNodeError::Broker)?;
                        }
                        Poll::Empty => continue,
                        Poll::Closed => {
                            return Err(EventNodeError::CompletionClosed(self.adapter.snapshot()));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
