use super::*;
use crate::v2::commands::ErrorPayload;
use p4_protocol::event::Envelope;

/// Unnumbered direct-response ownership. A failed wire preflight retains these
/// exact parts, but never grants them publication authority.
#[derive(Debug)]
pub(super) struct DirectEmission {
    pub(super) base: Envelope,
    pub(super) source: Endpoint,
    pub(super) target: Endpoint,
    pub(super) class: EventClass,
    pub(super) content_type: String,
    pub(super) body: Vec<u8>,
    diagnostic: bool,
}

/// The complete body and all envelope fields have passed a wire round-trip
/// with the widest usable future sequence. The actual ID is assigned ONLY at
/// the committed FIFO head. This is not a storage/byte reservation.
#[derive(Debug)]
pub(super) struct PreparedEmission(DirectEmission);

impl PreparedEmission {
    #[cfg(test)]
    pub(super) fn intent_for_test(&self) -> &DirectEmission {
        &self.0
    }

    pub(super) fn is_diagnostic(&self) -> bool {
        self.0.diagnostic
    }

    pub(super) fn materialize(&mut self, sequence: u64) -> Event {
        let intent = &mut self.0;
        Event {
            envelope: intent.base.next(
                derived_envelope_event_id(&intent.base, sequence),
                intent.source.clone(),
                intent.target.clone(),
                intent.class,
                sequence,
                intent.content_type.clone(),
            ),
            payload: std::mem::take(&mut intent.body),
        }
    }
}

impl DirectEmission {
    fn prepare(mut self) -> Result<PreparedEmission, (Self, String)> {
        // MAX itself cannot be issued because the counter must advance by one.
        // Every other issued decimal ID is no wider than this validation ID;
        // all other fields and the serialized body remain immutable.
        let sequence = u64::MAX - 1;
        let event = Event {
            envelope: self.base.next(
                derived_envelope_event_id(&self.base, sequence),
                self.source.clone(),
                self.target.clone(),
                self.class,
                sequence,
                self.content_type.clone(),
            ),
            payload: std::mem::take(&mut self.body),
        };
        let result = p4_protocol::event::encode(&event)
            .map_err(|error| format!("completion event cannot be encoded: {error}"))
            .and_then(|wire| {
                let decoded = p4_protocol::event::decode(&wire)
                    .map_err(|error| format!("completion event cannot be decoded: {error}"))?;
                if decoded != event {
                    return Err("completion event changed during its wire round-trip".into());
                }
                Ok(())
            });
        self.body = event.payload;
        match result {
            Ok(()) => Ok(PreparedEmission(self)),
            Err(detail) => Err((self, detail)),
        }
    }
}

