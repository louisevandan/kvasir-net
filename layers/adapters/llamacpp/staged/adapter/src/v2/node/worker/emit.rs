use super::*;
use crate::v2::commands::ErrorPayload;
use p4_protocol::event::Envelope;

/// A SESSION response prepared for immediate use by this sole worker mutator.
/// It does not reserve asynchronous queue capacity, bytes, or future effects.
/// No emitter, await, or yield may intervene between preparation and commit.
pub(super) struct PreparedEmission {
    event: Event,
    next_event: u64,
}

impl Worker {
    /// SESSION currently is the sole consumer. Preserve its exact existing
    /// payload/envelope construction, but perform fallible work before its
    /// routing authority is installed. Encode alone is insufficient: a large
    /// original ID is repeated as derived ID and causation, while the decoder
    /// also bounds their combined envelope. This temporary round-trip cost is
    /// validation, not a retained-byte reservation or a general wire repair.
    pub(super) fn prepare_json_emission<T: Serialize>(
        &self,
        base: &Event,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        value: &T,
    ) -> Result<PreparedEmission, String> {
        self.ensure_event_id_obligations(1, 0)?;
        let payload = serde_json::to_vec(value)
            .map_err(|error| format!("completion payload serialization failed: {error}"))?;
        let sequence = self.state.next_event;
        let next_event = sequence
            .checked_add(1)
            .ok_or("completion event ID is exhausted")?;
        let envelope = base.envelope.next(
            derived_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        let event = Event { envelope, payload };
        let wire = p4_protocol::event::encode(&event)
            .map_err(|error| format!("completion event cannot be encoded: {error}"))?;
        let decoded = p4_protocol::event::decode(&wire)
            .map_err(|error| format!("completion event cannot be decoded: {error}"))?;
        if decoded != event {
            return Err("completion event changed during its wire round-trip".into());
        }
        Ok(PreparedEmission { event, next_event })
    }

    /// Called immediately after the infallible SESSION authority commit.
    /// Publication still uses the existing blocking Full/Closed behavior;
    /// once committed, a later publication failure does not roll back SESSION.
    pub(super) fn publish_prepared_emission(
        &mut self,
        prepared: PreparedEmission,
    ) -> Result<(), ()> {
        debug_assert_eq!(self.state.next_event, prepared.event.envelope.sequence);
        self.state.next_event = prepared.next_event;
        self.publish_or_wait(prepared.event)
    }

    pub(super) fn emit_batch_errors(
        &mut self,
        bases: &[Event],
        code: &str,
        detail: &str,
    ) -> Result<(), ()> {
        for base in bases {
            self.emit_error(base, code, detail.to_owned())?;
        }
        Ok(())
    }

    pub(super) fn emit_reply_envelope_json<T: Serialize>(
        &mut self,
        base: &Envelope,
        reply: ReplySpec,
        ingress: Address,
        class: EventClass,
        content_type: &str,
        value: &T,
    ) -> Result<(), ()> {
        let payload = serde_json::to_vec(value).map_err(|_| ())?;
        let sequence = self.state.next_event;
        self.state.next_event = self.state.next_event.checked_add(1).ok_or(())?;
        let target = Endpoint::outer(ingress, reply.channel.clone(), reply.connection_generation);
        let mut envelope = base.next(
            derived_envelope_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        envelope.correlation_id = reply.correlation_id;
        envelope.return_route = match &envelope.target {
            Endpoint::Outer(route) => Some(route.clone()),
            _ => unreachable!(),
        };
        envelope.deadline_unix_ms = reply.deadline_unix_ms;
        self.publish_or_wait(Event { envelope, payload })
    }

    pub(super) fn emit_error(
        &mut self,
        base: &Event,
        code: &str,
        detail: String,
    ) -> Result<(), ()> {
        self.emit_json(
            base,
            reply_target(base),
            EventClass::Output,
            ERROR_CONTENT_TYPE,
            &ErrorPayload {
                code: code.into(),
                detail,
            },
        )
    }

    pub(super) fn emit_json<T: Serialize>(
        &mut self,
        base: &Event,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        value: &T,
    ) -> Result<(), ()> {
        let payload = serde_json::to_vec(value).map_err(|_| ())?;
        self.emit_bytes(base, target, class, content_type, payload)
    }

    pub(super) fn emit_bytes(
        &mut self,
        base: &Event,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        payload: Vec<u8>,
    ) -> Result<(), ()> {
        // Direct responses have no committed effect claim. They must not
        // spend the IDs already owed to a queued suffix or future receipt.
        self.ensure_event_id_obligations(1, 0).map_err(|_| ())?;
        self.emit_envelope_bytes_retaining(&base.envelope, target, class, content_type, payload)
            .map_err(|_| ())
    }

    /// Effect publication owns the frame body on success and returns that
    /// same body on failure. The generic Event API above keeps its historical
    /// unit error. ID allocation and blocking Full retry remain unchanged.
    pub(super) fn emit_envelope_bytes_retaining(
        &mut self,
        base: &Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        payload: Vec<u8>,
    ) -> Result<(), Vec<u8>> {
        self.emit_retaining_kind(base, target, class, content_type, payload, false)
    }

    pub(super) fn emit_head_control_retaining(
        &mut self,
        base: &Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        payload: Vec<u8>,
    ) -> Result<(), Vec<u8>> {
        self.emit_retaining_kind(base, target, class, content_type, payload, true)
    }

    fn emit_retaining_kind(
        &mut self,
        base: &Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        payload: Vec<u8>,
        head_control: bool,
    ) -> Result<(), Vec<u8>> {
        // A queued effect consumes its existing share here. The whole-group
        // check belongs before commit, not after a prefix already executed.
        // Retain checked_add below for late ID corruption/failure injection.
        let sequence = self.state.next_event;
        let Some(next_event) = sequence.checked_add(1) else {
            return Err(payload);
        };
        self.state.next_event = next_event;
        let envelope = base.next(
            derived_envelope_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        self.publish_kind(Event { envelope, payload }, head_control)
            .map_err(|event| event.payload)
    }

    /// Publishes a completion, waiting for room rather than dropping it.
    ///
    /// The other direction of this pipe learned to hold an event when the
    /// adapter was full; this one used to discard the completion and fail the
    /// worker, which loses a token that has already been computed and ends
    /// the node for a queue that was about to drain. A closed mailbox is
    /// still fatal - nothing will ever read it. A single completion larger
    /// than the entire storage budget is permanent too: waiting cannot make
    /// that value fit. These errors return the original Event to the caller.
    ///
    /// This runs on the worker's own thread, which owns no lock and holds no
    /// llama context between events, so blocking here backs the pressure up
    /// to the node's inbound queue rather than into the stage server.
    fn publish_or_wait(&mut self, event: Event) -> Result<(), ()> {
        self.publish_or_retain(event).map_err(|_| ())
    }

    fn publish_or_retain(&mut self, event: Event) -> Result<(), Event> {
        self.publish_kind(event, false)
    }

    fn publish_kind(&mut self, event: Event, head_control: bool) -> Result<(), Event> {
        // One unchanged Event stays owned here throughout Full. ACK servicing
        // cannot recursively publish, execute native, or issue another batch.
        debug_assert_eq!(self.active_publications, 0);
        self.active_publications += 1;
        let result = self.wait_for_publication(event, head_control);
        self.active_publications -= 1;
        result
    }

    fn wait_for_publication(&mut self, event: Event, head_control: bool) -> Result<(), Event> {
        let mut pending = event;
        loop {
            // Full may have retired a previously forwarded control. Never
            // reuse a pre-Full ticket against a removed/new pending identity.
            let ticket = if head_control {
                if pending.envelope.class != EventClass::Control {
                    return Err(pending);
                }
                match self.prepare_head_control_forward(
                    &pending.envelope.target,
                    &pending.envelope.payload_content_type,
                    &pending.payload,
                ) {
                    Ok(ticket) => Some(ticket),
                    Err(_) => return Err(pending),
                }
            } else {
                None
            };
            match self.publisher.try_publish(pending) {
                Ok(()) => {
                    if let Some(ticket) = ticket {
                        self.complete_head_forward(ticket);
                    }
                    return Ok(());
                }
                Err(PublishError::Full(event)) => {
                    if self.shutting_down.load(Ordering::SeqCst) {
                        // The reader is going away, so the room this is
                        // waiting for will never come. Abandoning the
                        // completion loses a token; waiting for it hangs the
                        // shutdown, which loses the node.
                        self.set_snapshot("completion_queue_full:abandoned_at_shutdown");
                        return Err(event);
                    }
                    self.set_snapshot("completion_queue_full:waiting");
                    pending = event;
                    if self.service_blocked_ack().is_err() {
                        return Err(pending);
                    }
                    std::thread::sleep(COMPLETION_RETRY_INTERVAL);
                }
                Err(PublishError::Closed(event)) => {
                    self.set_snapshot("completion_queue_closed");
                    return Err(event);
                }
                Err(PublishError::TooLarge { event, .. }) => {
                    self.set_snapshot("completion_storage_budget_exceeded");
                    return Err(event);
                }
                Err(PublishError::CostOverflow(event)) => {
                    self.set_snapshot("completion_storage_cost_overflow");
                    return Err(event);
                }
            }
        }
    }

    pub(super) fn enqueue_deferred_ack_error(&mut self) -> Result<(), ()> {
        let Some((base, detail)) = self.deferred_ack_error.as_ref() else {
            return Ok(());
        };
        let body = serde_json::to_vec(&ErrorPayload {
            code: "LLAMA_ADAPTER_EVENT_REJECTED".into(),
            detail: detail.clone(),
        })
        .map_err(|_| ())?;
        let causal = Event {
            envelope: base.clone(),
            payload: Vec::new(),
        };
        let target = reply_target(&causal);
        let (base, _) = self
            .deferred_ack_error
            .take()
            .expect("single worker owns diagnostic");
        // Transfer the existing ID share; allocate its number only when this
        // FIFO effect reaches publication, after all preceding suffix effects.
        self.effects
            .push_back(super::effects::CommittedEffect::Forward {
                base,
                target,
                class: EventClass::Output,
                content_type: ERROR_CONTENT_TYPE,
                body,
            });
        Ok(())
    }
}

fn derived_event_id(base: &Event, sequence: u64) -> String {
    derived_envelope_event_id(&base.envelope, sequence)
}

fn derived_envelope_event_id(base: &Envelope, sequence: u64) -> String {
    format!("{}:llamacpp:{sequence}", base.event_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_adapter::node_adapter::{Poll, completion_mailbox};
    use p4_protocol::Address;
    use p4_protocol::event::{Envelope, EventClass, OuterEndpoint};

    fn input(id: &str) -> Event {
        let address = Address::tcp("127.0.0.1", 1);
        Event {
            envelope: Envelope {
                protocol_version: Envelope::VERSION,
                event_id: id.into(),
                correlation_id: "same-correlation".into(),
                causation_id: None,
                source: Endpoint::agent(address.clone()),
                target: Endpoint::agent(address),
                return_route: None,
                class: EventClass::Control,
                sequence: 1,
                deadline_unix_ms: None,
                adapter_kind: Some("llamacpp".into()),
                payload_content_type: "test".into(),
            },
            payload: Vec::new(),
        }
    }

    #[test]
    fn different_causal_events_cannot_generate_the_same_completion_id() {
        assert_ne!(
            derived_event_id(&input("node-a-load"), 1),
            derived_event_id(&input("node-b-load"), 1)
        );
    }

    #[test]
    fn an_impossible_completion_cost_is_not_misclassified_as_full_at_shutdown() {
        let address = Address::tcp("127.0.0.1", 1);
        let (_sender, receiver) = mpsc::sync_channel(1);
        let (publisher, mailbox) =
            p4_adapter::node_adapter::completion_mailbox_with_budget(1, 0).unwrap();
        let snapshot = Arc::new(Mutex::new("loaded".into()));
        let mut worker = Worker::new(
            Endpoint::node(address, "node", 1),
            receiver,
            publisher,
            Arc::clone(&snapshot),
            // The guard makes a wrong Full classification terminate too,
            // with a different snapshot. A mutation must fail an assertion,
            // not leave this synchronous worker test spinning forever.
            Arc::new(std::sync::atomic::AtomicBool::new(true)),
        );
        let mut event = input("too-large");
        event.payload = Vec::with_capacity(1024);
        event.payload.extend_from_slice(&[0, 255, 128]);
        let pointer = event.payload.as_ptr();
        let capacity = event.payload.capacity();
        let expected = event.clone();
        let next_event = worker.state.next_event;
        let returned = worker.publish_or_retain(event).unwrap_err();
        assert_eq!(returned, expected);
        assert_eq!(returned.payload.as_ptr(), pointer);
        assert_eq!(returned.payload.capacity(), capacity);
        assert_eq!(worker.state.next_event, next_event);
        assert_eq!(worker.active_publications, 0);
        assert_eq!(mailbox.storage_snapshot().retained_count, 0);
        assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
        assert_eq!(
            snapshot.lock().unwrap().as_str(),
            "completion_storage_budget_exceeded"
        );
        assert_eq!(mailbox.try_take(), Poll::Empty);
    }

    #[test]
    fn one_failed_mixed_batch_reports_every_participating_request() {
        let address = Address::tcp("127.0.0.1", 1);
        let (_sender, receiver) = mpsc::sync_channel(1);
        let (publisher, mailbox) = completion_mailbox(2);
        let mut worker = Worker::new(
            Endpoint::node(address.clone(), "node", 1),
            receiver,
            publisher,
            Arc::new(Mutex::new("loaded".into())),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut first = input("request-a");
        first.envelope.return_route = Some(OuterEndpoint {
            ingress_agent: address.clone(),
            channel: "outer-a".into(),
            connection_generation: 1,
        });
        let mut second = input("request-b");
        second.envelope.return_route = Some(OuterEndpoint {
            ingress_agent: address,
            channel: "outer-b".into(),
            connection_generation: 2,
        });

        assert_eq!(
            worker.emit_batch_errors(
                &[first, second],
                "LLAMA_LOGICAL_BATCH_FAILED",
                "memory_dirty=1;action=reload",
            ),
            Ok(())
        );
        let Poll::Event(first_error) = mailbox.try_take() else {
            panic!("first request did not receive its error event");
        };
        let Poll::Event(second_error) = mailbox.try_take() else {
            panic!("second request did not receive its error event");
        };
        assert_ne!(first_error.envelope.target, second_error.envelope.target);
        assert!(first_error.envelope.event_id.starts_with("request-a:"));
        assert!(second_error.envelope.event_id.starts_with("request-b:"));
        assert_eq!(first_error.payload, second_error.payload);
    }
}
