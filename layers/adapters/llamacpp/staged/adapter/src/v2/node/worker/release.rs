use super::*;

impl Worker {
    pub(super) fn tail(&mut self, event: Event) -> Result<(), String> {
        let capsules = CapsuleSet::decode(&event.payload)
            .map_err(|error| format!("invalid tail capsule: {error:?}"))?;
        let session_id = single_session(&capsules)?;
        let session = self
            .state
            .sessions
            .get(&session_id)
            .ok_or_else(|| "tail session is not configured".to_owned())?
            .clone();
        if session.command.role != NodeRole::First {
            return Err("tail continuation must target the first node".into());
        }
        let mut released = Vec::new();
        for capsule in capsules.0 {
            if !capsule.terminal {
                return Err("tail continuation is not terminal".into());
            }
            for outcome in capsule.outcomes {
                let owner = capsule
                    .owners
                    .get(outcome.owner_index as usize)
                    .ok_or_else(|| "tail outcome owner is missing".to_owned())?;
                let key = request_key(&owner.session_id, &owner.request_id);
                let Some(request) = self.state.requests.get_mut(&key) else {
                    continue;
                };
                if request.sequence_id != Some(owner.sequence_id) {
                    return Err("tail sequence does not match the admitted request".into());
                }
                request.generated += 1;
                if outcome.stop.is_some() {
                    self.state.requests.remove(&key);
                    released.push(ReleaseSequence {
                        key,
                        id: owner.sequence_id,
                    });
                } else {
                    request.decode = Some((outcome.token, outcome.position));
                }
            }
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
        }
        self.admit_pending()?;
        self.emit_json(
            &event,
            reply_target(&event),
            EventClass::Telemetry,
            RELEASED_CONTENT_TYPE,
            &serde_json::json!({
                "session_id": command.session_id,
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
