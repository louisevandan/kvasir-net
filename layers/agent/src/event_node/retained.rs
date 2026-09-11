//! Explicit retained variant of the reactive node boundary. Production roots
//! must select this with an owned adapter and owned downstream consumers.
use super::{EventNodeError, HELD_RETRY_INTERVAL};
use crate::event_broker::{DispatchError, RetainedEventBroker};
use p4_adapter::node_adapter::{
    CompletionMailbox, OwnedPoll, RetainedCompletion, RetainedNodeAdapter, RetainedOfferError,
};
use std::future::poll_fn;
use std::sync::Arc;

/// Terminal node failure together with every event still owned by this task.
///
/// A refusal is not retirement: the caller now owns these unchanged values
/// and must account for their outcome. This boundary preserves existing Event
/// ownership, including count/byte claims through every held state. Graceful drain and
/// remote delivery remain separate contracts.
#[derive(Debug)]
pub struct RetainedEventNodeFailure {
    pub error: EventNodeError,
    pub held_input: Option<Box<RetainedCompletion>>,
    pub held_output: Option<Box<RetainedCompletion>>,
    /// At most one independent front can be removed after its destination is
    /// reserved. A terminal validation/routing race retains it alongside the
    /// older blocked output; it is never another ordinary holding queue.
    pub completion_at_failure: Option<Box<RetainedCompletion>>,
}

pub struct RetainedEventNode {
    adapter: Arc<dyn RetainedNodeAdapter>,
    inbound: Arc<CompletionMailbox>,
    broker: Arc<RetainedEventBroker>,
}

impl RetainedEventNode {
    /// This helper is synchronous: no destination permit or receipt pin may
    /// cross an await. It neither executes adapter work nor reads its payload.
    fn forward_independent_front(
        &self,
        blocked: &RetainedCompletion,
    ) -> Result<(), (EventNodeError, Option<Box<RetainedCompletion>>)> {
        let Some(front) = self.adapter.peek_retained_completion() else {
            return Ok(());
        };
        if front.envelope.source == blocked.event().envelope.source
            && front.envelope.correlation_id == blocked.event().envelope.correlation_id
        {
            return Ok(());
        }
        let reserved = self.broker.reserve_retained_completion(&front);
        if matches!(&reserved, Err(DispatchError::Full(_))) {
            // The original remains in its real mailbox. Do not create a
            // second held output just to discover another full destination.
            return Ok(());
        }
        match self.adapter.try_take_retained_matching(&front) {
            OwnedPoll::Empty => Ok(()), // a changed front never consumes its replacement
            OwnedPoll::Closed => Err((
                EventNodeError::CompletionClosed(self.adapter.snapshot()),
                None,
            )),
            OwnedPoll::Event(event) => match reserved {
                Ok(ticket) => self
                    .broker
                    .dispatch_retained_completion(ticket, event)
                    .map(|_| ())
                    .map_err(|failure| {
                        (
                            EventNodeError::Broker(failure.error),
                            Some(failure.completion),
                        )
                    }),
                Err(error) => Err((EventNodeError::Broker(error), Some(Box::new(event)))),
            },
        }
    }

    pub fn new(
        adapter: Arc<dyn RetainedNodeAdapter>,
        inbound: Arc<CompletionMailbox>,
        broker: Arc<RetainedEventBroker>,
    ) -> Self {
        Self {
            adapter,
            inbound,
            broker,
        }
    }

    pub async fn run(self) -> Result<(), RetainedEventNodeFailure> {
        // Retain at most one event in each direction. Waiting exclusively on
        // an outbound Full deadlocks two nodes whose own inbound queues need
        // draining to make room for one another. Neither Full commits the
        // broker ledger nor consumes the adapter input; retry the exact event.
        // This is not a general proof for a fully saturated cyclic network:
        // adapters may also be Full. End-to-end credits remain a separate gate.
        let mut held_input: Option<RetainedCompletion> = None;
        let mut held_output: Option<RetainedCompletion> = None;
        let mut input_closed = false;
        loop {
            if let Some(event) = held_output.take() {
                match self.broker.dispatch_retained(event) {
                    Ok(_) => {}
                    Err(failure) => {
                        held_output = Some(*failure.completion);
                        match failure.error {
                            DispatchError::Full(_) => {}
                            error => {
                                return Err(RetainedEventNodeFailure {
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
                match self.adapter.try_offer_retained(event) {
                    Ok(()) => {}
                    Err(RetainedOfferError::Full(event)) => held_input = Some(event),
                    Err(RetainedOfferError::Closed(event)) => {
                        held_input = Some(event);
                        return Err(RetainedEventNodeFailure {
                            error: EventNodeError::AdapterClosed,
                            held_input: held_input.map(Box::new),
                            held_output: held_output.map(Box::new),
                            completion_at_failure: None,
                        });
                    }
                }
            }
            if let Some(blocked) = held_output.as_ref()
                && let Err((error, completion_at_failure)) = self.forward_independent_front(blocked)
            {
                return Err(RetainedEventNodeFailure {
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
                inbound = poll_fn(|context| self.inbound.poll_take_owned(context)), if !input_closed && held_input.is_none() => {
                    match inbound {
                        OwnedPoll::Event(event) => held_input = Some(event),
                        OwnedPoll::Closed => input_closed = true,
                        OwnedPoll::Empty => {},
                    }
                }
                completion = poll_fn(|context| self.adapter.poll_take_retained(context)), if held_output.is_none() => {
                    match completion {
                        OwnedPoll::Event(event) => held_output = Some(event),
                        // Correct mailbox implementations return Pending and
                        // register a waker. Defensively avoid spinning on an
                        // adapter that instead reports Ready(Empty).
                        OwnedPoll::Empty => tokio::time::sleep(HELD_RETRY_INTERVAL).await,
                        OwnedPoll::Closed => {
                            return Err(RetainedEventNodeFailure {
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
