use super::*;

impl Worker {
    pub(super) fn settle(&mut self, event: Event) -> Result<(), String> {
        let mut command: SettlementCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid settlement payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("settlement load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&command.session_id)
            .ok_or_else(|| "settlement session is not configured".to_owned())?
            .clone();
        if session.command.role == NodeRole::First {
            return Err("settlement cannot re-enter the first node".into());
        }
        if command
            .sequences
            .iter()
            .any(|sequence| !sequence.proposal.is_empty())
        {
            return Err("settlement cannot carry a proposal before the terminal node".into());
        }
        self.settle_stage_sequences(&mut command.sequences)?;
        if let Some(next) = session.next {
            if command
                .sequences
                .iter()
                .any(|sequence| !sequence.proposal.is_empty())
            {
                return Err("non-terminal settlement produced a proposal".into());
            }
            self.emit_json(
                &event,
                next,
                EventClass::Control,
                SETTLE_CONTENT_TYPE,
                &command,
            )
        } else {
            self.emit_json(
                &event,
                session.first,
                EventClass::Control,
                SETTLED_CONTENT_TYPE,
                &command,
            )
        }
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn settled(&mut self, event: Event) -> Result<(), String> {
        let command: SettlementCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid settlement completion payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("settlement completion load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&command.session_id)
            .ok_or_else(|| "settlement completion session is not configured".to_owned())?;
        if session.command.role != NodeRole::First {
            return Err("settlement completion must target the first node".into());
        }
        for sequence in &command.sequences {
            let continuation = {
                let request = self
                    .state
                    .requests
                    .get_mut(&sequence.key)
                    .ok_or_else(|| "settled request is no longer active".to_owned())?;
                if request.sequence_id != Some(sequence.id) || !request.in_flight {
                    return Err("settled sequence does not match the in-flight request".into());
                }
                request
                    .after_settlement
                    .take()
                    .ok_or_else(|| "settled request lost its continuation".to_owned())?
            };
            let ready = match continuation {
                super::super::state::SettlementContinuation::Replay(ready) => {
                    if !sequence.proposal.is_empty()
                        || sequence.replay_tokens.is_empty()
                        || ready.phase != Phase::Replay
                        || ready.tokens != sequence.replay_tokens
                        || ready.position != sequence.replay_position
                    {
                        return Err("settlement replay rows changed in flight".into());
                    }
                    ready
                }
                super::super::state::SettlementContinuation::Proposal { position } => {
                    if !sequence.replay_tokens.is_empty() {
                        return Err("direct settlement unexpectedly became replay".into());
                    }
                    super::proposal::ready_from_proposal(
                        &mut self.state.next_speculative_id,
                        sequence.proposal.clone(),
                        position,
                    )?
                }
            };
            {
                let request = self
                    .state
                    .requests
                    .get_mut(&sequence.key)
                    .expect("settled request was validated above");
                request.ready = Some(ready);
                request.in_flight = false;
            }
            if self.state.verify_fence_matches(&sequence.key) {
                self.state
                    .finish_verify_fence(&sequence.key)
                    .map_err(str::to_owned)?;
            }
        }
        Ok(())
    }

    pub(super) fn settle_stage_sequences(
        &mut self,
        sequences: &mut [SettlementSequence],
    ) -> Result<(), String> {
        for sequence in sequences {
            let count = u32::try_from(sequence.replay_tokens.len())
                .map_err(|_| "settlement replay is too large".to_owned())?;
            let mut body = Vec::with_capacity(16 + sequence.replay_tokens.len() * 4);
            body.extend_from_slice(&sequence.id.to_le_bytes());
            body.extend_from_slice(&sequence.retain_from.to_le_bytes());
            body.extend_from_slice(&sequence.replay_position.to_le_bytes());
            body.extend_from_slice(&count.to_le_bytes());
            for token in &sequence.replay_tokens {
                body.extend_from_slice(&token.to_le_bytes());
            }
            let result =
                self.stage_request(Operation::PhysicalSettle, Operation::PhysicalSettle, body)?;
            if result.len() < 4 {
                return Err("physical settlement result is truncated".into());
            }
            let proposal_count = u32::from_le_bytes(result[..4].try_into().unwrap()) as usize;
            let expected = 4usize
                .checked_add(
                    proposal_count
                        .checked_mul(4)
                        .ok_or_else(|| "settlement proposal length overflow".to_owned())?,
                )
                .ok_or_else(|| "settlement proposal length overflow".to_owned())?;
            if result.len() != expected {
                return Err("physical settlement result length is invalid".into());
            }
            sequence.proposal = result[4..]
                .chunks_exact(4)
                .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
                .collect();
        }
        Ok(())
    }
}
