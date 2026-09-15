//! Normalizes a completed logical fragment without native calls or publication.
//!
//! The caller owns an event-wide candidate and discards it on any refusal.
//! Physical flight completion and a pending KV settlement are different states:
//! an effect never creates a fictitious extra physical fragment.

use super::super::flight::SettledFragment;
use super::super::state::{ReadyRows, RequestState, SettlementContinuation, request_key};
use crate::v2::{GeneratedToken, Phase, PhysicalOutcome, RowOwner, SettlementSequence};

#[derive(Default)]
pub(super) struct FragmentEffects {
    pub stopped: bool,
    pub settlement: Option<SettlementSequence>,
    pub outputs: Vec<(RowOwner, GeneratedToken)>,
    pub resolve_verify: bool,
}

pub(super) fn apply_fragment(
    request: &mut RequestState,
    fragment: &SettledFragment,
    next_spec: &mut u64,
) -> Result<FragmentEffects, String> {
    validate_rows(request, fragment)?;
    let first = &fragment.owners[0];
    let phase = first.phase;
    let mut effects = FragmentEffects::default();

    // Replay is an atomic sampling operation despite its false logical output
    // flags. Only ordinary, non-final prompt fragments have no decision.
    let decision_owner = match phase {
        Phase::Prefill => fragment.owners.iter().find(|owner| owner.output),
        Phase::Decode | Phase::Verify | Phase::Replay => Some(first),
    };
    let decision = match (&fragment.outcome, decision_owner) {
        (None, None) => None,
        (Some((owner, outcome)), Some(expected)) if owner == expected => {
            validate_outcome(owner, outcome, fragment.owners.len())?;
            Some((owner, outcome))
        }
        (None, Some(_)) => return Err("completed output fragment has no decision".into()),
        (Some(_), None) => return Err("non-output prompt fragment returned a decision".into()),
        _ => return Err("decision owner does not match its logical fragment".into()),
    };

    request
        .settle_fragment(phase, fragment.owners.len())
        .map_err(|refusal| refusal.as_str().to_owned())?;
    let Some((owner, outcome)) = decision else {
        return Ok(effects);
    };
    let generated = u32::try_from(outcome.generated.len())
        .map_err(|_| "generated token count overflow".to_owned())?;
    request.generated = request
        .generated
        .checked_add(generated)
        .ok_or_else(|| "generated token count overflow".to_owned())?;
    effects.outputs = outcome
        .generated
        .iter()
        .cloned()
        .map(|token| (owner.clone(), token))
        .collect();
    effects.stopped = outcome
        .generated
        .last()
        .is_some_and(|token| token.stop.is_some());
    if effects.stopped {
        request.ready = None;
        request.after_settlement = None;
        // Release acknowledgement, not physical completion, clears a stopped
        // Verify fence. The caller retains that separate release barrier.
        return Ok(effects);
    }

    if let Some(retain_from) = outcome.retain_from {
        let continuation = if outcome.replay_tokens.is_empty() {
            SettlementContinuation::Proposal {
                token: outcome
                    .generated
                    .last()
                    .expect("direct rollback emits tokens")
                    .token,
                position: outcome
                    .generated
                    .last()
                    .expect("direct rollback emits tokens")
                    .position,
            }
        } else {
            SettlementContinuation::Replay(ReadyRows {
                phase: Phase::Replay,
                tokens: outcome.replay_tokens.clone(),
                position: outcome.replay_position,
                speculative_id: owner.speculative_id,
            })
        };
        request.ready = None;
        request.after_settlement = Some(continuation);
        effects.settlement = Some(SettlementSequence {
            incarnation: request.incarnation,
            operation_id: 0, // Assigned by the whole-event commit candidate, never sent unassigned.
            key: fragment.key.clone(),
            id: owner.sequence_id,
            retain_from,
            replay_tokens: outcome.replay_tokens.clone(),
            replay_position: outcome.replay_position,
            proposal: Vec::new(),
        });
    } else {
        let position = outcome
            .generated
            .last()
            .expect("continuing decision emits tokens")
            .position;
        request.ready = Some(super::proposal::ready_from_proposal(
            next_spec,
            outcome.proposal.clone(),
            position,
        )?);
        request.after_settlement = None;
        effects.resolve_verify = phase == Phase::Verify;
    }
    Ok(effects)
}

