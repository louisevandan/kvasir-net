//! Per-stage execution admission. Transport event IDs never authorize a second
//! native execution. This ledger is distinct from head terminal settlement and
//! from the incarnation/control receipt registry.
use super::*;

impl Worker {
    pub(super) fn physical(
        &mut self,
        event: impl std::borrow::Borrow<Event>,
    ) -> Result<(), String> {
        let event = event.borrow();
        let ingress_unix_ms = observe::unix_ms();
        let input = CapsuleSet::decode(&event.payload)
            .map_err(|error| format!("invalid physical capsule: {error:?}"))?;
        let session_id = single_session(&input)?;
        if input
            .0
            .iter()
            .flat_map(|c| &c.owners)
            .any(|o| o.load_generation != self.state.load_generation)
        {
            return Err("physical batch load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&session_id)
            .ok_or_else(|| "physical batch session is not configured".to_owned())?
            .clone();
        if session.command.role() == NodeRole::First {
            return Err("physical cut-set cannot target the first node".into());
        }
        self.require_stage_source(
            &event,
            session
                .previous
                .as_ref()
                .ok_or("physical batch has no declared predecessor")?,
            "physical batch",
        )?;
        // Complete-event conflict checks precede any owner or native mutation.
        // The execution counter belongs to the head's native Session, not to
        // this receiving load. Its authority comes from the immutable SESSION
        // route, never from the incoming event ID or a claimed payload issuer.
        let plan = self
            .state
            .physical_receives
            .prepare(&session.first, &input)?;
        let fresh_indices = plan.fresh_indices().to_vec();
        let fresh = CapsuleSet(
            input
                .0
                .into_iter()
                .enumerate()
                .filter_map(|(i, c)| fresh_indices.binary_search(&i).is_ok().then_some(c))
                .collect(),
        );
        let (candidate_owners, candidate_frontiers) = if fresh.0.is_empty() {
            (None, None)
        } else {
            let rows = fresh.0.iter().flat_map(|c| &c.owners).collect::<Vec<_>>();
            self.validate_observation_rows(&event, &session_id, &rows, false)?;
            let owners = self.state.stage_owners.prepare_rows(
                self.state.load_generation,
                self.state.sequence_capacity,
                &rows,
            )?;
            let frontiers = self.state.stage_frontiers.prepare_rows(
                self.state.load_generation,
                self.state.sequence_capacity,
                &rows,
            )?;
            (Some(owners), Some(frontiers))
        };
        let request_body = if fresh.0.is_empty() {
            None
        } else {
            let body = fresh
                .encode()
                .map_err(|e| format!("invalid fresh physical input: {e:?}"))?;
            u32::try_from(body.len()).map_err(|_| "fresh physical frame is too large")?;
            Some(body)
        };
        let ids = fresh
            .0
            .iter()
            .try_fold(1u64 + u64::from(self.service_budget.enabled() && !fresh.0.is_empty()), |n, capsule| {
                n.checked_add(capsule.owners.len() as u64)
            })
            .ok_or("physical notification obligation overflow")?;
        self.ensure_event_id_obligations(ids, 0)?;
        let attempt = self.state.physical_receives.begin(plan)?;
        let start_unix_ms = observe::unix_ms();
        let mut rpc_us = 0;
        let fresh_result = if let Some(body) = request_body {
            let rpc_started = Instant::now();
            let response = self.stage_request(Operation::PhysicalBatch, Operation::PhysicalResult, body);
            rpc_us = rpc_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            let response = response.and_then(|body| {
                    CapsuleSet::decode(&body).map_err(|e| format!("invalid physical result: {e:?}"))
                });
            match response {
                Ok(result) => result,
                Err(error) => {
                    // A prefix may have executed. Neither a missing response
                    // nor a success opcode with malformed bytes is rollback.
                    self.effects_fenced = true;
                    self.state.physical_receives.mark_uncertain(attempt)?;
                    return Err(error);
                }
            }
        } else {
            CapsuleSet(Vec::new())
        };
        let end_unix_ms = observe::unix_ms();
        // A success opcode is not proof of a valid state transition. Validate
        // sampled decisions before committing any owner, frontier or receipt.
        for outcome in fresh_result.0.iter().flat_map(|capsule| &capsule.outcomes) {
            if let Err(error) = super::super::frontier::validate_continuation_width(
                &outcome.proposal,
                &outcome.replay_tokens,
                self.state.physical_capacity,
            ) {
                // The engine already ran the whole Fresh subset. A good
                // prefix is not safe to commit when another result is invalid.
                self.effects_fenced = true;
                self.state.physical_receives.mark_uncertain(attempt)?;
                return Err(error);
            }
        }
        let candidate_frontiers = match candidate_frontiers {
            Some(candidate) => match self.state.stage_frontiers.complete_rows(
                candidate,
                &fresh_result,
                session.command.role() == NodeRole::Last,
            ) {
                Ok(candidate) => Some(candidate),
                Err(error) => {
                    self.effects_fenced = true;
                    self.state.physical_receives.mark_uncertain(attempt)?;
                    return Err(error);
                }
            },
            None => None,
        };
        // No replayed receipt is fresh compute. Prepare every Fresh recipient
        // before cache/frontier commit; owner equality is checked by complete.
        let telemetry = match self.prepare_stage_span(
            &event,
            &session_id,
            &fresh_result,
            ingress_unix_ms,
            start_unix_ms,
            end_unix_ms,
            false,
        ) {
            Ok(telemetry) => telemetry,
            Err(error) => {
                self.effects_fenced = true;
                self.state.physical_receives.mark_uncertain(attempt)?;
                return Err(error);
            }
        };
        let feedback = self.prepare_service_feedback(event, &session, &fresh_result, rpc_us).map_err(|error| {
            self.effects_fenced = true;
            error
        })?;
        let result = self
            .state
            .physical_receives
            .complete(
                attempt,
                &fresh_result,
                session.command.role() == NodeRole::Last,
            )
            .map_err(|error| {
                self.effects_fenced = true;
                error
            })?;
        if let Some(owners) = candidate_owners {
            self.state.stage_owners = owners;
        }
        if let Some(frontiers) = candidate_frontiers {
            self.state
                .stage_frontiers
                .commit(frontiers)
                .map_err(|error| {
                    self.effects_fenced = true;
                    error
                })?;
        }
        let body = result.encode().map_err(|e| {
            self.effects_fenced = true;
            format!("physical receipt encoding failed: {e:?}")
        })?;
        let (target, content_type) = match session.command.role() {
            NodeRole::Middle => (
                session.next.expect("validated middle next"),
                PHYSICAL_BATCH_CONTENT_TYPE,
            ),
            NodeRole::Last => (session.first, TAIL_BATCH_CONTENT_TYPE),
            NodeRole::First => unreachable!(),
        };
        self.effects
            .push_back(effects::CommittedEffect::ForwardObserved {
                base: event.envelope.clone(),
                target,
                class: EventClass::Data,
                content_type,
                body,
                telemetry,
            });
        if let Some(feedback) = feedback { self.effects.push_back(feedback); }
        self.flush_effects()
    }
}
