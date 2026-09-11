//! Retained local delivery. Receipt identity/window semantics are shared with
//! the raw broker; destination bytes remain charged through downstream holds.
use super::*;
use p4_adapter::node_adapter::{
    CompletionFront, CompletionPublisher, CompletionQueueReservation, CompletionReservation,
    DeferredCompletionNotification, ReserveError, ReservedPublishReason, RetainedCompletion,
    retained_event_bytes,
};

pub type RetainedEventBroker = EventBroker<CompletionPublisher>;

#[derive(Debug)]
pub struct RetainedDispatchFailure {
    pub error: DispatchError,
    pub completion: Box<RetainedCompletion>,
}

impl std::fmt::Display for RetainedDispatchFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl std::error::Error for RetainedDispatchFailure {}

/// Immediate front operation only: no ticket is held across an await, native
/// execution or remote transfer. It reserves actual bytes AND the queue slot.
pub(crate) struct RetainedCompletionDispatch {
    front: CompletionFront,
    kind: RetainedDispatchKind,
}

enum RetainedDispatchKind {
    Existing(Arc<receipt_memory::Receipt>),
    Destination {
        delivery: Delivery,
        sender: CompletionPublisher,
        slot: CompletionQueueReservation,
        reservation: CompletionReservation,
    },
}

enum Input {
    Raw(Event),
    Retained(RetainedCompletion),
}
impl Input {
    fn event(&self) -> &Event {
        match self {
            Self::Raw(event) => event,
            Self::Retained(completion) => completion.event(),
        }
    }
}

fn admission_error(error: ReserveError, delivery: &Delivery) -> DispatchError {
    match error {
        ReserveError::Full => DispatchError::Full(delivery.clone()),
        ReserveError::Closed => DispatchError::Closed(delivery.clone()),
        ReserveError::TooLarge { required, limit } => DispatchError::StorageTooLarge {
            delivery: delivery.clone(),
            required,
            limit,
        },
        ReserveError::InvalidCount | ReserveError::CostOverflow => {
            DispatchError::Invalid("retained Event footprint overflow".into())
        }
    }
}