fn validate_rows(request: &RequestState, fragment: &SettledFragment) -> Result<(), String> {
    let first = fragment
        .owners
        .first()
        .ok_or_else(|| "settled fragment has no rows".to_owned())?;
    let key = request_key(&request.command.session_id, &request.command.request_id);
    if fragment.key != key || request.after_settlement.is_some() {
        return Err("fragment identity or pending settlement does not match request".into());
    }
    let atomic = matches!(first.phase, Phase::Verify | Phase::Replay);
    let row_count = u32::try_from(fragment.owners.len())
        .map_err(|_| "fragment row count overflow".to_owned())?;
    for (index, owner) in fragment.owners.iter().enumerate() {
        let offset = u32::try_from(index).map_err(|_| "fragment position overflow".to_owned())?;
        if owner.load_generation != request.command.load_generation
            || owner.incarnation != request.incarnation
            || owner.session_id != request.command.session_id
            || owner.request_id != request.command.request_id
            || owner.sequence_key != key
            || Some(owner.sequence_id) != request.sequence_id
            || owner.reply != request.reply
            || owner.options != request.command.options
            || owner.max_tokens != request.command.max_tokens
            || owner.generated_tokens != request.generated
            || owner.phase != first.phase
            || first.position.checked_add(offset) != Some(owner.position)
            || (atomic
                && (owner.speculative_id == 0
                    || owner.speculative_id != first.speculative_id
                    || owner.speculative_count != row_count
                    || owner.speculative_index != offset))
            || (!atomic
                && (owner.speculative_id != 0
                    || owner.speculative_count != 0
                    || owner.speculative_index != 0))
        {
            return Err("settled row differs from the request's issued identity or range".into());
        }
    }
    match first.phase {
        Phase::Prefill => {
            if first.position as usize != request.prompt_cursor {
                return Err("prompt fragment does not start at its settled cursor".into());
            }
            for owner in &fragment.owners {
                let position = owner.position as usize;
                if request.command.tokens.get(position) != Some(&owner.input_token)
                    || position >= request.prompt_issued
                    || owner.output != (position + 1 == request.command.tokens.len())
                {
                    return Err(
                        "prompt row token, issued range or final output flag is invalid".into(),
                    );
                }
            }
        }
        Phase::Decode | Phase::Verify | Phase::Replay => {
            let ready = request
                .ready
                .as_ref()
                .ok_or_else(|| "settled work has no issued ready rows".to_owned())?;
            if request.prompt_cursor != request.command.tokens.len()
                || request.prompt_issued != request.command.tokens.len()
                || ready.phase != first.phase
                || ready.position != first.position
                || ready.speculative_id != first.speculative_id
                || ready.tokens.len() != fragment.owners.len()
                || (first.phase == Phase::Decode && fragment.owners.len() != 1)
                || fragment
                    .owners
                    .iter()
                    .zip(&ready.tokens)
                    .any(|(owner, token)| {
                        owner.input_token != *token
                            || owner.output != (first.phase != Phase::Replay)
                    })
            {
                return Err("settled work differs from the issued decode or atomic rows".into());
            }
        }
    }
    Ok(())
}

/// Checks a decision present on a partial physical return before its receipt
/// is committed. Missing decisions are allowed here: only complete logical
/// fragments can be required to have produced their decision.
pub(super) fn validate_decision(
    owners: &[RowOwner],
    outcome: &Option<(RowOwner, PhysicalOutcome)>,
) -> Result<(), String> {
    let first = owners
        .first()
        .ok_or_else(|| "decision fragment has no expected rows".to_owned())?;
    let Some((owner, outcome)) = outcome else {
        return Ok(());
    };
    let expected = match first.phase {
        Phase::Prefill => owners.iter().find(|owner| owner.output),
        Phase::Decode | Phase::Verify | Phase::Replay => Some(first),
    };
    if expected != Some(owner) {
        return Err("decision owner does not match its logical fragment".into());
    }
    validate_outcome(owner, outcome, owners.len())
}

