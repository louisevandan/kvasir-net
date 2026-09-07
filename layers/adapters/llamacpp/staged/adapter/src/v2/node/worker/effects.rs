//! Execute only effects whose settlement has committed. Token/continuation
//! decisions live upstream; local dispatch progress is acknowledged separately
//! from the downstream KV ACK. Native ambiguity fences further execution.
use super::*;
use p4_protocol::event::Envelope;
use std::collections::VecDeque;

#[derive(Debug)]
pub(super) enum CommittedEffect {
    Output {
        base: Envelope,
        reply: ReplySpec,
        ingress: Address,
        payload: ApprovedOutputPayload,
    },
    ReleaseReceipt {
        base: Envelope,
        reply: ReplySpec,
        ingress: Address,
        payload: ReleaseReceipt,
    },
    Forward {
        base: Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &'static str,
        body: Vec<u8>,
    },
    /// Only a head-originating mutation establishes pending dispatch authority.
    /// Middle/tail forwarding remains the separate generic effect above.
    ForwardHeadControl {
        base: Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &'static str,
        body: Vec<u8>,
    },
    ForwardObserved {
        base: Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &'static str,
        body: Vec<u8>,
        telemetry: Vec<super::observe::PreparedTelemetry>,
    },
    Telemetry(super::observe::PreparedTelemetry),
    Settle {
        load_generation: u64,
        session_id: String,
        sequence: SettlementSequence,
    },
    Release {
        load_generation: u64,
        session_id: String,
        sequence: ReleaseSequence,
    },
}

impl Worker {
    pub(super) fn flush_effects(&mut self) -> Result<(), String> {
        if self.effects_fenced {
            return Err("committed effects are fenced after an uncertain failure".into());
        }
        // Still synchronous: splitting this loop into actor turns must not
        // interleave one command's native controls without group reservation.
        // The popped active effect remains owned by this call. A future byte
        // or count budget must count it too, not only the queued suffix.
        while let Some(mut effect) = self.effects.pop_front() {
            self.active_effect_ids = effect.event_count()?.saturating_sub(1);
            let mut after_forward = None;
            let result = match &mut effect {
                CommittedEffect::Output {
                    base,
                    reply,
                    ingress,
                    payload,
                } => self
                    .emit_reply_envelope_json(
                        base,
                        reply.clone(),
                        ingress.clone(),
                        EventClass::Output,
                        OUTPUT_CONTENT_TYPE,
                        payload,
                    )
                    .map_err(|_| "committed output could not be delivered".to_owned())
                    .map(|()| {
                        if std::env::var_os("P4_STAGED_TRACE_OUTPUT_POSITION").is_some() {
                            crate::v2::record::record(&format!(
                                "P4_OUTPUT_EMITTED request={} sequence={} position={}",
                                payload.outcome.request_id,
                                payload.outcome.sequence_id,
                                payload.outcome.position,
                            ));
                        }
                    }),
                CommittedEffect::ReleaseReceipt {
                    base,
                    reply,
                    ingress,
                    payload,
                } => self
                    .emit_reply_envelope_json(
                        base,
                        reply.clone(),
                        ingress.clone(),
                        EventClass::Telemetry,
                        RELEASE_RECEIPT_CONTENT_TYPE,
                        payload,
                    )
                    .map_err(|_| "committed release receipt could not be delivered".to_owned()),
                CommittedEffect::Forward {
                    base,
                    target,
                    class,
                    content_type,
                    body,
                } => self
                    .emit_effect_body(base, target.clone(), *class, content_type, body)
                    .map_err(|_| "committed control could not be delivered".to_owned()),
                CommittedEffect::ForwardHeadControl {
                    base,
                    target,
                    class,
                    content_type,
                    body,
                } => self
                    .prepare_head_control_forward(target, content_type, body)
                    .and_then(|_| {
                        if *class != EventClass::Control {
                            return Err("head control forward has the wrong event class".into());
                        }
                        match self.emit_head_control_retaining(
                            base,
                            target.clone(),
                            *class,
                            content_type,
                            std::mem::take(body),
                        ) {
                            Ok(()) => Ok(()),
                            Err(unsent) => {
                                *body = unsent;
                                Err("committed head control could not be delivered".to_owned())
                            }
                        }
                    }),
                CommittedEffect::Settle {
                    load_generation,
                    session_id,
                    sequence,
                } => self
                    .prepare_head_settle(*load_generation, session_id, sequence)
                    .and_then(|ticket| {
                        // Native settlement fills proposal on its input. Keep
                        // this control candidate separate so a malformed reply
                        // cannot mutate the retained original intent.
                        let mut sequences = vec![sequence.clone()];
                        self.settle_stage_sequences(&mut sequences)?;
                        if sequences[0].proposal.is_empty() {
                            self.complete_head_local(ticket);
                            Ok(())
                        } else {
                            Err("first-stage settlement produced a proposal".into())
                        }
                    }),
                CommittedEffect::Release {
                    load_generation,
                    session_id,
                    sequence,
                } => self
                    .prepare_head_release(*load_generation, session_id, sequence)
                    .and_then(|ticket| {
                        self.release_stage_sequence(sequence)?;
                        self.complete_head_local(ticket);
                        Ok(())
                    }),
                CommittedEffect::ForwardObserved {
                    base,
                    target,
                    class,
                    content_type,
                    body,
                    telemetry,
                } => self
                    .emit_effect_body(base, target.clone(), *class, content_type, body)
                    .map_err(|_| "committed physical result could not be delivered".to_owned())
                    .map(|()| {
                        let stamp = super::observe::unix_ms();
                        for delivery in telemetry.iter_mut() {
                            delivery.forwarded_at(stamp);
                        }
                        after_forward = Some(std::mem::take(telemetry));
                    }),
                CommittedEffect::Telemetry(delivery) => match &delivery.payload {
                    super::observe::TelemetryPayload::Batch(payload) => self
                        .emit_reply_envelope_json(
                            &delivery.base,
                            delivery.reply.clone(),
                            delivery.ingress.clone(),
                            EventClass::Telemetry,
                            BATCH_OBSERVATION_CONTENT_TYPE,
                            payload,
                        ),
                    super::observe::TelemetryPayload::Span(payload) => self
                        .emit_reply_envelope_json(
                            &delivery.base,
                            delivery.reply.clone(),
                            delivery.ingress.clone(),
                            EventClass::Telemetry,
                            STAGE_SPAN_CONTENT_TYPE,
                            payload,
                        ),
                }
                .map_err(|_| "committed observation could not be delivered".to_owned()),
            };
            self.active_effect_ids = 0;
            if let Err(error) = result {
                // Restore the same owned intent, not a cloned DTO. Forward
                // bytes have been returned to it by emit_effect_body. An
                // engine response may be lost after it acted; replay requires
                // operation reconciliation,
                // which this native wire does not yet provide.
                self.effects.push_front(effect);
                self.effects_fenced = true;
                return Err(error);
            }
            if let Some(telemetry) = after_forward {
                // Forward succeeded. Retain the fixed payloads before trying
                // any recipient; a later failure must not repeat forwarding.
                for delivery in telemetry.into_iter().rev() {
                    self.effects
                        .push_front(CommittedEffect::Telemetry(delivery));
                }
            }
            self.enqueue_deferred_ack_error()
                .map_err(|_| "deferred ACK diagnostic could not be retained".to_owned())?;
        }
        Ok(())
    }

