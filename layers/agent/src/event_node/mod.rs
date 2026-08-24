//! Reactive v2 node: move inbound events into one concrete adapter and move
//! adapter completions back to the agent broker. Neither direction executes
//! adapter business logic or waits for a business response.

use crate::event_broker::{DispatchError, EventBroker, EventReceiver};
use p4_adapter::node_adapter::{NodeAdapter, OfferError, Poll};
use std::future::poll_fn;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventNodeError {
    AdapterFull,
    AdapterClosed,
    CompletionClosed,
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
        loop {
            tokio::select! {
                inbound = self.inbound.recv() => {
                    let Some(event) = inbound else { return Ok(()); };
                    match self.adapter.try_offer(event) {
                        Ok(()) => {}
                        Err(OfferError::Full) => return Err(EventNodeError::AdapterFull),
                        Err(OfferError::Closed) => return Err(EventNodeError::AdapterClosed),
                    }
                }
                completion = poll_fn(|context| self.adapter.poll_take(context)) => {
                    match completion {
                        Poll::Event(event) => {
                            self.broker.dispatch(event).map_err(EventNodeError::Broker)?;
                        }
                        Poll::Empty => continue,
                        Poll::Closed => return Err(EventNodeError::CompletionClosed),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
