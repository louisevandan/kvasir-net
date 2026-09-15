//! Execute only effects whose settlement has committed. Token/continuation
//! decisions live upstream; local dispatch progress is acknowledged separately
//! from the downstream KV ACK. Native ambiguity fences further execution.
use super::*;
use p4_protocol::event::Envelope;
use std::collections::VecDeque;

#[derive(Debug)]
pub(super) enum CommittedEffect {
    /// Wire-prevalidated direct response, without an assigned Event ID.
    Direct(super::emit::PreparedEmission),
    /// Failed preflight is retained for diagnosis, never made publishable by
    /// clearing a fence or retrying a different envelope/ID.
    UndeliverableDirect {
        intent: super::emit::DirectEmission,
        detail: String,
    },
    /// Exact wire-level value retained across final publication failures.
    /// IDs and payload bytes are never regenerated from a DTO on retry.
    Publication {
        event: Event,
        after: PublicationAfter,
    },
    PreparedReservedPublication {
        publication: super::emit::PreparedReservedPublication,
        after: PublicationAfter,
    },
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

/// A materialized Event has already spent its event-ID share. The after-action
/// contains only work that becomes eligible AFTER mailbox acceptance. This is
/// neither a storage reservation nor a native-operation reconciliation token.
#[derive(Debug)]
pub(super) enum PublicationAfter {
    Direct {
        diagnostic: bool,
    },
    OutputTrace {
        request_id: String,
        sequence_id: u32,
        position: u32,
    },
    ReleaseReceipt,
    Forward,
    HeadControl,
    Observed(Vec<super::observe::PreparedTelemetry>),
    ReservedObserved(Vec<super::emit::PreparedReservedTelemetry>),
    Telemetry,
}

impl PublicationAfter {
    pub(super) fn failure_message(&self) -> &'static str {
        match self {
            Self::Direct { diagnostic: true } => "committed diagnostic could not be delivered",
            Self::Direct { diagnostic: false } => {
                "committed direct response could not be delivered"
            }
            Self::OutputTrace { .. } => "committed output could not be delivered",
            Self::ReleaseReceipt => "committed release receipt could not be delivered",
            Self::Forward => "committed control could not be delivered",
            Self::HeadControl => "committed head control could not be delivered",
            Self::Observed(_) => "committed physical result could not be delivered",
            Self::ReservedObserved(_) => "reserved physical result could not be delivered",
            Self::Telemetry => "committed observation could not be delivered",
        }
    }
}

impl Worker {
    pub(super) fn flush_effects(&mut self) -> Result<(), String> {
        self.flush_effects_inner(None)
    }

    /// Only a freshly appended diagnostic group whose OLD prefix was empty
    /// may use this mode. Keep the native fence set throughout; never process
    /// a native effect, earlier failed publication, or subsequently added work.
    pub(super) fn flush_terminal_diagnostics(&mut self, count: usize) -> Result<(), String> {
        self.flush_effects_inner(Some(count))
    }

