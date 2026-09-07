use super::*;
use p4_protocol::event::Envelope;

// Prepared and committed without yielding on the sole worker mutator. This is
// an ACK candidate, not a ticket that may survive another handler or load.
struct PreparedReleaseAck {
    sequences: Vec<ReleaseSequence>,
    admission_count: usize,
    notifications: Vec<super::effects::CommittedEffect>,
}

impl Worker {
    pub(super) fn tail(&mut self, event: Event) -> Result<(), String> {
        self.tail_without_flush(event)?;
        self.flush_effects()
    }

    // Whole-return authority and intent commit. Emission is a separate
    // consumer, also allowing late delivery faults to be tested after commit.
    pub(super) fn tail_without_flush(&mut self, event: Event) -> Result<(), String> {
        if self.effects_fenced {
            return Err("committed effects are fenced".into());
        }
        let capsules = CapsuleSet::decode(&event.payload)
            .map_err(|error| format!("invalid tail capsule: {error:?}"))?;
        let session_id = single_session(&capsules)?;
        if capsules
            .0
            .iter()
            .flat_map(|capsule| &capsule.owners)
            .any(|owner| owner.load_generation != self.state.load_generation)
        {
            return Err("tail load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&session_id)
            .ok_or_else(|| "tail session is not configured".to_owned())?
            .clone();
        if session.command.role() != NodeRole::First {
            return Err("tail continuation must target the first node".into());
        }
        self.require_stage_source(&event, &session.last, "tail batch")?;
        #[cfg(test)]
        self.observe_issue_state("before_tail_settlement");
        self.validate_flight_counts(None, &std::collections::BTreeMap::new())?;
        let mut plan = self.state.flights.prepare_return(&capsules)?;
        for decision in &plan.decisions {
            super::outcome::validate_decision(&decision.owners, &decision.outcome)?;
            if let Some((_, outcome)) = &decision.outcome {
                super::super::frontier::validate_continuation_width(
                    &outcome.proposal,
                    &outcome.replay_tokens,
                    self.state.physical_capacity,
                )?;
            }
        }
        // Only affected requests are cloned. The ledger owns issued identity;
        // this candidate transaction owns request meaning and effect intents.
        let mut requests = std::collections::BTreeMap::<String, Option<RequestState>>::new();
        let mut next_speculative_id = self.state.next_speculative_id;
        let mut next_control_operation = self.state.next_control_operation;
        let mut outputs = Vec::new();
        let mut released = Vec::new();
        let mut pending_releases = Vec::new();
        let mut settlements = Vec::new();
        let mut resolved = std::collections::BTreeSet::new();
        for fragment in std::mem::take(&mut plan.fragments) {
            if !requests.contains_key(&fragment.key) {
                let request = self
                    .state
                    .requests
                    .get(&fragment.key)
                    .ok_or_else(|| "tail request is no longer active".to_owned())?
                    .clone();
                requests.insert(fragment.key.clone(), Some(request));
            }
            let candidate = requests.get_mut(&fragment.key).expect("candidate inserted");
            let request = candidate
                .as_mut()
                .ok_or_else(|| "tail returned a fragment after the request stopped".to_owned())?;
            let effect =
                super::outcome::apply_fragment(request, &fragment, &mut next_speculative_id)?;
            if let Some(mut settlement) = effect.settlement {
                settlement.operation_id = next_control_operation;
                next_control_operation = next_control_operation
                    .checked_add(1)
                    .filter(|_| settlement.operation_id != 0)
                    .ok_or("control operation exhausted")?;
                settlements.push(settlement);
            }
            if effect.resolve_verify
                && (!self.state.verify_fence_matches(&fragment.key)
                    || !resolved.insert(fragment.key.clone()))
            {
                return Err("tail resolved an unowned verification fence".into());
            }
            if effect.stopped {
                if self.state.pending_releases.contains_key(&fragment.key) {
                    return Err("stopped request already has a pending release".into());
                }
                let sequence = ReleaseSequence {
                    incarnation: request.incarnation,
                    operation_id: next_control_operation,
                    key: fragment.key.clone(),
                    id: request
                        .sequence_id
                        .ok_or_else(|| "stopped request lost its sequence".to_owned())?,
                };
                let reply: ReplySpec = serde_json::from_str(&request.reply)
                    .map_err(|_| "release reply contract is invalid")?;
                pending_releases.push(super::super::state::PendingRelease {
                    sequence: sequence.clone(),
                    original: request.template.envelope.clone(),
                    reply,
                    dispatch: super::super::state::ControlDispatch::queued(
                        self.state.load_generation,
                        session_id.clone(),
                    ),
                });
                released.push(sequence);
                next_control_operation = next_control_operation
                    .checked_add(1)
                    .filter(|_| next_control_operation != 0)
                    .ok_or("control operation exhausted")?;
            }
            let operation = effect
                .stopped
                .then(|| released.last().expect("release assigned").operation_id);
            let proof = if effect.stopped {
                let witness = request
                    .issued_work
                    .ok_or("terminal request has no accepted issued-work witness")?;
                let authority = request.issue_authority()?;
                let seed = crate::v2::IssueWitness::new(&authority).map_err(str::to_owned)?;
                if witness.authority_digest() != seed.authority_digest() {
                    return Err(
                        "terminal issued-work authority differs from original submission".into(),
                    );
                }
                let proof = witness.proof();
                proof.validate().map_err(str::to_owned)?;
                Some(proof)
            } else {
                None
            };
            outputs.extend(effect.outputs.into_iter().map(|(owner, token)| {
                let release_operation = token.stop.as_ref().and(operation);
                let issued_work = token.stop.as_ref().and(proof);
                (
                    owner,
                    token,
                    request.template.envelope.event_id.clone(),
                    release_operation,
                    issued_work,
                )
            }));
            if effect.stopped {
                *candidate = None;
            }
        }
        let pending_settlements = settlements.clone();
        let mut effects = Self::prepare_outputs(&event, outputs)?;
        if !settlements.is_empty() || !released.is_empty() {
            let next = session
                .next
                .clone()
                .ok_or_else(|| "first session has no next stage".to_owned())?;
            if !settlements.is_empty() {
                let command = SettlementCommand {
                    load_generation: self.state.load_generation,
                    session_id: session_id.clone(),
                    sequences: settlements.clone(),
                };
                command.validate().map_err(str::to_owned)?;
                let body = serde_json::to_vec(&command).map_err(|error| error.to_string())?;
                effects.extend(settlements.into_iter().map(|sequence| {
                    super::effects::CommittedEffect::Settle {
                        load_generation: self.state.load_generation,
                        session_id: session_id.clone(),
                        sequence,
                    }
                }));
                effects.push_back(super::effects::CommittedEffect::ForwardHeadControl {
                    base: event.envelope.clone(),
                    target: next.clone(),
                    class: EventClass::Control,
                    content_type: SETTLE_CONTENT_TYPE,
                    body,
                });
            }
            if !released.is_empty() {
                let command = ReleaseCommand {
                    load_generation: self.state.load_generation,
                    session_id: session_id.clone(),
                    sequences: released.clone(),
                };
                command.validate().map_err(str::to_owned)?;
                let body = serde_json::to_vec(&command).map_err(|error| error.to_string())?;
                effects.extend(released.into_iter().map(|sequence| {
                    super::effects::CommittedEffect::Release {
                        load_generation: self.state.load_generation,
                        session_id: session_id.clone(),
                        sequence,
                    }
                }));
                effects.push_back(super::effects::CommittedEffect::ForwardHeadControl {
                    base: event.envelope.clone(),
                    target: next,
                    class: EventClass::Control,
                    content_type: RELEASE_CONTENT_TYPE,
                    body,
                });
            }
        }
        self.validate_flight_counts(Some(&plan), &requests)?;
        let future =
            u64::try_from(pending_releases.len()).map_err(|_| "receipt obligation overflow")?;
        let new_ids = super::obligations::effect_event_count(&effects)?
            .checked_add(future)
            .ok_or("receipt obligation overflow")?;
        self.ensure_event_id_obligations(new_ids, 0)?;
        // No fallible validation or external effect remains inside this commit.
        for (key, candidate) in requests {
            if let Some(request) = candidate {
                self.state.requests.insert(key, request);
            } else {
                self.state.requests.remove(&key);
            }
        }
        self.state.next_speculative_id = next_speculative_id;
        self.state.next_control_operation = next_control_operation;
        for settlement in pending_settlements {
            self.state.pending_settlements.insert(
                settlement.key.clone(),
                super::super::state::PendingSettlement {
                    sequence: settlement,
                    dispatch: super::super::state::ControlDispatch::queued(
                        self.state.load_generation,
                        session_id.clone(),
                    ),
                },
            );
        }
        for release in pending_releases {
            self.state
                .pending_releases
                .insert(release.sequence.key.clone(), release);
        }
        for key in resolved {
            self.state
                .finish_verify_fence(&key)
                .expect("fence validated before commit");
        }
        self.state.commit_flight_return(plan);
        self.effects.extend(effects);
        Ok(())
    }

    fn validate_flight_counts(
        &self,
        plan: Option<&super::super::flight::ReturnPlan>,
        candidates: &std::collections::BTreeMap<String, Option<RequestState>>,
    ) -> Result<(), String> {
        let mut counts = self.state.flights.outstanding(plan);
        for (key, current) in &self.state.requests {
            let request = candidates.get(key).map_or(Some(current), Option::as_ref);
            let expected = counts.remove(key).unwrap_or(0);
            if request.map_or(0, |request| request.outstanding) != expected {
                return Err("request outstanding differs from issued fragment identities".into());
            }
        }
        if !counts.is_empty() {
            return Err("issued fragment has no resident request".into());
        }
        Ok(())
    }

    pub(super) fn release(&mut self, event: Event) -> Result<(), String> {
        let command: ReleaseCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid release payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("release load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&command.session_id)
            .ok_or_else(|| "release session is not configured".to_owned())?
            .clone();
        if session.command.role() == NodeRole::First {
            return Err("release cannot re-enter the first node".into());
        }
        self.require_stage_source(
            &event,
            session
                .previous
                .as_ref()
                .ok_or("release has no declared predecessor")?,
            "release",
        )?;
        let mut ids = std::collections::BTreeSet::new();
        let mut controls = Vec::new();
        for sequence in &command.sequences {
            if !ids.insert(sequence.id) {
                return Err("release repeats a slot".into());
            }
            let identity =
                self.operation_identity(&sequence.key, sequence.id, sequence.incarnation)?;
            let body = crate::v2::control_identity::release(
                command.load_generation,
                &command.session_id,
                sequence,
            )?;
            controls.push((identity, sequence.operation_id, body));
        }
        self.state.stage_owners.validate_control_batch(&controls)?;
        for (identity, operation_id, body) in &controls {
            if matches!(
                self.state
                    .stage_owners
                    .check_control(identity, *operation_id, body)?,
                super::super::ownership::ControlCheck::New
            ) {
                self.state.stage_frontiers.prepare_release(identity)?;
            }
        }
        self.ensure_event_id_obligations(1, 0)?;
        for sequence in &command.sequences {
            self.release_stage_sequence(sequence)?;
        }
        let (target, class, content_type) = if let Some(next) = session.next {
            (next, EventClass::Control, RELEASE_CONTENT_TYPE)
        } else {
            (session.first, EventClass::Telemetry, RELEASED_CONTENT_TYPE)
        };
        self.effects
            .push_back(super::effects::CommittedEffect::Forward {
                base: event.envelope,
                target,
                class,
                content_type,
                body: serde_json::to_vec(&command).map_err(|error| error.to_string())?,
            });
        self.flush_effects()
    }

    pub(super) fn released(&mut self, event: Event) -> Result<(), String> {
        self.released_without_flush(event)?;
        self.flush_effects()
    }

    /// Consume the complete, validated ACK and retain every receipt intent.
    /// Publication and any newly runnable native work belong to the caller's
    /// effect/issue gates, never to this settlement-only entry point.
    pub(super) fn released_without_flush(&mut self, event: Event) -> Result<(), String> {
        let prepared = self.prepare_release_ack(event)?;
        self.commit_release_ack(prepared);
        Ok(())
    }

    fn prepare_release_ack(&self, event: Event) -> Result<PreparedReleaseAck, String> {
        let command: ReleaseCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid release completion payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("release completion load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&command.session_id)
            .ok_or_else(|| "release completion session is not configured".to_owned())?;
        if session.command.role() != NodeRole::First {
            return Err("release completion must target the first node".into());
        }
        self.require_stage_source(&event, &session.last, "release completion")?;
        let mut seen_keys = std::collections::BTreeSet::new();
        let mut seen_ids = std::collections::BTreeSet::new();
        for sequence in &command.sequences {
            if sequence.id >= self.state.sequence_capacity
                || !seen_keys.insert(sequence.key.clone())
                || !seen_ids.insert(sequence.id)
                || self
                    .state
                    .pending_releases
                    .get(&sequence.key)
                    .is_none_or(|expected| {
                        &expected.sequence != sequence
                            || !expected
                                .dispatch
                                .allows_ack(command.load_generation, &command.session_id)
                    })
                || !sequence
                    .key
                    .starts_with(&format!("{}\0", command.session_id))
                || self.state.free_sequences.contains(&sequence.id)
                || self
                    .state
                    .requests
                    .values()
                    .any(|request| request.sequence_id == Some(sequence.id))
            {
                return Err("release completion contains a non-owned sequence".into());
            }
        }
        // Admission is another consumer of the returned slots. Validate its
        // complete candidate before releasing any owner/fence from this event.
        let candidate_slots: Vec<_> = self
            .state
            .free_sequences
            .iter()
            .copied()
            .chain(command.sequences.iter().map(|sequence| sequence.id))
            .collect();
        self.validate_admission(&candidate_slots)?;
        let admission_count = candidate_slots.len().min(self.state.pending.len());
        // Build every owner notification before committing any slot/admission.
        // ACK routing is not the original request's OUTER authority.
        let mut groups = std::collections::BTreeMap::<
            String,
            (Envelope, ReplySpec, Address, ReleaseReceipt),
        >::new();
        for sequence in &command.sequences {
            let pending = &self.state.pending_releases[&sequence.key];
            let reply = &pending.reply;
            if reply.channel.is_empty()
                || reply.correlation_id.is_empty()
                || reply.connection_generation == 0
            {
                return Err("release reply contract is incomplete".into());
            }
            let ingress = Address::from_str(&reply.ingress_agent)
                .map_err(|_| "release reply ingress is invalid")?;
            let owner = Endpoint::outer(
                ingress.clone(),
                reply.channel.clone(),
                reply.connection_generation,
            );
            let Endpoint::Outer(owner_route) = &owner else {
                unreachable!()
            };
            if pending.original.source != owner
                || pending.original.return_route.as_ref() != Some(owner_route)
                || pending.original.target != self.endpoint
                || pending.original.correlation_id != reply.correlation_id
                || pending.original.deadline_unix_ms != reply.deadline_unix_ms
            {
                return Err("release reply differs from its original submission provenance".into());
            }
            let route = serde_json::to_string(reply).map_err(|error| error.to_string())?;
            let request_id = sequence
                .key
                .strip_prefix(&format!("{}\0", command.session_id))
                .filter(|request| !request.is_empty() && !request.contains('\0'))
                .ok_or("release completion request key is not canonical")?;
            let entry = groups.entry(route).or_insert_with(|| {
                (
                    pending.original.clone(),
                    reply.clone(),
                    ingress,
                    ReleaseReceipt {
                        load_generation: command.load_generation,
                        session_id: command.session_id.clone(),
                        members: Vec::new(),
                    },
                )
            });
            entry.3.members.push(ReleaseMember {
                request_id: request_id.into(),
                submission_event_id: pending.original.event_id.clone(),
                sequence_id: sequence.id,
                incarnation: sequence.incarnation,
                operation_id: sequence.operation_id,
            });
        }
        let mut notifications = Vec::with_capacity(groups.len());
        for (_, (base, reply, ingress, payload)) in groups {
            payload.validate().map_err(str::to_owned)?;
            // Serialization can fail before commit, never after consuming ACK.
            serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
            notifications.push(super::effects::CommittedEffect::ReleaseReceipt {
                base,
                reply,
                ingress,
                payload,
            });
        }
        self.ensure_event_id_obligations(
            u64::try_from(notifications.len())
                .map_err(|_| "release notification count overflow")?,
            u64::try_from(command.sequences.len()).map_err(|_| "release member count overflow")?,
        )?;
        Ok(PreparedReleaseAck {
            sequences: command.sequences,
            admission_count,
            notifications,
        })
    }

    fn commit_release_ack(&mut self, prepared: PreparedReleaseAck) {
        // No handler, native operation, publication or fallible interpretation
        // separates the complete preparation from this commit. The free-slot
        // order is the candidate_slots order validated during preparation.
        for sequence in &prepared.sequences {
            self.state.pending_releases.remove(&sequence.key);
            self.state.free_sequences.push_back(sequence.id);
            if self.state.verify_fence_matches(&sequence.key) {
                self.state
                    .finish_verify_fence(&sequence.key)
                    .expect("validated release fence remains present during ACK commit");
            }
        }
        self.commit_admission(prepared.admission_count);
        self.effects.extend(prepared.notifications);
        #[cfg(test)]
        self.observe_issue_state("after_release_committed");
    }

    pub(super) fn admit_pending(&mut self) -> Result<(), String> {
        let slots: Vec<_> = self.state.free_sequences.iter().copied().collect();
        self.validate_admission(&slots)?;
        self.commit_admission(slots.len().min(self.state.pending.len()));
        Ok(())
    }

    fn commit_admission(&mut self, count: usize) {
        for _ in 0..count {
            let sequence_id = self
                .state
                .free_sequences
                .pop_front()
                .expect("admission slot validated");
            let key = self
                .state
                .pending
                .pop_front()
                .expect("admission request validated");
            let request = self
                .state
                .requests
                .get_mut(&key)
                .expect("admission request validated");
            request.sequence_id = Some(sequence_id);
        }
    }

    fn validate_admission(&self, slots: &[u32]) -> Result<(), String> {
        let mut keys = std::collections::BTreeSet::new();
        let mut ids = std::collections::BTreeSet::new();
        for (id, key) in slots.iter().zip(&self.state.pending) {
            if *id >= self.state.sequence_capacity
                || !ids.insert(*id)
                || !keys.insert(key)
                || self
                    .state
                    .requests
                    .values()
                    .any(|request| request.sequence_id == Some(*id))
            {
                return Err("admission slot or pending request is duplicated".into());
            }
            let request = self
                .state
                .requests
                .get(key)
                .ok_or("pending request identity is missing")?;
            if request.sequence_id.is_some() {
                return Err("pending request already owns a sequence".into());
            }
        }
        Ok(())
    }

    pub(super) fn release_stage_sequence(
        &mut self,
        sequence: &ReleaseSequence,
    ) -> Result<(), String> {
        let identity = self.operation_identity(&sequence.key, sequence.id, sequence.incarnation)?;
        let body = crate::v2::control_identity::release(
            self.state.load_generation,
            &identity.session_id,
            sequence,
        )?;
        let check =
            self.state
                .stage_owners
                .check_control(&identity, sequence.operation_id, &body)?;
        let frontier = if matches!(check, super::super::ownership::ControlCheck::New) {
            Some(self.state.stage_frontiers.prepare_release(&identity)?)
        } else {
            None
        };
        let response = match check {
            super::super::ownership::ControlCheck::Replay(response) => response,
            super::super::ownership::ControlCheck::New => self.stage_request(
                Operation::PhysicalRelease,
                Operation::PhysicalRelease,
                body.clone(),
            )?,
        };
        if response != body {
            self.effects_fenced = true;
            return Err("physical release acknowledgement is invalid".into());
        }
        self.state
            .stage_owners
            .commit_control(&identity, sequence.operation_id, &body, &response, true)
            .map_err(|error| {
                self.effects_fenced = true;
                error
            })?;
        if let Some(frontier) = frontier {
            self.state
                .stage_frontiers
                .commit(frontier)
                .map_err(|error| {
                    self.effects_fenced = true;
                    error
                })?;
        }
        Ok(())
    }

    pub(super) fn operation_identity(
        &self,
        key: &str,
        id: u32,
        incarnation: u64,
    ) -> Result<super::super::ownership::Identity, String> {
        let (session, request) = key.split_once('\0').ok_or("control key lacks session")?;
        if session.is_empty()
            || request.is_empty()
            || request.contains('\0')
            || !self.state.sessions.contains_key(session)
        {
            return Err("control key has no configured session".into());
        }
        Ok(super::super::ownership::Identity {
            load_generation: self.state.load_generation,
            session_id: session.into(),
            sequence_key: key.into(),
            sequence_id: id,
            incarnation,
        })
    }
}