impl Worker {
    /// SESSION performs all response interpretation before installing routing
    /// authority. Earlier FIFO suffixes may still owe IDs, so preparation must
    /// neither choose the actual sequence nor consume its counter.
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
        self.direct_intent(base, target, class, content_type, payload, false)
            .prepare()
            .map_err(|(_, detail)| detail)
    }

    /// Called immediately after the infallible SESSION authority commit. The
    /// prepared body joins the same FIFO as all preceding committed effects.
    /// A later publication failure retains that effect; it does not roll back
    /// SESSION or manufacture a differently numbered response.
    pub(super) fn publish_prepared_emission(
        &mut self,
        prepared: PreparedEmission,
    ) -> Result<(), ()> {
        self.commit_direct_effects(vec![effects::CommittedEffect::Direct(prepared)], false)
    }

    pub(super) fn emit_batch_errors(
        &mut self,
        bases: &[impl std::borrow::Borrow<Event>],
        code: &str,
        detail: &str,
    ) -> Result<(), ()> {
        if bases.is_empty() {
            return Ok(());
        }
        let count = u64::try_from(bases.len()).map_err(|_| ())?;
        self.ensure_event_id_obligations(count, 0).map_err(|_| ())?;
        let mut prepared = Vec::with_capacity(bases.len());
        for base in bases {
            let base = base.borrow();
            let body = serde_json::to_vec(&ErrorPayload {
                code: code.into(),
                detail: detail.into(),
            })
            .map_err(|_| ())?;
            prepared.push(Self::direct_effect(self.direct_intent(
                base,
                reply_target(base),
                EventClass::Output,
                ERROR_CONTENT_TYPE,
                body,
                true,
            )));
        }
        // Every failed computation owner is retained before trying the first
        // delivery. Closed on owner one must not lose the remaining owners.
        self.commit_direct_effects(prepared, true)
    }

    /// Freeze the reply Event once at its committed FIFO position. This only
    /// spends the pre-existing event-ID obligation; it does not reserve storage
    /// or grant authority to replay an uncertain operation.
    pub(super) fn materialize_reply_envelope_json<T: Serialize>(
        &mut self,
        base: &Envelope,
        reply: &ReplySpec,
        ingress: &Address,
        class: EventClass,
        content_type: &str,
        value: &T,
    ) -> Result<Event, ()> {
        let payload = serde_json::to_vec(value).map_err(|_| ())?;
        let sequence = self.state.next_event;
        let next_event = sequence.checked_add(1).ok_or(())?;
        let target = Endpoint::outer(
            ingress.clone(),
            reply.channel.clone(),
            reply.connection_generation,
        );
        let mut envelope = base.next(
            derived_envelope_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        envelope.correlation_id = reply.correlation_id.clone();
        envelope.return_route = match &envelope.target {
            Endpoint::Outer(route) => Some(route.clone()),
            _ => unreachable!(),
        };
        envelope.deadline_unix_ms = reply.deadline_unix_ms;
        self.state.next_event = next_event;
        Ok(Event { envelope, payload })
    }

    /// Move the exact body only after checked ID allocation succeeds. No
    /// publisher, native call, or input servicing occurs while constructing it.
    pub(super) fn materialize_envelope_bytes(
        &mut self,
        base: &Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        payload: &mut Vec<u8>,
    ) -> Result<Event, ()> {
        let sequence = self.state.next_event;
        let next_event = sequence.checked_add(1).ok_or(())?;
        let envelope = base.next(
            derived_envelope_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        let event = Event {
            envelope,
            payload: std::mem::take(payload),
        };
        self.state.next_event = next_event;
        Ok(event)
    }

    pub(super) fn emit_error(
        &mut self,
        base: &Event,
        code: &str,
        detail: String,
    ) -> Result<(), ()> {
        let body = serde_json::to_vec(&ErrorPayload {
            code: code.into(),
            detail,
        })
        .map_err(|_| ())?;
        self.ensure_event_id_obligations(1, 0).map_err(|_| ())?;
        let effect = Self::direct_effect(self.direct_intent(
            base,
            reply_target(base),
            EventClass::Output,
            ERROR_CONTENT_TYPE,
            body,
            true,
        ));
        self.commit_direct_effects(vec![effect], true)
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
        // Admission owns one future ID, not a number allocated ahead of an
        // older effect. Permanent failure preserves the whole intent/Event.
        self.ensure_event_id_obligations(1, 0).map_err(|_| ())?;
        let effect = Self::direct_effect(self.direct_intent(
            base,
            target,
            class,
            content_type,
            payload,
            false,
        ));
        self.commit_direct_effects(vec![effect], false)
    }

    fn direct_intent(
        &self,
        base: &Event,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        body: Vec<u8>,
        diagnostic: bool,
    ) -> DirectEmission {
        DirectEmission {
            base: base.envelope.clone(),
            source: self.endpoint.clone(),
            target,
            class,
            content_type: content_type.into(),
            body,
            diagnostic,
        }
    }

    fn direct_effect(intent: DirectEmission) -> effects::CommittedEffect {
        match intent.prepare() {
            Ok(prepared) => effects::CommittedEffect::Direct(prepared),
            Err((intent, detail)) => {
                effects::CommittedEffect::UndeliverableDirect { intent, detail }
            }
        }
    }

    fn commit_direct_effects(
        &mut self,
        prepared: Vec<effects::CommittedEffect>,
        diagnostic: bool,
    ) -> Result<(), ()> {
        if prepared.is_empty() {
            return Ok(());
        }
        // This decision belongs to this freshly prepared diagnostic group,
        // BEFORE append. A retained failed Publication/native prefix must not
        // be replayed merely because another diagnostic arrives afterward.
        let fenced_diagnostic = diagnostic
            && self.effects_fenced
            && self.effects.is_empty()
            && self.active_publications == 0
            && self.active_effect_ids == 0;
        let count = prepared.len();
        let invalid = prepared.iter().find_map(|effect| match effect {
            effects::CommittedEffect::UndeliverableDirect { detail, .. } => Some(detail.clone()),
            _ => None,
        });
        self.effects.extend(prepared);
        if let Some(detail) = invalid {
            self.effects_fenced = true;
            let previous = self.snapshot.lock().map(|s| s.clone()).unwrap_or_default();
            self.set_snapshot(&format!("{previous};direct_response_invalid:{detail}"));
            return Err(());
        }
        if self.effects_fenced && !fenced_diagnostic {
            return Err(());
        }
        if self.active_publications != 0 {
            // The outer publisher owns the current FIFO head and its ID
            // shares. Never recurse into it or consume the appended suffix.
            return Ok(());
        }
        let previous =
            diagnostic.then(|| self.snapshot.lock().map(|s| s.clone()).unwrap_or_default());
        let result = if fenced_diagnostic {
            self.flush_terminal_diagnostics(count)
        } else {
            self.flush_effects()
        };
        if result.is_err()
            && let Some(previous) = previous
        {
            let failure = self.snapshot.lock().map(|s| s.clone()).unwrap_or_default();
            if failure != previous {
                self.set_snapshot(&format!("{previous};direct_diagnostic_failed:{failure}"));
            }
        }
        result.map_err(|_| ())
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
    #[cfg(test)]
    fn publish_or_retain(&mut self, event: Event) -> Result<(), Event> {
        self.publish_kind(event, false)
    }

    pub(super) fn publish_kind(&mut self, event: Event, head_control: bool) -> Result<(), Event> {
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
                    #[cfg(test)]
                    self.observe_issue_state("publication_full_after_ack");
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

#[cfg(test)]
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