    fn flush_effects_inner(&mut self, mut diagnostic_count: Option<usize>) -> Result<(), String> {
        if self.effects_fenced && diagnostic_count.is_none() {
            return Err("committed effects are fenced after an uncertain failure".into());
        }
        // Still synchronous: no async actor, native group reservation, or byte
        // budget is established here. The popped effect remains owned until it
        // either moves into the real mailbox or returns to the same FIFO head.
        while let Some(mut effect) = self.effects.pop_front() {
            if let Some(remaining) = diagnostic_count {
                if remaining == 0 {
                    self.effects.push_front(effect);
                    return Ok(());
                }
                if !matches!(&effect, CommittedEffect::Direct(prepared) if prepared.is_diagnostic())
                {
                    self.effects.push_front(effect);
                    return Err("terminal diagnostic cannot replay a preceding effect".into());
                }
            }
            if let Err(error) = self.materialize_effect(&mut effect) {
                self.effects.push_front(effect);
                self.effects_fenced = true;
                return Err(error);
            }
            // The current Publication already owns an ID. Its deferred
            // observations still owe IDs; subtracting one here would hide one
            // of those obligations while Full services an incoming ACK.
            self.active_effect_ids = match effect.event_count() {
                Ok(count) => count,
                Err(error) => {
                    self.effects.push_front(effect);
                    self.effects_fenced = true;
                    return Err(error);
                }
            };
            let result = match effect {
                CommittedEffect::Publication { event, after } => {
                    let head_control = matches!(&after, PublicationAfter::HeadControl);
                    match self.publish_kind(event, head_control) {
                        Ok(()) => Ok(Some(after)),
                        Err(event) => {
                            let error = after.failure_message().to_owned();
                            Err((CommittedEffect::Publication { event, after }, error))
                        }
                    }
                }
                CommittedEffect::PreparedReservedPublication { publication, after } => {
                    match self.publish_reserved_kind(publication) {
                        Ok(()) => Ok(Some(after)),
                        Err(publication) => {
                            let error = after.failure_message().to_owned();
                            Err((
                                CommittedEffect::PreparedReservedPublication { publication, after },
                                error,
                            ))
                        }
                    }
                }
                mut native => {
                    let result = match &mut native {
                        CommittedEffect::Settle {
                            load_generation,
                            session_id,
                            sequence,
                        } => self
                            .prepare_head_settle(*load_generation, session_id, sequence)
                            .and_then(|ticket| {
                                // Native can act before a reply is lost. Do not
                                // replace its original intent with a mutable reply.
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
                        _ => unreachable!("publication intents were materialized before execution"),
                    };
                    result.map(|()| None).map_err(|error| (native, error))
                }
            };
            self.active_effect_ids = 0;
            match result {
                Err((effect, error)) => {
                    // Retain the SAME Event, including its envelope and ID.
                    // Full already retries that Event inside publish_kind.
                    // Closed/shutdown/permanent failure does not authorize
                    // replay, ID reallocation, or automatic fence removal.
                    self.effects.push_front(effect);
                    self.effects_fenced = true;
                    return Err(error);
                }
                Ok(Some(PublicationAfter::Observed(telemetry))) => {
                    let stamp = super::observe::unix_ms();
                    // The forward itself is gone only after acceptance. Freeze
                    // one timestamp and preserve recipient order before trying
                    // any observation; their failures cannot repeat forwarding.
                    for mut delivery in telemetry.into_iter().rev() {
                        delivery.forwarded_at(stamp);
                        self.effects
                            .push_front(CommittedEffect::Telemetry(delivery));
                    }
                }
                Ok(Some(PublicationAfter::ReservedObserved(telemetry))) => {
                    let stamp = super::observe::unix_ms();
                    for delivery in telemetry.into_iter().rev() {
                        self.effects
                            .push_front(CommittedEffect::PreparedReservedPublication {
                                publication: delivery.finalize(stamp),
                                after: PublicationAfter::Telemetry,
                            });
                    }
                }
                Ok(Some(PublicationAfter::OutputTrace {
                    request_id,
                    sequence_id,
                    position,
                })) => {
                    if std::env::var_os("P4_STAGED_TRACE_OUTPUT_POSITION").is_some() {
                        crate::v2::record::record(&format!(
                            "P4_OUTPUT_EMITTED request={request_id} sequence={sequence_id} position={position}",
                        ));
                    }
                }
                Ok(_) => {}
            }
            if let Some(remaining) = diagnostic_count.as_mut() {
                *remaining -= 1;
                if *remaining == 0 {
                    return Ok(());
                }
                continue;
            }
            self.enqueue_deferred_ack_error()
                .map_err(|_| "deferred ACK diagnostic could not be retained".to_owned())?;
        }
        Ok(())
    }

    /// Materialize ONLY the current FIFO head. Serialization and checked ID
    /// allocation finish before replacing the intent; no native call, publish,
    /// ACK service, or yield intervenes. A failure before replacement preserves
    /// the original intent. An already frozen Event never gets another ID.
    fn materialize_effect(&mut self, effect: &mut CommittedEffect) -> Result<(), String> {
        let (event, after) = match effect {
            CommittedEffect::Direct(prepared) => {
                // Check before moving the body. Rejected allocation preserves
                // the exact unnumbered prepared response and all earlier IDs.
                let sequence = self.state.next_event;
                let next_event = sequence
                    .checked_add(1)
                    .ok_or("completion event ID is exhausted")?;
                let after = PublicationAfter::Direct {
                    diagnostic: prepared.is_diagnostic(),
                };
                let event = prepared.materialize(sequence);
                self.state.next_event = next_event;
                (event, after)
            }
            CommittedEffect::UndeliverableDirect { detail, .. } => return Err(detail.clone()),
            CommittedEffect::Output {
                base,
                reply,
                ingress,
                payload,
            } => {
                let after = PublicationAfter::OutputTrace {
                    request_id: payload.outcome.request_id.clone(),
                    sequence_id: payload.outcome.sequence_id,
                    position: payload.outcome.position,
                };
                let event = self
                    .materialize_reply_envelope_json(
                        base,
                        reply,
                        ingress,
                        EventClass::Output,
                        OUTPUT_CONTENT_TYPE,
                        payload,
                    )
                    .map_err(|_| after.failure_message().to_owned())?;
                (event, after)
            }
            CommittedEffect::ReleaseReceipt {
                base,
                reply,
                ingress,
                payload,
            } => {
                let after = PublicationAfter::ReleaseReceipt;
                let event = self
                    .materialize_reply_envelope_json(
                        base,
                        reply,
                        ingress,
                        EventClass::Telemetry,
                        RELEASE_RECEIPT_CONTENT_TYPE,
                        payload,
                    )
                    .map_err(|_| after.failure_message().to_owned())?;
                (event, after)
            }
            CommittedEffect::Forward {
                base,
                target,
                class,
                content_type,
                body,
            } => {
                let after = PublicationAfter::Forward;
                let event = self
                    .materialize_envelope_bytes(base, target.clone(), *class, content_type, body)
                    .map_err(|_| after.failure_message().to_owned())?;
                (event, after)
            }
            CommittedEffect::ForwardHeadControl {
                base,
                target,
                class,
                content_type,
                body,
            } => {
                // Retain the original pre-publication validation order. This
                // ticket is deliberately not stored across any Full servicing.
                self.prepare_head_control_forward(target, content_type, body)?;
                if *class != EventClass::Control {
                    return Err("head control forward has the wrong event class".into());
                }
                let after = PublicationAfter::HeadControl;
                let event = self
                    .materialize_envelope_bytes(base, target.clone(), *class, content_type, body)
                    .map_err(|_| after.failure_message().to_owned())?;
                (event, after)
            }
            CommittedEffect::ForwardObserved {
                base,
                target,
                class,
                content_type,
                body,
                telemetry,
            } => {
                let event = self
                    .materialize_envelope_bytes(base, target.clone(), *class, content_type, body)
                    .map_err(|_| "committed physical result could not be delivered".to_owned())?;
                (event, PublicationAfter::Observed(std::mem::take(telemetry)))
            }
            CommittedEffect::Telemetry(delivery) => {
                let event = match &delivery.payload {
                    super::observe::TelemetryPayload::Batch(payload) => self
                        .materialize_reply_envelope_json(
                            &delivery.base,
                            &delivery.reply,
                            &delivery.ingress,
                            EventClass::Telemetry,
                            BATCH_OBSERVATION_CONTENT_TYPE,
                            payload,
                        ),
                    super::observe::TelemetryPayload::Span(payload) => self
                        .materialize_reply_envelope_json(
                            &delivery.base,
                            &delivery.reply,
                            &delivery.ingress,
                            EventClass::Telemetry,
                            STAGE_SPAN_CONTENT_TYPE,
                            payload,
                        ),
                }
                .map_err(|_| "committed observation could not be delivered".to_owned())?;
                (event, PublicationAfter::Telemetry)
            }
            CommittedEffect::PreparedReservedPublication { publication, .. } => {
                if !publication.owns_event_id() {
                    let sequence = self.state.next_event;
                    let next_event = sequence
                        .checked_add(1)
                        .ok_or("completion event ID is exhausted")?;
                    publication.materialize(sequence)?;
                    self.state.next_event = next_event;
                }
                return Ok(());
            }
            CommittedEffect::Publication { .. }
            | CommittedEffect::Settle { .. }
            | CommittedEffect::Release { .. } => {
                return Ok(());
            }
        };
        *effect = CommittedEffect::Publication { event, after };
        Ok(())
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
            let ingress = reply.context()?.route.ingress_agent;
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