impl EventBroker<CompletionPublisher> {
    /// Fence delivery before observing quiescence; caller code never executes
    /// under the registration lock. Existing owned front tickets recheck this.
    pub fn pause_node_admission(&self, node: &str, generation: u64) -> Result<NodeAdmissionPause, DispatchError> {
        let nodes = self.nodes.write().map_err(|_| DispatchError::Poisoned)?;
        let route = nodes.get(node).ok_or_else(|| DispatchError::UnknownNode(node.into()))?;
        if route.generation != generation {
            return Err(DispatchError::StaleNode { node: node.into(), current_generation: route.generation,
                incoming_generation: generation });
        }
        if route.admission_paused.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return Err(DispatchError::Full(Delivery::Node { node: node.into(), generation }));
        }
        Ok(NodeAdmissionPause(Arc::clone(&route.admission_paused)))
    }


    pub(crate) fn reserve_retained_completion(
        &self,
        front: &CompletionFront,
    ) -> Result<RetainedCompletionDispatch, DispatchError> {
        front
            .envelope
            .validate()
            .map_err(|e| DispatchError::Invalid(e.to_string()))?;
        let existing = self
            .ledger
            .lock()
            .map_err(|_| DispatchError::Poisoned)?
            .inspect_completion_header(&front.envelope)?;
        let kind = if let Some(receipt) = existing {
            RetainedDispatchKind::Existing(receipt)
        } else {
            let (delivery, sender) = self.destination(&front.envelope.target)?;
            let (slot, reservation) = sender
                .try_reserve_delivery(front.event_bytes)
                .map_err(|error| admission_error(error, &delivery))?;
            RetainedDispatchKind::Destination {
                delivery,
                sender,
                slot,
                reservation,
            }
        };
        Ok(RetainedCompletionDispatch {
            front: front.clone(),
            kind,
        })
    }

    /// Raw ingress must acquire this actual retained destination before it is
    /// accepted. The original raw owner is returned on every refusal.
    pub fn dispatch_ingress(&self, event: Event) -> Result<DispatchOutcome, DispatchFailure> {
        self.dispatch_input(None, Input::Raw(event))
            .map_err(|(error, input)| match input {
                Input::Raw(event) => DispatchFailure::new(error, event),
                Input::Retained(_) => unreachable!("ingress preserves its input mode"),
            })
    }

    pub fn dispatch_retained(
        &self,
        completion: RetainedCompletion,
    ) -> Result<DispatchOutcome, RetainedDispatchFailure> {
        self.dispatch_retained_with_ticket(None, completion)
    }

    pub(crate) fn dispatch_retained_completion(
        &self,
        ticket: RetainedCompletionDispatch,
        completion: RetainedCompletion,
    ) -> Result<DispatchOutcome, RetainedDispatchFailure> {
        self.dispatch_retained_with_ticket(Some(ticket), completion)
    }

    fn dispatch_retained_with_ticket(
        &self,
        ticket: Option<RetainedCompletionDispatch>,
        completion: RetainedCompletion,
    ) -> Result<DispatchOutcome, RetainedDispatchFailure> {
        self.dispatch_input(ticket, Input::Retained(completion))
            .map_err(|(error, input)| match input {
                Input::Retained(completion) => RetainedDispatchFailure {
                    error,
                    completion: Box::new(completion),
                },
                Input::Raw(_) => unreachable!("retained input never becomes raw on refusal"),
            })
    }

    fn dispatch_input(
        &self,
        ticket: Option<RetainedCompletionDispatch>,
        input: Input,
    ) -> Result<DispatchOutcome, (DispatchError, Input)> {
        if let Err(error) = input.event().validate() {
            return Err((DispatchError::Invalid(error.to_string()), input));
        }
        let event_bytes = match retained_event_bytes(input.event()) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Err((
                    DispatchError::Invalid("retained Event footprint overflow".into()),
                    input,
                ));
            }
        };
        let front = CompletionFront {
            envelope: input.event().envelope.clone(),
            event_bytes,
        };
        let ticket = match ticket {
            Some(ticket) if ticket.front != front => {
                return Err((
                    DispatchError::Invalid("completion front changed after reservation".into()),
                    input,
                ));
            }
            Some(ticket) => ticket,
            None => match self.reserve_retained_completion(&front) {
                Ok(ticket) => ticket,
                Err(error) => return Err((error, input)),
            },
        };
        let (delivery, sender, slot, reservation) = match ticket.kind {
            RetainedDispatchKind::Existing(receipt) => {
                return if receipt.event() == input.event() {
                    Ok(DispatchOutcome::Duplicate) // input retires outside every broker lock
                } else {
                    Err((DispatchError::ConflictingDuplicate, input))
                };
            }
            RetainedDispatchKind::Destination {
                delivery,
                sender,
                slot,
                reservation,
            } => (delivery, sender, slot, reservation),
        };
        // All callback-bearing owners live outside the transaction closure.
        // This also covers duplicate/route/closure races and permission cleanup.
        let mut input = Some(input);
        let mut permissions = Some((slot, reservation));
        let result = (|| {
            let mut ledger = self.ledger.lock().map_err(|_| DispatchError::Poisoned)?;
            match ledger.inspect(input.as_ref().unwrap().event())? {
                LedgerVerdict::Duplicate => return Ok((DispatchOutcome::Duplicate, None)),
                LedgerVerdict::New => {}
            }
            // Keep node registration stable through acceptance. Checking it and
            // releasing the read lock before enqueue would admit a removed route.
            let nodes = self.nodes.read().map_err(|_| DispatchError::Poisoned)?;
            if let Delivery::Node { node, generation } = &delivery {
                let current = nodes
                    .get(node)
                    .ok_or_else(|| DispatchError::UnknownNode(node.clone()))?;
                if current.generation != *generation {
                    return Err(DispatchError::StaleNode {
                        node: node.clone(),
                        current_generation: current.generation,
                        incoming_generation: *generation,
                    });
                }
                if !current.sender.same_mailbox(&sender) {
                    return Err(DispatchError::Invalid(
                        "completion destination changed after reservation".into(),
                    ));
                }
            }
            if let Delivery::Node { node, .. } = &delivery {
                if nodes.get(node).expect("validated route").admission_paused.load(std::sync::atomic::Ordering::Acquire) {
                    return Err(DispatchError::Full(delivery.clone()));
                }
            }
            // Independent exact receipt: never bind the source claim to the
            // deduplication window. A receipt byte limit is a separate migration.
            let receipt = input.as_ref().unwrap().event().clone();
            let (slot, reservation) = permissions.take().unwrap();
            let notification: Result<DeferredCompletionNotification, ReservedPublishReason> =
                match input.take().unwrap() {
                    Input::Raw(event) => sender
                        .publish_with_queue_deferred(event, reservation, slot)
                        .map_err(|failure| {
                            input = Some(Input::Raw(failure.event));
                            permissions = Some((failure.slot, failure.reservation));
                            failure.reason
                        }),
                    Input::Retained(completion) => completion
                        .transfer_with_queue_deferred(&sender, reservation, slot)
                        .map_err(|failure| {
                            input = Some(Input::Retained(failure.completion));
                            permissions = Some((failure.slot, failure.reservation));
                            failure.reason
                        }),
                };
            let notification = notification.map_err(|reason| match reason {
                ReservedPublishReason::Closed => DispatchError::Closed(delivery.clone()),
                other => DispatchError::Invalid(format!("reserved delivery rejected: {other:?}")),
            })?;
            ledger.commit(receipt);
            Ok((
                DispatchOutcome::Enqueued(delivery.clone()),
                Some(notification),
            ))
        })();
        match result {
            Ok((outcome, notification)) => {
                // Explicit ordering: ledger/registration unlocked; release old
                // source and unused permissions before notifying the receiver.
                drop(input);
                drop(permissions);
                if let Some(notification) = notification {
                    notification.notify();
                }
                Ok(outcome)
            }
            Err(error) => Err((error, input.expect("refusal preserves original input"))),
        }
    }
}

#[cfg(test)]
mod tests;
