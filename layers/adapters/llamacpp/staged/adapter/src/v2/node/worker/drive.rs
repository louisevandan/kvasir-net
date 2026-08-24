use super::*;
use crate::v2::commands::ErrorPayload;

impl Worker {
    pub(super) fn drive_first_batches(&mut self) -> Result<(), ()> {
        loop {
            let Some(session_id) = self.state.first_session_with_work() else {
                return Ok(());
            };
            let session = match self.state.sessions.get(&session_id).cloned() {
                Some(session) => session,
                None => return Ok(()),
            };
            let mut demands = Vec::new();
            for (key, request) in &self.state.requests {
                if request.command.session_id != session_id {
                    continue;
                }
                let Some(phase) = request.phase() else {
                    continue;
                };
                let available_rows = match phase {
                    Phase::Prefill => request.command.tokens.len() - request.prompt_cursor,
                    Phase::Decode => 1,
                };
                demands.push(Demand {
                    request_id: key.clone(),
                    sequence_id: request.sequence_id.expect("work has an admitted sequence"),
                    compatibility: session_id.clone(),
                    phase,
                    available_rows,
                });
            }
            if demands.is_empty() {
                return Ok(());
            }
            let allocations = self
                .scheduler
                .plan(&demands, self.state.batch_capacity)
                .map_err(|error| {
                    self.set_snapshot(&format!("scheduler_failed:{error:?}"));
                })?;
            if allocations.is_empty() {
                return Ok(());
            }
            let mut rows = Vec::new();
            let mut updates = Vec::new();
            let mut template = None;
            for allocation in allocations {
                let request = self
                    .state
                    .requests
                    .get(&allocation.request_id)
                    .expect("scheduler allocation references active request");
                if template.is_none() {
                    template = Some(request.template.clone());
                }
                match allocation.phase {
                    Phase::Prefill => {
                        for offset in 0..allocation.rows {
                            let index = request.prompt_cursor + offset;
                            let position = u32::try_from(index).map_err(|_| ())?;
                            rows.push(LogicalRow {
                                owner: RowOwner {
                                    request_id: request.command.request_id.clone(),
                                    sequence_key: allocation.request_id.clone(),
                                    session_id: session_id.clone(),
                                    reply: request.reply.clone(),
                                    sequence_id: request
                                        .sequence_id
                                        .expect("work has an admitted sequence"),
                                    phase: Phase::Prefill,
                                    position,
                                    max_tokens: request.command.max_tokens,
                                    generated_tokens: request.generated,
                                    output: index + 1 == request.command.tokens.len(),
                                    options: request.command.options.clone(),
                                },
                                token: request.command.tokens[index],
                            });
                        }
                    }
                    Phase::Decode => {
                        let (token, position) = request
                            .decode
                            .expect("decode allocation has a pending token");
                        rows.push(LogicalRow {
                            owner: RowOwner {
                                request_id: request.command.request_id.clone(),
                                sequence_key: allocation.request_id.clone(),
                                session_id: session_id.clone(),
                                reply: request.reply.clone(),
                                sequence_id: request
                                    .sequence_id
                                    .expect("work has an admitted sequence"),
                                phase: Phase::Decode,
                                position,
                                max_tokens: request.command.max_tokens,
                                generated_tokens: request.generated,
                                output: true,
                                options: request.command.options.clone(),
                            },
                            token,
                        });
                    }
                }
                updates.push((allocation.request_id, allocation.phase, allocation.rows));
            }
            let logical_rows = rows.len();
            let logical = LogicalBatch(rows).encode().map_err(|error| {
                self.set_snapshot(&format!("logical_encode_failed:{error:?}"));
            })?;
            let body = self
                .stage_request(Operation::LogicalBatch, Operation::PhysicalResult, logical)
                .map_err(|detail| {
                    self.set_snapshot(&format!("logical_batch_failed:{detail}"));
                })?;
            let physical = CapsuleSet::decode(&body).map_err(|error| {
                self.set_snapshot(&format!("physical_result_failed:{error:?}"));
            })?;
            if single_session(&physical).map_err(|_| ())? != session_id
                || physical.0.iter().any(|capsule| capsule.terminal)
            {
                self.set_snapshot("physical_result_identity_failed");
                return Err(());
            }
            self.emit_batch_observation(
                &template
                    .clone()
                    .expect("non-empty allocation has a template"),
                &session_id,
                logical_rows,
                &physical,
            )?;
            for (request_id, phase, count) in updates {
                let request = self
                    .state
                    .requests
                    .get_mut(&request_id)
                    .expect("successful batch keeps request active");
                match phase {
                    Phase::Prefill => request.prompt_cursor += count,
                    Phase::Decode => request.decode = None,
                }
            }
            self.emit_bytes(
                &template.expect("non-empty allocation has a template"),
                session.next.expect("validated first session has next"),
                EventClass::Data,
                PHYSICAL_BATCH_CONTENT_TYPE,
                body,
            )?;
        }
    }

    pub(super) fn emit_tail_results(
        &mut self,
        base: &Event,
        session: &PipelineSession,
        result: CapsuleSet,
        body: Vec<u8>,
    ) -> Result<(), ()> {
        if result.0.iter().any(|capsule| !capsule.terminal) {
            self.set_snapshot("tail_returned_non_terminal_capsule");
            return Err(());
        }
        for capsule in &result.0 {
            for outcome in &capsule.outcomes {
                let owner = capsule.owners.get(outcome.owner_index as usize).ok_or(())?;
                let reply: ReplySpec = serde_json::from_str(&owner.reply).map_err(|_| ())?;
                if reply.correlation_id.is_empty()
                    || reply.channel.is_empty()
                    || reply.connection_generation == 0
                {
                    return Err(());
                }
                let ingress = Address::from_str(&reply.ingress_agent).map_err(|_| ())?;
                let payload = OutcomePayload {
                    request_id: owner.request_id.clone(),
                    sequence_id: owner.sequence_id,
                    token: outcome.token,
                    text: outcome.text.clone(),
                    position: outcome.position,
                    stop: outcome.stop.clone(),
                };
                self.emit_reply_json(
                    base,
                    reply,
                    ingress,
                    EventClass::Output,
                    OUTPUT_CONTENT_TYPE,
                    &payload,
                )?;
            }
        }
        self.emit_bytes(
            base,
            session.first.clone(),
            EventClass::Data,
            TAIL_BATCH_CONTENT_TYPE,
            body,
        )
    }

    pub(super) fn emit_reply_json<T: Serialize>(
        &mut self,
        base: &Event,
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
        let mut envelope = base.envelope.next(
            derived_event_id(base, sequence),
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
        self.publisher
            .try_publish(Event { envelope, payload })
            .map_err(|_| {
                self.set_snapshot("completion_queue_full");
            })
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
        let sequence = self.state.next_event;
        self.state.next_event = self.state.next_event.checked_add(1).ok_or(())?;
        let envelope = base.envelope.next(
            derived_event_id(base, sequence),
            self.endpoint.clone(),
            target,
            class,
            sequence,
            content_type,
        );
        self.publisher
            .try_publish(Event { envelope, payload })
            .map_err(|_| {
                self.set_snapshot("completion_queue_full");
            })
    }
}

fn derived_event_id(base: &Event, sequence: u64) -> String {
    format!("{}:llamacpp:{sequence}", base.envelope.event_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_protocol::Address;
    use p4_protocol::event::{Envelope, EventClass};

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
}
