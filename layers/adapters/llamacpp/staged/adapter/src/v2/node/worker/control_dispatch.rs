//! Head-only authority for locally applied and accepted control effects.
//! Tickets are prepared immediately before a synchronous effect and consumed
//! without yielding after it. They are not reservations valid across turns.
use super::super::state::{ControlDispatch, ControlDispatchPhase};
use super::*;

enum HeadControlMembers {
    Release(Vec<String>),
    Settle(Vec<String>),
}

// A native-success callback cannot accidentally accept a forward ticket (or
// vice versa). These private constructors also keep unvalidated keys out.
pub(super) struct HeadLocalTicket(HeadControlMembers);
pub(super) struct HeadForwardTicket(HeadControlMembers);

impl Worker {
    fn head_control_scope(
        &self,
        generation: u64,
        session_id: &str,
    ) -> Result<&PipelineSession, String> {
        let session = self
            .state
            .sessions
            .get(session_id)
            .ok_or("head control session is missing")?;
        if generation == 0
            || generation != self.state.load_generation
            || session.command.load_generation != generation
            || session.command.role() != NodeRole::First
            || session.first != self.endpoint
        {
            return Err("head control scope is stale or not the first stage".into());
        }
        Ok(session)
    }

    fn pending_release_dispatch(
        &self,
        generation: u64,
        session: &str,
        sequence: &ReleaseSequence,
    ) -> Result<&ControlDispatch, String> {
        self.head_control_scope(generation, session)?;
        let pending = self
            .state
            .pending_releases
            .get(&sequence.key)
            .ok_or("head release no longer has pending authority")?;
        if pending.sequence != *sequence
            || pending.dispatch.load_generation != generation
            || pending.dispatch.session_id != session
            || sequence
                .key
                .split_once('\0')
                .is_none_or(|(owner, request)| {
                    owner != session || request.is_empty() || request.contains('\0')
                })
        {
            return Err("head release differs from pending authority".into());
        }
        Ok(&pending.dispatch)
    }

    fn pending_settle_dispatch(
        &self,
        generation: u64,
        session: &str,
        sequence: &SettlementSequence,
    ) -> Result<&ControlDispatch, String> {
        self.head_control_scope(generation, session)?;
        let pending = self
            .state
            .pending_settlements
            .get(&sequence.key)
            .ok_or("head settlement no longer has pending authority")?;
        // This is the original outgoing command, before the terminal appends
        // a proposal. ACK validation keeps that later proposal independent.
        if pending.sequence != *sequence
            || !sequence.proposal.is_empty()
            || pending.dispatch.load_generation != generation
            || pending.dispatch.session_id != session
            || sequence
                .key
                .split_once('\0')
                .is_none_or(|(owner, request)| {
                    owner != session || request.is_empty() || request.contains('\0')
                })
        {
            return Err("head settlement differs from pending authority".into());
        }
        Ok(&pending.dispatch)
    }

    pub(super) fn prepare_head_release(
        &self,
        generation: u64,
        session: &str,
        sequence: &ReleaseSequence,
    ) -> Result<HeadLocalTicket, String> {
        self.pending_release_dispatch(generation, session, sequence)?;
        Ok(HeadLocalTicket(HeadControlMembers::Release(vec![
            sequence.key.clone(),
        ])))
    }

    pub(super) fn prepare_head_settle(
        &self,
        generation: u64,
        session: &str,
        sequence: &SettlementSequence,
    ) -> Result<HeadLocalTicket, String> {
        self.pending_settle_dispatch(generation, session, sequence)?;
        Ok(HeadLocalTicket(HeadControlMembers::Settle(vec![
            sequence.key.clone(),
        ])))
    }

    pub(super) fn prepare_head_control_forward(
        &self,
        target: &Endpoint,
        content_type: &str,
        body: &[u8],
    ) -> Result<HeadForwardTicket, String> {
        let mut ids = std::collections::BTreeSet::new();
        let mut keys = std::collections::BTreeSet::new();
        match content_type {
            RELEASE_CONTENT_TYPE => {
                let command: ReleaseCommand = serde_json::from_slice(body)
                    .map_err(|e| format!("invalid head release: {e}"))?;
                command.validate().map_err(str::to_owned)?;
                let session =
                    self.head_control_scope(command.load_generation, &command.session_id)?;
                if session.next.as_ref() != Some(target) {
                    return Err("head control target is not the declared next stage".into());
                }
                for sequence in &command.sequences {
                    let dispatch = self.pending_release_dispatch(
                        command.load_generation,
                        &command.session_id,
                        sequence,
                    )?;
                    if !ids.insert(sequence.id)
                        || !keys.insert(sequence.key.clone())
                        || dispatch.phase == ControlDispatchPhase::Queued
                    {
                        return Err(
                            "head control was not locally applied or repeats a member".into()
                        );
                    }
                }
                Ok(HeadForwardTicket(HeadControlMembers::Release(
                    keys.into_iter().collect(),
                )))
            }
            SETTLE_CONTENT_TYPE => {
                let command: SettlementCommand = serde_json::from_slice(body)
                    .map_err(|e| format!("invalid head settlement: {e}"))?;
                command.validate().map_err(str::to_owned)?;
                let session =
                    self.head_control_scope(command.load_generation, &command.session_id)?;
                if session.next.as_ref() != Some(target) {
                    return Err("head control target is not the declared next stage".into());
                }
                for sequence in &command.sequences {
                    let dispatch = self.pending_settle_dispatch(
                        command.load_generation,
                        &command.session_id,
                        sequence,
                    )?;
                    if !ids.insert(sequence.id)
                        || !keys.insert(sequence.key.clone())
                        || dispatch.phase == ControlDispatchPhase::Queued
                    {
                        return Err(
                            "head control was not locally applied or repeats a member".into()
                        );
                    }
                }
                Ok(HeadForwardTicket(HeadControlMembers::Settle(
                    keys.into_iter().collect(),
                )))
            }
            _ => Err("unsupported head control forward kind".into()),
        }
    }

    /// All validation/allocations precede the effect. The sole worker mutator
    /// does not yield between effect success and this infallible commit.
    pub(super) fn complete_head_local(&mut self, ticket: HeadLocalTicket) {
        self.complete_head_members(ticket.0, false);
    }

    pub(super) fn complete_head_forward(&mut self, ticket: HeadForwardTicket) {
        self.complete_head_members(ticket.0, true);
    }

    fn complete_head_members(&mut self, ticket: HeadControlMembers, forwarded: bool) {
        let update = |dispatch: &mut ControlDispatch| {
            if forwarded {
                dispatch.phase = ControlDispatchPhase::ForwardAccepted;
            } else if dispatch.phase == ControlDispatchPhase::Queued {
                // Cached native replay must never regress a forwarded control.
                dispatch.phase = ControlDispatchPhase::LocalApplied;
            }
        };
        match ticket {
            HeadControlMembers::Release(keys) => {
                for key in keys {
                    update(
                        &mut self
                            .state
                            .pending_releases
                            .get_mut(&key)
                            .expect("validated release remains pending during synchronous effect")
                            .dispatch,
                    );
                }
            }
            HeadControlMembers::Settle(keys) => {
                for key in keys {
                    update(
                        &mut self
                            .state
                            .pending_settlements
                            .get_mut(&key)
                            .expect(
                                "validated settlement remains pending during synchronous effect",
                            )
                            .dispatch,
                    );
                }
            }
        }
    }
}
