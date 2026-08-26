use super::*;

impl Worker {
    pub(super) fn drive_first_batches(&mut self) -> Result<(), ()> {
        loop {
            // The scheduler places Verify after ordinary rows.  Keep it the
            // last physical work admitted until the tail either commits it or
            // every stage has applied the partial-accept settlement.
            if self.state.verify_fenced() {
                return Ok(());
            }
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
                    Phase::Decode | Phase::Verify | Phase::Replay => request
                        .ready
                        .as_ref()
                        .expect("ready phase has rows")
                        .tokens
                        .len(),
                };
                demands.push(Demand {
                    request_id: key.clone(),
                    sequence_id: request.sequence_id.expect("work has an admitted sequence"),
                    compatibility: session_id.clone(),
                    phase,
                    available_rows,
                    atomic: matches!(phase, Phase::Verify | Phase::Replay),
                });
            }
            if demands.is_empty() {
                return Ok(());
            }
            let allocations = self
                .scheduler
                .plan_with_physical_capacity(
                    &demands,
                    self.state.batch_capacity,
                    self.state.physical_capacity,
                    self.state.equal_sequence_ubatch,
                    self.state.max_atomic_sequences,
                    self.state.atomic_batch_exclusive,
                )
                .map_err(|error| {
                    self.set_snapshot(&format!("scheduler_failed:{error:?}"));
                })?;
            if allocations.is_empty() {
                return Ok(());
            }
            let mut rows = Vec::new();
            let mut updates = Vec::new();
            let mut batch_events = Vec::new();
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
                batch_events.push(request.template.clone());
                match allocation.phase {
                    Phase::Prefill => {
                        for offset in 0..allocation.rows {
                            let index = request.prompt_cursor + offset;
                            let position = u32::try_from(index).map_err(|_| ())?;
                            rows.push(LogicalRow {
                                owner: RowOwner {
                                    load_generation: self.state.load_generation,
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
                                    input_token: request.command.tokens[index],
                                    speculative_id: 0,
                                    speculative_index: 0,
                                    speculative_count: 0,
                                    options: request.command.options.clone(),
                                },
                                token: request.command.tokens[index],
                            });
                        }
                    }
                    Phase::Decode | Phase::Verify | Phase::Replay => {
                        let ready = request.ready.as_ref().expect("allocated rows are ready");
                        if allocation.rows != ready.tokens.len() {
                            self.set_snapshot("atomic_or_decode_allocation_was_split");
                            return Err(());
                        }
                        let count = u32::try_from(ready.tokens.len()).map_err(|_| ())?;
                        for (offset, token) in ready.tokens.iter().copied().enumerate() {
                            let position = ready
                                .position
                                .checked_add(u32::try_from(offset).map_err(|_| ())?)
                                .ok_or(())?;
                            let speculative =
                                matches!(allocation.phase, Phase::Verify | Phase::Replay);
                            rows.push(LogicalRow {
                                owner: RowOwner {
                                    load_generation: self.state.load_generation,
                                    request_id: request.command.request_id.clone(),
                                    sequence_key: allocation.request_id.clone(),
                                    session_id: session_id.clone(),
                                    reply: request.reply.clone(),
                                    sequence_id: request
                                        .sequence_id
                                        .expect("work has an admitted sequence"),
                                    phase: allocation.phase,
                                    position,
                                    max_tokens: request.command.max_tokens,
                                    generated_tokens: request.generated,
                                    output: true,
                                    input_token: token,
                                    speculative_id: if speculative {
                                        ready.speculative_id
                                    } else {
                                        0
                                    },
                                    speculative_index: if speculative { offset as u32 } else { 0 },
                                    speculative_count: if speculative { count } else { 0 },
                                    options: request.command.options.clone(),
                                },
                                token,
                            });
                        }
                    }
                }
                updates.push((allocation.request_id, allocation.phase, allocation.rows));
            }
            let logical_rows = rows.len();
            let logical = match LogicalBatch(rows).encode() {
                Ok(logical) => logical,
                Err(error) => {
                    let detail = format!("logical batch encoding failed: {error:?}");
                    self.set_snapshot(&format!("logical_encode_failed:{error:?}"));
                    self.emit_batch_errors(
                        &batch_events,
                        "LLAMA_LOGICAL_BATCH_ENCODE_FAILED",
                        &detail,
                    )?;
                    return Err(());
                }
            };
            let body = match self.stage_request(
                Operation::LogicalBatch,
                Operation::PhysicalResult,
                logical,
            ) {
                Ok(body) => body,
                Err(detail) => {
                    self.set_snapshot(&format!("logical_batch_failed:{detail}"));
                    self.emit_batch_errors(&batch_events, "LLAMA_LOGICAL_BATCH_FAILED", &detail)?;
                    return Err(());
                }
            };
            let physical = match CapsuleSet::decode(&body) {
                Ok(physical) => physical,
                Err(error) => {
                    let detail = format!("physical result decoding failed: {error:?}");
                    self.set_snapshot(&format!("physical_result_failed:{error:?}"));
                    self.emit_batch_errors(
                        &batch_events,
                        "LLAMA_PHYSICAL_RESULT_INVALID",
                        &detail,
                    )?;
                    return Err(());
                }
            };
            if single_session(&physical).ok().as_deref() != Some(session_id.as_str())
                || physical.0.iter().any(|capsule| capsule.terminal)
            {
                self.set_snapshot("physical_result_identity_failed");
                self.emit_batch_errors(
                    &batch_events,
                    "LLAMA_PHYSICAL_RESULT_INVALID",
                    "physical result identity or stage role is invalid",
                )?;
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
            let mut verify_request_ids = Vec::new();
            for (request_id, phase, count) in updates {
                let request = self
                    .state
                    .requests
                    .get_mut(&request_id)
                    .expect("successful batch keeps request active");
                let _ = count;
                request.in_flight = true;
                if phase == Phase::Verify {
                    verify_request_ids.push(request_id);
                }
            }
            if !verify_request_ids.is_empty() {
                if let Err(detail) = self.state.begin_verify_fence(&verify_request_ids) {
                    self.set_snapshot(detail);
                    self.emit_batch_errors(&batch_events, "LLAMA_VERIFY_FENCE_FAILED", detail)?;
                    return Err(());
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
                for generated in &outcome.generated {
                    let payload = OutcomePayload {
                        load_generation: owner.load_generation,
                        session_id: owner.session_id.clone(),
                        request_id: owner.request_id.clone(),
                        sequence_id: owner.sequence_id,
                        token: generated.token,
                        text: generated.text.clone(),
                        position: generated.position,
                        stop: generated.stop.clone(),
                    };
                    self.emit_reply_json(
                        base,
                        reply.clone(),
                        ingress.clone(),
                        EventClass::Output,
                        OUTPUT_CONTENT_TYPE,
                        &payload,
                    )?;
                }
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
}