fn validate_outcome(
    owner: &RowOwner,
    outcome: &PhysicalOutcome,
    rows: usize,
) -> Result<(), String> {
    super::super::frontier::validate_outcome(owner, outcome, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::InferenceCommand;
    use p4_protocol::Address;
    use p4_protocol::event::{Endpoint, Envelope, Event, EventClass};

    fn fixture(phase: Phase, rows: usize) -> (RequestState, SettledFragment) {
        let address = Address::tcp("127.0.0.1", 1);
        let command = InferenceCommand {
            load_generation: 1,
            session_id: "session".into(),
            request_id: "request".into(),
            tokens: vec![7; 4],
            prompt: None,
            options: String::new(),
            session_key: None,
            max_tokens: 16,
        };
        let event = Event {
            envelope: Envelope {
                protocol_version: Envelope::VERSION,
                event_id: "input".into(),
                correlation_id: "reply".into(),
                causation_id: None,
                source: Endpoint::agent(address.clone()),
                target: Endpoint::agent(address),
                return_route: Some(p4_protocol::event::OuterEndpoint { ingress_agent: p4_protocol::Address::tcp("127.0.0.1", 52001), channel: "outer".into(), connection_generation: 1 }),
                class: EventClass::Data,
                sequence: 1,
                deadline_unix_ms: None,
                adapter_kind: Some("llamacpp".into()),
                payload_content_type: "test".into(),
            },
            payload: Vec::new(),
        };
        let atomic = matches!(phase, Phase::Verify | Phase::Replay);
        let first_position = if phase == Phase::Prefill { 0 } else { 4 };
        let generated = if phase == Phase::Prefill { 0 } else { 1 };
        let key = request_key("session", "request");
        let owners = (0..rows)
            .map(|index| RowOwner {
                incarnation: 1,
                load_generation: 1,
                session_id: "session".into(),
                request_id: "request".into(),
                sequence_key: key.clone(),
                sequence_id: 0,
                reply: "reply".into(),
                options: String::new(),
                phase,
                position: first_position + index as u32,
                max_tokens: 16,
                generated_tokens: generated,
                output: if phase == Phase::Prefill {
                    index == 3
                } else {
                    phase != Phase::Replay
                },
                input_token: 7,
                speculative_id: if atomic { 3 } else { 0 },
                speculative_index: if atomic { index as u32 } else { 0 },
                speculative_count: if atomic { rows as u32 } else { 0 },
            })
            .collect();
        let mut request = RequestState::new(command, event, "reply".into(), 1, Some(0));
        request.prompt_cursor = if phase == Phase::Prefill { 0 } else { 4 };
        request.prompt_issued = 4;
        request.ready = (phase != Phase::Prefill).then_some(ReadyRows {
            phase,
            tokens: vec![7; rows],
            position: first_position,
            speculative_id: if atomic { 3 } else { 0 },
        });
        request.outstanding = 1;
        request.generated = generated;
        (
            request,
            SettledFragment {
                key,
                owners,
                outcome: None,
            },
        )
    }

    fn generated(position: u32, token: i32) -> GeneratedToken {
        GeneratedToken {
            token,
            text: String::new(),
            position,
            stop: None,
        }
    }

    fn decision(fragment: &mut SettledFragment, count: usize) {
        let owner = if fragment.owners[0].phase == Phase::Prefill {
            fragment.owners.last().unwrap()
        } else {
            &fragment.owners[0]
        }
        .clone();
        let tokens: Vec<_> = (0..count)
            .map(|index| generated(owner.position + index as u32 + 1, 10 + index as i32))
            .collect();
        let proposal = tokens
            .last()
            .map(|token| vec![token.token])
            .unwrap_or_default();
        fragment.outcome = Some((
            owner,
            PhysicalOutcome {
                owner_index: 0,
                generated: tokens,
                proposal,
                retain_from: None,
                replay_tokens: Vec::new(),
                replay_position: 0,
            },
        ));
    }

    #[test]
    fn ordinary_final_prefill_generates_once_and_preserves_empty_utf8_piece() {
        let (mut request, mut fragment) = fixture(Phase::Prefill, 4);
        decision(&mut fragment, 1);
        let effects = apply_fragment(&mut request, &fragment, &mut 8).unwrap();
        assert_eq!(
            (
                request.prompt_cursor,
                request.generated,
                request.outstanding
            ),
            (4, 1, 0)
        );
        assert_eq!(request.ready.as_ref().unwrap().position, 4);
        assert_eq!(effects.outputs.len(), 1);
        assert_eq!(effects.outputs[0].1.text, "");
    }

    #[test]
    fn partial_prefill_has_no_outcome_but_final_prefill_requires_one() {
        let (mut request, fragment) = fixture(Phase::Prefill, 2);
        assert!(
            apply_fragment(&mut request, &fragment, &mut 8)
                .unwrap()
                .outputs
                .is_empty()
        );
        assert_eq!((request.prompt_cursor, request.generated), (2, 0));
        let (mut request, fragment) = fixture(Phase::Prefill, 4);
        assert!(apply_fragment(&mut request, &fragment, &mut 8).is_err());
    }

    #[test]
    fn max_one_first_token_stops_without_creating_decode() {
        let (mut request, mut fragment) = fixture(Phase::Prefill, 4);
        request.input_mut_for_test().command.max_tokens = 1;
        for owner in &mut fragment.owners {
            owner.max_tokens = 1;
        }
        decision(&mut fragment, 1);
        let outcome = &mut fragment.outcome.as_mut().unwrap().1;
        outcome.generated[0].stop = Some("length".into());
        outcome.proposal.clear();
        assert!(
            apply_fragment(&mut request, &fragment, &mut 8)
                .unwrap()
                .stopped
        );
        assert_eq!(request.generated, 1);
        assert!(request.ready.is_none());
    }

    #[test]
    fn ordinary_decisions_reject_missing_extra_wrong_position_and_over_budget_tokens() {
        for mutation in 0..5 {
            let (mut request, mut fragment) = fixture(Phase::Decode, 1);
            decision(&mut fragment, 1);
            let outcome = &mut fragment.outcome.as_mut().unwrap().1;
            match mutation {
                0 => outcome.generated.clear(),
                1 => outcome.generated.push(generated(6, 11)),
                2 => outcome.generated[0].position += 1,
                3 => outcome.proposal[0] += 1,
                _ => outcome.proposal.resize(16, 10),
            }
            assert!(
                apply_fragment(&mut request, &fragment, &mut 8).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn ordinary_can_return_a_multi_token_speculative_proposal() {
        let (mut request, mut fragment) = fixture(Phase::Decode, 1);
        decision(&mut fragment, 1);
        fragment
            .outcome
            .as_mut()
            .unwrap()
            .1
            .proposal
            .extend([11, 12]);
        apply_fragment(&mut request, &fragment, &mut 8).unwrap();
        assert_eq!(request.ready.as_ref().unwrap().phase, Phase::Verify);
    }

    #[test]
    fn verify_full_acceptance_uses_first_owner_positions_and_resolves_fence() {
        let (mut request, mut fragment) = fixture(Phase::Verify, 3);
        decision(&mut fragment, 3);
        let effects = apply_fragment(&mut request, &fragment, &mut 8).unwrap();
        assert!(effects.resolve_verify);
        assert_eq!(
            effects
                .outputs
                .iter()
                .map(|(_, token)| token.position)
                .collect::<Vec<_>>(),
            vec![5, 6, 7]
        );
        assert_eq!(request.generated, 4);
    }

    #[test]
    fn verify_direct_rollback_waits_for_kv_without_inventing_physical_flight() {
        let (mut request, mut fragment) = fixture(Phase::Verify, 3);
        decision(&mut fragment, 2);
        let outcome = &mut fragment.outcome.as_mut().unwrap().1;
        outcome.proposal.clear();
        outcome.retain_from = Some(6);
        let effects = apply_fragment(&mut request, &fragment, &mut 8).unwrap();
        assert_eq!(request.outstanding, 0);
        assert_eq!(request.generated, 3);
        assert!(matches!(
            request.after_settlement,
            Some(SettlementContinuation::Proposal { position: 6, .. })
        ));
        assert_eq!(effects.settlement.unwrap().retain_from, 6);
        assert!(!effects.resolve_verify);
        assert!(request.ready.is_none());
    }

    #[test]
    fn verify_checkpoint_replay_may_generate_zero_then_replay_generates_the_group() {
        let (mut request, mut fragment) = fixture(Phase::Verify, 3);
        decision(&mut fragment, 0);
        let outcome = &mut fragment.outcome.as_mut().unwrap().1;
        outcome.replay_tokens = vec![7, 10];
        outcome.replay_position = 4;
        outcome.retain_from = Some(6);
        let effects = apply_fragment(&mut request, &fragment, &mut 8).unwrap();
        assert_eq!((request.generated, request.outstanding), (1, 0));
        assert!(effects.outputs.is_empty());
        assert!(matches!(
            request.after_settlement,
            Some(SettlementContinuation::Replay(_))
        ));
        assert!(effects.settlement.is_some());
        let (mut request, mut fragment) = fixture(Phase::Replay, 2);
        assert!(fragment.owners.iter().all(|owner| !owner.output));
        decision(&mut fragment, 2);
        let effects = apply_fragment(&mut request, &fragment, &mut 8).unwrap();
        assert_eq!(request.generated, 3);
        assert_eq!(effects.outputs.len(), 2);
        assert!(effects.settlement.is_none());
    }

    #[test]
    fn malformed_atomic_decisions_and_late_stop_are_refused() {
        for mutation in 0..6 {
            let (mut request, mut fragment) = fixture(Phase::Verify, 3);
            decision(&mut fragment, 2);
            let outcome = &mut fragment.outcome.as_mut().unwrap().1;
            match mutation {
                0 => {} // partial accept without rollback
                1 => {
                    outcome.proposal.clear();
                    outcome.retain_from = Some(7);
                }
                2 => {
                    outcome.generated[0].stop = Some("stop".into());
                }
                3 => {
                    outcome.generated.clear();
                    outcome.proposal.clear();
                    outcome.replay_tokens = vec![9, 10];
                    outcome.replay_position = 4;
                    outcome.retain_from = Some(6);
                }
                4 => {
                    outcome.generated.clear();
                    outcome.proposal.clear();
                    outcome.replay_tokens = vec![7, 10];
                    outcome.replay_position = 5;
                    outcome.retain_from = Some(7);
                }
                _ => {
                    outcome.generated[1].stop = Some("length".into());
                    outcome.proposal.clear();
                }
            }
            assert!(
                apply_fragment(&mut request, &fragment, &mut 8).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn atomic_stop_shortens_the_group_and_has_no_continuation() {
        let (mut request, mut fragment) = fixture(Phase::Verify, 3);
        decision(&mut fragment, 1);
        let outcome = &mut fragment.outcome.as_mut().unwrap().1;
        outcome.generated[0].stop = Some("eos".into());
        outcome.proposal.clear();
        let effects = apply_fragment(&mut request, &fragment, &mut 8).unwrap();
        assert!(effects.stopped);
        assert!(!effects.resolve_verify);
        assert!(effects.settlement.is_none());
    }

    #[test]
    fn request_identity_range_and_atomic_membership_are_not_only_codec_checks() {
        for mutation in 0..7 {
            let (mut request, mut fragment) = fixture(Phase::Verify, 3);
            decision(&mut fragment, 3);
            match mutation {
                0 => fragment.owners[0].load_generation += 1,
                1 => fragment.owners[0].sequence_id += 1,
                2 => fragment.owners[1].position += 1,
                3 => fragment.owners[1].speculative_index = 0,
                4 => fragment.owners[1].input_token += 1,
                5 => fragment.outcome.as_mut().unwrap().0 = fragment.owners[1].clone(),
                _ => fragment.key.push('x'),
            }
            assert!(
                apply_fragment(&mut request, &fragment, &mut 8).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn partial_return_checks_present_decisions_without_requiring_missing_ones() {
        let (_, mut fragment) = fixture(Phase::Prefill, 4);
        assert!(validate_decision(&fragment.owners, &None).is_ok());
        decision(&mut fragment, 1);
        assert!(validate_decision(&fragment.owners, &fragment.outcome).is_ok());
        fragment.outcome.as_mut().unwrap().1.generated[0].position += 1;
        assert!(validate_decision(&fragment.owners, &fragment.outcome).is_err());
        let (_, mut fragment) = fixture(Phase::Prefill, 2);
        decision(&mut fragment, 1);
        assert!(validate_decision(&fragment.owners, &fragment.outcome).is_err());
    }
}
