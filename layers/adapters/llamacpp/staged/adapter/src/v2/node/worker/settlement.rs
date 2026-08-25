use super::*;

impl Worker {
    pub(super) fn settle(&mut self, event: Event) -> Result<(), String> {
        let command: SettlementCommand = serde_json::from_slice(&event.payload)
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
        self.settle_stage_sequences(&command.sequences)?;
        if let Some(next) = session.next {
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
            {
                let request = self
                    .state
                    .requests
                    .get_mut(&sequence.key)
                    .ok_or_else(|| "settled request is no longer active".to_owned())?;
                if request.sequence_id != Some(sequence.id) || !request.in_flight {
                    return Err("settled sequence does not match the in-flight request".into());
                }
                let ready = request
                    .after_settlement
                    .take()
                    .ok_or_else(|| "settled request lost its continuation".to_owned())?;
                if sequence.replay_tokens.is_empty() {
                    if ready.phase == Phase::Replay {
                        return Err("settlement unexpectedly lost replay rows".into());
                    }
                } else if ready.phase != Phase::Replay
                    || ready.tokens != sequence.replay_tokens
                    || ready.position != sequence.replay_position
                {
                    return Err("settlement replay rows changed in flight".into());
                }
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
        sequences: &[SettlementSequence],
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
            self.stage_request(Operation::PhysicalSettle, Operation::PhysicalSettle, body)?;
        }
        Ok(())
    }
}
