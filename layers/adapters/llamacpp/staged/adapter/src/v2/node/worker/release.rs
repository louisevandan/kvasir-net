use super::*;

impl Worker {
    pub(super) fn tail(&mut self, event: Event) -> Result<(), String> {
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
        if session.command.role != NodeRole::First {
            return Err("tail continuation must target the first node".into());
        }
        let mut completed_rows = std::collections::BTreeMap::<String, (Phase, usize)>::new();
        for capsule in &capsules.0 {
            if !capsule.terminal {
                return Err("tail continuation is not terminal".into());
            }
            for owner in &capsule.owners {
                let key = request_key(&owner.session_id, &owner.request_id);
                let entry = completed_rows.entry(key).or_insert((owner.phase, 0));
                if entry.0 != owner.phase {
                    return Err("tail continuation mixes request phases".into());
                }
                entry.1 += 1;
            }
        }
        let mut released = Vec::new();
        let mut settlements = Vec::new();
        let mut resolved_verify_fence = None;
        let mut outcomes_by_key = std::collections::BTreeMap::new();
        for capsule in capsules.0 {
            for outcome in capsule.outcomes {
                let owner = capsule
                    .owners
                    .get(outcome.owner_index as usize)
                    .ok_or_else(|| "tail outcome owner is missing".to_owned())?;
                let key = request_key(&owner.session_id, &owner.request_id);
                if outcomes_by_key
                    .insert(key, (owner.clone(), outcome))
                    .is_some()
                {
                    return Err("tail returned duplicate request decisions".into());
                }
            }
        }
        for (key, (phase, rows)) in completed_rows {
            let outcome = outcomes_by_key.remove(&key);
            let Some(request) = self.state.requests.get_mut(&key) else {
                continue;
            };
            if !request.in_flight {
                return Err("tail completed a request without an in-flight batch".into());
            }
            if phase == Phase::Prefill {
                request.prompt_cursor = request
                    .prompt_cursor
                    .checked_add(rows)
                    .ok_or_else(|| "prompt cursor overflow".to_owned())?;
                if request.prompt_cursor > request.command.tokens.len() {
                    return Err("tail completed more prompt rows than submitted".into());
                }
            } else {
                request.ready = None;
            }
            let Some((owner, outcome)) = outcome else {
                if phase == Phase::Replay {
                    request.ready = match request.after_settlement.take() {
                        Some(super::super::state::SettlementContinuation::Replay(rows)) => {
                            Some(rows)
                        }
                        Some(super::super::state::SettlementContinuation::Proposal { .. }) => {
                            return Err("replay completed with a proposal continuation".into());
                        }
                        None => None,
                    };
                }
                request.in_flight = false;
                continue;
            };
            if request.sequence_id != Some(owner.sequence_id) {
                return Err("tail sequence does not match the admitted request".into());
            }
            request.generated = request
                .generated
                .checked_add(
                    u32::try_from(outcome.generated.len())
                        .map_err(|_| "generated token count overflow".to_owned())?,
                )
                .ok_or_else(|| "generated token count overflow".to_owned())?;
            let stopped = outcome.generated.iter().any(|token| token.stop.is_some());
            if stopped {
                self.state.requests.remove(&key);
                released.push(ReleaseSequence {
                    key,
                    id: owner.sequence_id,
                });
                continue;
            }
            if outcome.retain_from.is_some() && !outcome.replay_tokens.is_empty() {
                if phase != Phase::Verify
                    || !outcome.proposal.is_empty()
                    || owner.speculative_id == 0
                {
                    return Err("checkpoint replay decision is inconsistent".into());
                }
                request.after_settlement =
                    Some(super::super::state::SettlementContinuation::Replay(
                        super::super::state::ReadyRows {
                            phase: Phase::Replay,
                            tokens: outcome.replay_tokens.clone(),
                            position: outcome.replay_position,
                            speculative_id: owner.speculative_id,
                        },
                    ));
                settlements.push(SettlementSequence {
                    key: key.clone(),
                    id: owner.sequence_id,
                    retain_from: outcome
                        .retain_from
                        .expect("checkpoint replay has a settlement boundary"),
                    replay_tokens: outcome.replay_tokens,
                    replay_position: outcome.replay_position,
                    proposal: Vec::new(),
                });
                continue;
            }
            if outcome.proposal.is_empty() && outcome.retain_from.is_none() {
                return Err("continuing tail decision has no proposal".into());
            }
            let position = if let Some(token) = outcome.generated.last() {
                token.position
            } else {
                outcome
                    .replay_position
                    .checked_add(
                        u32::try_from(outcome.replay_tokens.len())
                            .map_err(|_| "replay position overflow".to_owned())?,
                    )
                    .ok_or_else(|| "replay position overflow".to_owned())?
            };
            // The adapter consumes a provider-neutral proposal. A concrete
            // llama.cpp strategy either returns one target token (ordinary
            // Decode) or an atomic multi-token proposal (Verify); the queue
            // layer must not enable or disable a named strategy at runtime.
            if let Some(retain_from) = outcome.retain_from {
                if !outcome.proposal.is_empty() {
                    return Err("settlement decision created a proposal before settlement".into());
                }
                request.after_settlement =
                    Some(super::super::state::SettlementContinuation::Proposal { position });
                settlements.push(SettlementSequence {
                    key: key.clone(),
                    id: owner.sequence_id,
                    retain_from,
                    replay_tokens: outcome.replay_tokens,
                    replay_position: outcome.replay_position,
                    proposal: Vec::new(),
                });
            } else {
                request.ready = Some(super::proposal::ready_from_proposal(
                    &mut self.state.next_speculative_id,
                    outcome.proposal,
                    position,
                )?);
                request.in_flight = false;
                if phase == Phase::Verify {
                    resolved_verify_fence = Some(key.clone());
                }
            }
        }
        if !outcomes_by_key.is_empty() {
            return Err("tail decision has no completed request rows".into());
        }
        if !settlements.is_empty() {
            let mut command = SettlementCommand {
                load_generation: self.state.load_generation,
                session_id: session_id.clone(),
                sequences: settlements,
            };
            self.settle_stage_sequences(&mut command.sequences)?;
            if command
                .sequences
                .iter()
                .any(|sequence| !sequence.proposal.is_empty())
            {
                return Err("first-stage settlement produced a proposal".into());
            }
            self.emit_json(
                &event,
                session.next.clone().expect("validated first session next"),
                EventClass::Control,
                SETTLE_CONTENT_TYPE,
                &command,
            )
            .map_err(|_| "completion queue is full".to_owned())?;
        }
        if let Some(key) = resolved_verify_fence {
            self.state
                .finish_verify_fence(&key)
                .map_err(str::to_owned)?;
        }
        if !released.is_empty() {
            for sequence in &released {
                self.release_stage_sequence(sequence)?;
            }
            self.emit_json(
                &event,
                session.next.expect("validated first session next"),
                EventClass::Control,
                RELEASE_CONTENT_TYPE,
                &ReleaseCommand {
                    load_generation: self.state.load_generation,
                    session_id,
                    sequences: released,
                },
            )
            .map_err(|_| "completion queue is full".to_owned())?;
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
        if session.command.role == NodeRole::First {
            return Err("release cannot re-enter the first node".into());
        }
        for sequence in &command.sequences {
            self.release_stage_sequence(sequence)?;
        }
        if let Some(next) = session.next {
            self.emit_json(
                &event,
                next,
                EventClass::Control,
                RELEASE_CONTENT_TYPE,
                &command,
            )
        } else {
            self.emit_json(
                &event,
                session.first,
                EventClass::Telemetry,
                RELEASED_CONTENT_TYPE,
                &command,
            )
        }
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn released(&mut self, event: Event) -> Result<(), String> {
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
        if session.command.role != NodeRole::First {
            return Err("release completion must target the first node".into());
        }
        for sequence in &command.sequences {
            if sequence.id >= self.state.sequence_capacity
                || self.state.free_sequences.contains(&sequence.id)
                || self
                    .state
                    .requests
                    .values()
                    .any(|request| request.sequence_id == Some(sequence.id))
            {
                return Err("release completion contains a non-owned sequence".into());
            }
            self.state.free_sequences.push_back(sequence.id);
            if self.state.verify_fence_matches(&sequence.key) {
                self.state
                    .finish_verify_fence(&sequence.key)
                    .map_err(str::to_owned)?;
            }
        }
        self.admit_pending()?;
        self.emit_json(
            &event,
            reply_target(&event),
            EventClass::Telemetry,
            RELEASED_CONTENT_TYPE,
            &serde_json::json!({
                "session_id": command.session_id,
                "load_generation": command.load_generation,
                "released": command.sequences.len()
            }),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn admit_pending(&mut self) -> Result<(), String> {
        while let Some(sequence_id) = self.state.free_sequences.pop_front() {
            let Some(key) = self.state.pending.pop_front() else {
                self.state.free_sequences.push_front(sequence_id);
                break;
            };
            let request = self
                .state
                .requests
                .get_mut(&key)
                .ok_or_else(|| "pending request identity is missing".to_owned())?;
            if request.sequence_id.is_some() {
                return Err("pending request already owns a sequence".into());
            }
            request.sequence_id = Some(sequence_id);
        }
        Ok(())
    }

    fn release_stage_sequence(&mut self, sequence: &ReleaseSequence) -> Result<(), String> {
        let mut body = Vec::with_capacity(4 + sequence.key.len());
        body.extend_from_slice(&sequence.id.to_le_bytes());
        body.extend_from_slice(sequence.key.as_bytes());
        self.stage_request(Operation::PhysicalRelease, Operation::PhysicalRelease, body)
            .map(|_| ())
    }
}