    /// Move a frame body into publication. On failure the same allocation is
    /// returned to its original intent; a successful publication owns it now.
    /// This remains synchronous and does not add a resumable actor reservation.
    fn emit_effect_body(
        &mut self,
        base: &Envelope,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        body: &mut Vec<u8>,
    ) -> Result<(), ()> {
        match self.emit_envelope_bytes_retaining(
            base,
            target,
            class,
            content_type,
            std::mem::take(body),
        ) {
            Ok(()) => Ok(()),
            Err(unsent) => {
                *body = unsent;
                Err(())
            }
        }
    }

    pub(super) fn prepare_outputs(
        base: &Event,
        outputs: Vec<(
            capsule::RowOwner,
            capsule::GeneratedToken,
            String,
            Option<u64>,
            Option<crate::v2::IssuedWorkProof>,
        )>,
    ) -> Result<VecDeque<CommittedEffect>, String> {
        let mut effects = VecDeque::new();
        for (owner, token, submission_event_id, release_operation_id, issued_work) in outputs {
            let reply: ReplySpec = serde_json::from_str(&owner.reply)
                .map_err(|_| "tail reply contract is invalid".to_owned())?;
            if reply.correlation_id.is_empty()
                || reply.channel.is_empty()
                || reply.connection_generation == 0
            {
                return Err("tail reply contract is incomplete".into());
            }
            let ingress = Address::from_str(&reply.ingress_agent)
                .map_err(|_| "tail reply ingress is invalid".to_owned())?;
            let payload = ApprovedOutputPayload {
                submission_event_id,
                incarnation: owner.incarnation,
                release_operation_id,
                issued_work,
                outcome: OutcomePayload {
                    load_generation: owner.load_generation,
                    session_id: owner.session_id,
                    request_id: owner.request_id,
                    sequence_id: owner.sequence_id,
                    token: token.token,
                    text: token.text,
                    position: token.position,
                    stop: token.stop,
                },
            };
            payload.validate().map_err(str::to_owned)?;
            effects.push_back(CommittedEffect::Output {
                // Causation/routing need the envelope, never the incoming
                // physical/Tail payload once per generated output token.
                base: base.envelope.clone(),
                reply,
                ingress,
                payload,
            });
        }
        Ok(effects)
    }
}
