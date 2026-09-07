use super::*;

impl Worker {
    // Existing method-level fixtures deliberately drain all currently eligible
    // work. Production actor tests go through run(), never this convenience.
    #[cfg(test)]
    pub(super) fn drive_first_batches(&mut self) -> Result<(), ()> {
        while self.drive_one_batch()? {}
        Ok(())
    }

    /// True means one logical issue was accepted and forwarded. False means
    /// external input (e.g. a tail return) is needed, not permission to spin.
    pub(super) fn drive_one_batch(&mut self) -> Result<bool, ()> {
        if self.active_publications != 0
            || !self.effects.is_empty()
            || self.deferred_ack_error.is_some()
        {
            return Ok(false);
        }
        if self.shutting_down.load(Ordering::Acquire) {
            self.set_snapshot("stopped:shutdown_requested:before_issue");
            return Err(());
        }
        if self.effects_fenced || self.state.prepared_issue.is_some() {
            return Err(());
        }
        // The scheduler places Verify after ordinary rows.  Keep it the
        // last physical work admitted until the tail either commits it or
        // every stage has applied the partial-accept settlement.
        if self.state.verify_fenced() {
            return Ok(false);
        }
        // Arrival-phase coalescing. A batch is planned from whatever is
        // ready at this instant, so a group of requests that once arrived
        // together keeps re-forming with exactly those members: measured
        // over a continuous-arrival run, 4,483 physical batches carried
        // only 38 distinct membership sets, 2.84 rows each. Groups born at
        // different times never merge because each is in flight while the
        // others become ready.
        //
        // Coalescing holds a plan back until enough sequences are ready to
        // share one batch. Waiting for full quiescence (threshold beyond
        // the sequence count) maximises width but costs all pipeline
        // depth; measured on the continuous-arrival scenario it widened
        // batches 2.84 -> 10.56 rows and took mixed batches 2 -> 19, yet
        // cost 12% of throughput because a step is not fixed-cost enough
        // for width alone to pay for the lost depth. A finite threshold
        // keeps both: batches stay at least this wide, and the rest of the
        // active set stays in flight behind them.
        //
        // Normal completion re-enters this loop; the threshold is ignored
        // once nothing is in flight. Missing completions still require the
        // separate timeout/reconciliation contract: this is no liveness proof.
        if self.state.min_batch_rows > 1
            && self.state.any_in_flight()
            && self.state.ready_row_count() < self.state.min_batch_rows
        {
            self.gate_refusals = self.gate_refusals.saturating_add(1);
            return Ok(false);
        }
        // Tail-aware issue. The stage spans put the lap in the tail's
        // mailbox: a batch that reached node 3 while it was busy waited
        // 128 ms (p50) for the previous one, and 63% of them did. Each
        // batch costs the tail about 55 ms before its first layer, so a
        // queue of batches at the tail is fixed cost paid several times
        // for rows that could have shared one payment. Holding the plan
        // here while that many batches are already in the pipeline lets
        // the rows that would have queued at the tail merge into one
        // wider batch at the head instead - the wait is the same wait,
        // moved to where it widens something. It is not a row threshold:
        // when the pipeline has room the plan goes at once, however thin,
        // so depth is kept; the earlier quiescence experiment lost 26% by
        // waiting for width regardless of room.
        //
        // This historical hypothesis is not a promotion result. The knob
        // remains disabled by default and normal completion releases it.
        // Lost replies require reconciliation, not an assumed eventual tail.
        if self.state.max_open_batches > 0
            && self.state.any_in_flight()
            && self.state.open_batches.len() >= self.state.max_open_batches
        {
            self.gate_refusals = self.gate_refusals.saturating_add(1);
            return Ok(false);
        }
        let Some(session_id) = self.state.first_session_with_work() else {
            return Ok(false);
        };
        let session = match self.state.sessions.get(&session_id).cloned() {
            Some(session) => session,
            None => return Ok(false),
        };
        let mut demands = Vec::new();
        for (key, request) in &self.state.requests {
            if request.command.session_id != session_id {
                continue;
            }
            let Some(phase) = request.phase_within(self.state.prefill_fragments) else {
                continue;
            };
            let available_rows = match phase {
                Phase::Prefill => request.command.tokens.len() - request.prompt_issued,
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
            return Ok(false);
        }
        let ingress_unix_ms = super::observe::unix_ms();
        let ready_rows = self.state.available_row_count();
        let ready_sequences = self.state.ready_row_count();
        let idle_ms = self
            .last_stage_done
            .map(|at| at.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let idle_gated = self.gate_refusals;
        // Issue width. Across eighteen runs of one scenario throughput
        // tracked the share of the run with two or more stages computing
        // (r=0.891) and ran mildly *against* batch width (r=-0.357) and
        // UBATCH fill. A ready set issued as one wide batch occupies one
        // stage at a time; the same rows issued as several narrower
        // batches can be on several stages at once, and a sequence's
        // next token needs the whole lap either way. Capping the width
        // here trades the per-batch cost - about 55 ms before the first
        // layer - for that overlap, and which way the trade goes is the
        // measurement this knob exists to take.
        //
        // Never applied while a speculative transaction is pending: a
        // Verify or Replay allocation must stay whole inside one UBATCH,
        // and the scheduler reserves it against the full capacity.
        let atomic_pending = demands.iter().any(|demand| demand.atomic);
        let issue_cap = if self.state.max_issue_rows == 0 || atomic_pending {
            usize::MAX
        } else {
            self.state.max_issue_rows
        };
        let policy = self
            .scheduler
            .prepare_plan_with_physical_capacity(
                &demands,
                self.state.batch_capacity.min(issue_cap),
                self.state.physical_capacity.min(issue_cap),
                self.state.equal_sequence_ubatch,
                self.state.max_atomic_sequences,
                self.state.atomic_batch_exclusive,
            )
            .map_err(|error| {
                self.set_snapshot(&format!("scheduler_failed:{error:?}"));
            })?;
        if policy.allocations().is_empty() {
            return Ok(false);
        }
        let mut rows = Vec::new();
        let mut batch_events = Vec::new();
        let mut template = None;
        for allocation in policy.allocations() {
            let request = self
                .state
                .requests
                .get(&allocation.request_id)
                .expect("scheduler allocation references active request");
            if template.is_none() {
                template = Some(request.shared_input());
            }
            batch_events.push(request.shared_input());
            match allocation.phase {
                Phase::Prefill => {
                    for offset in 0..allocation.rows {
                        let index = request.prompt_issued + offset;
                        let position = u32::try_from(index).map_err(|_| ())?;
                        rows.push(LogicalRow {
                            owner: RowOwner {
                                load_generation: self.state.load_generation,
                                incarnation: request.incarnation,
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
                        let speculative = matches!(allocation.phase, Phase::Verify | Phase::Replay);
                        rows.push(LogicalRow {
                            owner: RowOwner {
                                load_generation: self.state.load_generation,
                                incarnation: request.incarnation,
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
                                output: allocation.phase != Phase::Replay,
                                input_token: token,
                                speculative_id: if speculative { ready.speculative_id } else { 0 },
                                speculative_index: if speculative { offset as u32 } else { 0 },
                                speculative_count: if speculative { count } else { 0 },
                                options: request.command.options.clone(),
                            },
                            token,
                        });
                    }
                }
            }
        }
        let logical_rows = rows.len();
        let planned = LogicalBatch(rows);
        let owner_rows: Vec<_> = planned.0.iter().map(|row| &row.owner).collect();
        self.validate_observation_rows(
            &template
                .as_ref()
                .expect("non-empty allocation has a template")
                .template,
            &session_id,
            &owner_rows,
            true,
        )
        .map_err(|detail| self.set_snapshot(&format!("issue_observation_failed:{detail}")))?;
        let candidate_owners = self
            .state
            .stage_owners
            .prepare_rows(
                self.state.load_generation,
                self.state.sequence_capacity,
                &owner_rows,
            )
            .map_err(|_| ())?;
        self.scheduler.validate_prepared(&policy).map_err(|_| ())?;
        let candidate_frontiers = self
            .state
            .stage_frontiers
            .prepare_rows(
                self.state.load_generation,
                self.state.sequence_capacity,
                &owner_rows,
            )
            .map_err(|detail| self.set_snapshot(&format!("issue_frontier_failed:{detail}")))?;
        let logical = match planned.encode() {
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
        let ids = u64::try_from(logical_rows)
            .ok()
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(1))
            .ok_or(())?;
        self.ensure_event_id_obligations(ids, 0).map_err(|_| ())?;
        // ID shortage is a refusal, not an uncertain native issue. Check it
        // before even preparing the issue so a later retry sees identical
        // request, scheduler, flight and native authority.
        if let Err(detail) = self.state.prepare_issue(planned) {
            self.set_snapshot(&format!("issue_prepare_failed:{detail}"));
            return Err(());
        }
        let stage_started = std::time::Instant::now();
        let start_unix_ms = super::observe::unix_ms();
        self.state.begin_native_issue().map_err(|_| ())?;
        #[cfg(test)]
        self.observe_issue_state("before_native_issue");
        let body =
            match self.stage_request(Operation::LogicalBatch, Operation::PhysicalResult, logical) {
                Ok(body) => body,
                Err(detail) => {
                    self.state.mark_issue_uncertain();
                    self.set_snapshot(&format!("logical_batch_failed:{detail}"));
                    self.emit_batch_errors(&batch_events, "LLAMA_LOGICAL_BATCH_FAILED", &detail)?;
                    return Err(());
                }
            };
        let stage_ms = stage_started.elapsed().as_millis() as u64;
        let end_unix_ms = super::observe::unix_ms();
        self.last_stage_done = Some(std::time::Instant::now());
        self.gate_refusals = 0;
        let physical = match CapsuleSet::decode(&body) {
            Ok(physical) => physical,
            Err(error) => {
                self.state.mark_issue_uncertain();
                let detail = format!("physical result decoding failed: {error:?}");
                self.set_snapshot(&format!("physical_result_failed:{error:?}"));
                self.emit_batch_errors(&batch_events, "LLAMA_PHYSICAL_RESULT_INVALID", &detail)?;
                return Err(());
            }
        };
        if single_session(&physical).ok().as_deref() != Some(session_id.as_str())
            || physical.0.iter().any(|capsule| capsule.terminal)
        {
            self.state.mark_issue_uncertain();
            self.set_snapshot("physical_result_identity_failed");
            self.emit_batch_errors(
                &batch_events,
                "LLAMA_PHYSICAL_RESULT_INVALID",
                "physical result identity or stage role is invalid",
            )?;
            return Err(());
        }
        let candidate_frontiers =
            match self
                .state
                .stage_frontiers
                .complete_rows(candidate_frontiers, &physical, false)
            {
                Ok(candidate) => candidate,
                Err(detail) => {
                    self.state.mark_issue_uncertain();
                    self.effects_fenced = true;
                    self.set_snapshot(&format!("physical_frontier_uncertain:{detail}"));
                    self.emit_batch_errors(
                        &batch_events,
                        "LLAMA_PHYSICAL_RESULT_INVALID",
                        &detail,
                    )?;
                    return Err(());
                }
            };
        let ordinal = self
            .state
            .prepared_issue
            .as_ref()
            .expect("native issue remains prepared")
            .ordinal;
        let prepare_telemetry = || -> Result<Vec<super::observe::PreparedTelemetry>, String> {
            let base = &template
                .as_ref()
                .expect("non-empty allocation has a template")
                .template;
            let mut telemetry = self.prepare_batch_observation(
                base,
                &session_id,
                ordinal,
                logical_rows,
                &physical,
                BatchPacing {
                    stage_ms,
                    idle_ms,
                    idle_gated,
                    ready_rows,
                    ready_sequences,
                },
            )?;
            telemetry.extend(self.prepare_stage_span(
                base,
                &session_id,
                &physical,
                ingress_unix_ms,
                start_unix_ms,
                end_unix_ms,
                true,
            )?);
            Ok(telemetry)
        };
        let telemetry = prepare_telemetry().map_err(|detail| {
            self.state.mark_issue_uncertain();
            self.effects_fenced = true;
            self.set_snapshot(&format!("native_observation_uncertain:{detail}"));
        })?;
        if let Err(detail) = self.state.accept_prepared_issue(&physical) {
            self.state.mark_issue_uncertain();
            self.set_snapshot(&format!("physical_issue_uncertain:{detail}"));
            self.emit_batch_errors(&batch_events, "LLAMA_PHYSICAL_RESULT_INVALID", &detail)?;
            return Err(());
        }
        #[cfg(test)]
        self.observe_issue_state("after_issue_accepted");
        // The first stage does not sample. Its append frontier becomes
        // authoritative only after the native split matches the issue.
        self.state
            .stage_frontiers
            .commit(candidate_frontiers)
            .map_err(|detail| {
                self.state.mark_issue_uncertain();
                self.effects_fenced = true;
                self.set_snapshot(&format!("issued_frontier_failed:{detail}"));
            })?;
        self.state.stage_owners = candidate_owners;
        self.scheduler
            .commit_plan(policy)
            .expect("validated policy remained unchanged during synchronous native issue");
        // The accepted flight and the exact bytes to forward are committed
        // together. A telemetry/publisher failure cannot discard the only
        // retained physical result or cause the native call to be repeated.
        let accepted_observations = self.validate_accepted_observations(&telemetry);
        self.effects
            .push_back(super::effects::CommittedEffect::ForwardObserved {
                base: template
                    .as_ref()
                    .expect("non-empty allocation has a template")
                    .template
                    .envelope
                    .clone(),
                target: session.next.expect("validated first session has next"),
                class: EventClass::Data,
                content_type: PHYSICAL_BATCH_CONTENT_TYPE,
                body,
                telemetry,
            });
        // Even an internal post-accept observation mismatch retains the exact
        // physical bytes. It cannot roll back approval or reissue native work.
        accepted_observations.map_err(|detail| {
            self.effects_fenced = true;
            self.set_snapshot(&format!("accepted_observation_failed:{detail}"));
        })?;
        self.flush_effects()
            .map_err(|error| self.set_snapshot(&format!("issued_forward_failed:{error}")))?;
        Ok(true)
    }

    #[cfg(test)]
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
        // The head validates issued identity and commits output intents before
        // publication. A native terminal result alone cannot publish to OUTER.
        self.effects
            .push_back(super::effects::CommittedEffect::Forward {
                base: base.envelope.clone(),
                target: session.first.clone(),
                class: EventClass::Data,
                content_type: TAIL_BATCH_CONTENT_TYPE,
                body,
            });
        self.flush_effects().map_err(|_| ())
    }
}
