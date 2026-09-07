//! Stage-local KV write order, separate from execution replay and request/tail
//! settlement. Slots are changed by touched deltas, never a whole-registry copy.
//! The sole worker executes native work without interleaving a load or another
//! mutation. After an uncertain native effect it must fence, not commit a guess.
//! A nonterminal stage learns full Verify acceptance implicitly from the next
//! append at its written end; partial acceptance requires explicit SETTLE.

use super::ownership::Identity;
use crate::v2::{CapsuleSet, Phase, PhysicalOutcome, RowOwner};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
struct VerifyWindow {
    start: u32,
    end: u32,
    round: u64,
    generated: u32,
    first_token: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Expectation {
    Any,
    First(i32),
    Exact(Arc<[i32]>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Settlement {
    Direct { retain: u32, token: i32 },
    Checkpoint { retain: u32, tokens: Arc<[i32]> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    Prompt,
    Ready(Expectation),
    Verified(VerifyWindow),
    AwaitSettlement(VerifyWindow, Settlement),
    Replay {
        start: u32,
        end: u32,
        round: u64,
        tokens: Arc<[i32]>,
    },
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Frontier {
    identity: Identity,
    next: u32,
    generated: u32,
    max_tokens: u32,
    reply: Arc<str>,
    options: Arc<str>,
    last_round: u64,
    mode: Mode,
}

#[derive(Debug)]
struct Cell {
    // Empty cells keep this revision: remove/recreate must not admit an old
    // candidate that observed an empty slot. Cell count is bounded by capacity.
    revision: u64,
    value: Option<Arc<Frontier>>,
}

#[derive(Debug)]
struct Change {
    slot: u32,
    expected_revision: u64,
    next: Option<Frontier>,
}

#[derive(Debug)]
enum Completion {
    Rows(BTreeMap<u32, Vec<RowOwner>>),
    Settlement { slot: u32, direct: bool },
    Ready,
}

#[derive(Debug)]
pub(crate) struct FrontierDelta {
    origin: Arc<()>,
    generation: u64,
    changes: Vec<Change>,
    completion: Completion,
}

#[derive(Debug, Default)]
pub(crate) struct StageFrontiers {
    origin: Arc<()>,
    generation: Option<u64>,
    cells: BTreeMap<u32, Cell>,
}

impl StageFrontiers {
    /// Includes Stopped until RELEASE, excludes empty revision tombstones.
    pub fn active_slots(&self) -> usize {
        self.cells
            .values()
            .filter(|cell| cell.value.is_some())
            .count()
    }

    fn delta(
        &self,
        generation: u64,
        changes: Vec<Change>,
        completion: Completion,
    ) -> Result<FrontierDelta, String> {
        if generation == 0 || self.generation.is_some_and(|bound| bound != generation) {
            return Err("frontier load generation is unbound or stale".into());
        }
        for change in &changes {
            change
                .expected_revision
                .checked_add(1)
                .ok_or("frontier slot revision exhausted")?;
        }
        Ok(FrontierDelta {
            origin: Arc::clone(&self.origin),
            generation,
            changes,
            completion,
        })
    }

    fn validate_delta(&self, delta: &FrontierDelta) -> Result<(), String> {
        if !Arc::ptr_eq(&self.origin, &delta.origin)
            || self
                .generation
                .is_some_and(|value| value != delta.generation)
            || delta.changes.iter().any(|change| {
                self.cells.get(&change.slot).map_or(0, |cell| cell.revision)
                    != change.expected_revision
            })
        {
            return Err("frontier delta is foreign, stale, or belongs to another load".into());
        }
        Ok(())
    }

    pub fn prepare_rows(
        &self,
        generation: u64,
        capacity: u32,
        rows: &[&RowOwner],
    ) -> Result<FrontierDelta, String> {
        if rows.is_empty() || capacity == 0 {
            return Err("frontier rows require nonempty loaded capacity".into());
        }
        let mut groups = BTreeMap::<u32, Vec<RowOwner>>::new();
        for row in rows {
            if row.load_generation != generation
                || row.sequence_id >= capacity
                || row.incarnation == 0
                || !row.has_canonical_request_identity()
                || row.reply.is_empty()
                || row.input_token < 0
                || row.max_tokens == 0
                || row.generated_tokens >= row.max_tokens
                || row.position > i32::MAX as u32
            {
                return Err("frontier row identity, capacity, token or budget is invalid".into());
            }
            groups
                .entry(row.sequence_id)
                .or_default()
                .push((*row).clone());
        }
        let mut changes = Vec::with_capacity(groups.len());
        for (slot, rows) in &groups {
            let first = &rows[0];
            let identity = Identity::from_owner(first);
            let cell = self.cells.get(slot);
            let current = cell.and_then(|cell| cell.value.as_deref());
            let mut next = if let Some(current) = current {
                if current.identity != identity {
                    return Err("frontier slot has a different unreleased identity".into());
                }
                current.clone()
            } else {
                if first.phase != Phase::Prefill
                    || first.position != 0
                    || first.generated_tokens != 0
                {
                    return Err("frontier admission must begin with prefill position zero".into());
                }
                Frontier {
                    identity,
                    next: 0,
                    generated: 0,
                    max_tokens: first.max_tokens,
                    reply: Arc::from(first.reply.as_str()),
                    options: Arc::from(first.options.as_str()),
                    last_round: 0,
                    mode: Mode::Prompt,
                }
            };
            advance_rows(&mut next, rows)?;
            changes.push(Change {
                slot: *slot,
                expected_revision: cell.map_or(0, |cell| cell.revision),
                next: Some(next),
            });
        }
        self.delta(generation, changes, Completion::Rows(groups))
    }

    /// All returned rows and decisions are checked before a delta is ready to
    /// commit. A returned native error is not an invitation to reuse this delta.
    pub fn complete_rows(
        &self,
        mut delta: FrontierDelta,
        result: &CapsuleSet,
        terminal: bool,
    ) -> Result<FrontierDelta, String> {
        self.validate_delta(&delta)?;
        let Completion::Rows(groups) = &delta.completion else {
            return Err("frontier delta does not await a row result".into());
        };
        let mut expected: BTreeMap<_, _> = groups
            .values()
            .flatten()
            .map(|row| ((row.sequence_id, row.position), row))
            .collect();
        let mut decisions = BTreeMap::new();
        let mut execution_ids = std::collections::BTreeSet::new();
        let mut returned_per_slot = BTreeMap::<u32, usize>::new();
        for capsule in &result.0 {
            capsule
                .validate()
                .map_err(|e| format!("frontier invalid result: {e:?}"))?;
            super::flight::validate_membership(capsule)
                .map_err(|e| format!("frontier result: {e}"))?;
            if capsule.terminal != terminal || !execution_ids.insert(capsule.execution_id) {
                return Err("frontier result has a duplicated execution or wrong role".into());
            }
            for row in &capsule.owners {
                let offset = returned_per_slot.entry(row.sequence_id).or_default();
                if groups
                    .get(&row.sequence_id)
                    .and_then(|rows| rows.get(*offset))
                    != Some(row)
                {
                    return Err("frontier result reordered the per-sequence write range".into());
                }
                *offset += 1;
                if expected.remove(&(row.sequence_id, row.position)) != Some(row) {
                    return Err("frontier result changed, duplicated or invented a row".into());
                }
            }
            for outcome in &capsule.outcomes {
                let owner = &capsule.owners[outcome.owner_index as usize];
                if decisions
                    .insert(owner.sequence_id, (owner, outcome))
                    .is_some()
                {
                    return Err("frontier result has multiple decisions for one slot".into());
                }
            }
        }
        if !expected.is_empty() {
            return Err("frontier result omitted issued rows".into());
        }
        if terminal {
            for change in &mut delta.changes {
                let rows = &groups[&change.slot];
                let first = &rows[0];
                let decision_owner = if first.phase == Phase::Prefill {
                    rows.iter().find(|row| row.output)
                } else {
                    Some(first)
                };
                let decision = decisions.remove(&change.slot);
                match (decision_owner, decision) {
                    (None, None) => {}
                    (Some(owner), Some((actual, outcome))) if owner == actual => {
                        validate_outcome(owner, outcome, rows.len())
                            .map_err(|error| format!("frontier outcome: {error}"))?;
                        apply_decision(
                            change.next.as_mut().expect("rows keep their frontier"),
                            rows,
                            outcome,
                        )?;
                    }
                    _ => return Err("frontier result omitted or misplaced its decision".into()),
                }
            }
        }
        if !decisions.is_empty() {
            return Err("frontier result has an unexpected decision".into());
        }
        delta.completion = Completion::Ready;
        Ok(delta)
    }

    fn owned(&self, identity: &Identity) -> Result<(&Cell, &Frontier), String> {
        self.cells
            .get(&identity.sequence_id)
            .and_then(|cell| cell.value.as_deref().map(|value| (cell, value)))
            .filter(|(_, value)| value.identity == *identity)
            .ok_or_else(|| "frontier control does not own the current slot".into())
    }

    pub fn prepare_settlement(
        &self,
        identity: &Identity,
        retain_from: u32,
        replay_position: u32,
        replay_tokens: &[i32],
    ) -> Result<FrontierDelta, String> {
        let (cell, current) = self.owned(identity)?;
        let (window, expectation) = match &current.mode {
            Mode::Verified(window) => (window, None),
            Mode::AwaitSettlement(window, expectation) => (window, Some(expectation)),
            _ => return Err("frontier SETTLE has no pending Verify window".into()),
        };
        let mut next = current.clone();
        let direct = replay_tokens.is_empty();
        if direct {
            if replay_position != 0 || retain_from <= window.start || retain_from >= window.end {
                return Err("frontier direct SETTLE is outside its Verify window".into());
            }
            let token = match expectation {
                None => Expectation::Any,
                Some(Settlement::Direct { retain, token }) if *retain == retain_from => {
                    Expectation::First(*token)
                }
                _ => return Err("frontier SETTLE differs from the tail rollback decision".into()),
            };
            next.next = retain_from;
            next.generated = window
                .generated
                .checked_add(retain_from - window.start)
                .ok_or("frontier generation overflow")?;
            next.mode = Mode::Ready(token);
        } else {
            if replay_position != window.start
                || replay_tokens.len() < 2
                || replay_tokens.len()
                    > current.max_tokens.saturating_sub(window.generated) as usize
                || replay_tokens.iter().any(|token| *token < 0)
                || replay_tokens.first() != Some(&window.first_token)
                || u32::try_from(replay_tokens.len())
                    .ok()
                    .and_then(|len| replay_position.checked_add(len))
                    != Some(retain_from)
                || retain_from > window.end
            {
                return Err("frontier checkpoint SETTLE changed its replay range or input".into());
            }
            if expectation.is_some_and(|expected| !matches!(expected, Settlement::Checkpoint { retain, tokens } if *retain == retain_from && tokens.as_ref() == replay_tokens)) {
                return Err("frontier checkpoint SETTLE differs from the tail replay decision".into());
            }
            // Restore goes to the pre-Verify checkpoint. retain_from is the
            // end of the FUTURE replay, not the restored memory frontier.
            next.next = replay_position;
            next.generated = window.generated;
            next.mode = Mode::Replay {
                start: replay_position,
                end: retain_from,
                round: window.round,
                tokens: Arc::from(replay_tokens),
            };
        }
        self.delta(
            identity.load_generation,
            vec![Change {
                slot: identity.sequence_id,
                expected_revision: cell.revision,
                next: Some(next),
            }],
            Completion::Settlement {
                slot: identity.sequence_id,
                direct,
            },
        )
    }

    pub fn complete_settlement(
        &self,
        mut delta: FrontierDelta,
        proposal: &[i32],
        terminal: bool,
    ) -> Result<FrontierDelta, String> {
        self.validate_delta(&delta)?;
        let Completion::Settlement { slot, direct } = delta.completion else {
            return Err("frontier delta does not await a SETTLE result".into());
        };
        let next = delta
            .changes
            .iter_mut()
            .find(|change| change.slot == slot)
            .and_then(|change| change.next.as_mut())
            .expect("settlement has one slot");
        if terminal && direct {
            let Mode::Ready(Expectation::First(token)) = &next.mode else {
                return Err("frontier terminal SETTLE lost the sampled token".into());
            };
            if proposal.first() != Some(token)
                || proposal.iter().any(|token| *token < 0)
                || proposal.len() > next.max_tokens.saturating_sub(next.generated) as usize
            {
                return Err("frontier SETTLE proposal changed its token or budget".into());
            }
            next.mode = Mode::Ready(Expectation::Exact(Arc::from(proposal)));
        } else if !proposal.is_empty() {
            return Err(
                "frontier nonterminal or checkpoint SETTLE cannot produce a proposal".into(),
            );
        }
        delta.completion = Completion::Ready;
        Ok(delta)
    }

    pub fn prepare_release(&self, identity: &Identity) -> Result<FrontierDelta, String> {
        let (cell, _) = self.owned(identity)?;
        self.delta(
            identity.load_generation,
            vec![Change {
                slot: identity.sequence_id,
                expected_revision: cell.revision,
                next: None,
            }],
            Completion::Ready,
        )
    }

    pub fn commit(&mut self, delta: FrontierDelta) -> Result<(), String> {
        self.validate_delta(&delta)?;
        if !matches!(delta.completion, Completion::Ready) {
            return Err("frontier cannot commit before result validation".into());
        }
        self.generation = Some(delta.generation);
        for change in delta.changes {
            self.cells.insert(
                change.slot,
                Cell {
                    revision: change.expected_revision + 1,
                    value: change.next.map(Arc::new),
                },
            );
        }
        Ok(())
    }
}

fn advance_rows(next: &mut Frontier, rows: &[RowOwner]) -> Result<(), String> {
    let first = &rows[0];
    let count = u32::try_from(rows.len()).map_err(|_| "frontier row count overflow")?;
    let end = first
        .position
        .checked_add(count)
        .ok_or("frontier position overflow")?;
    let atomic = matches!(first.phase, Phase::Verify | Phase::Replay);
    for (index, row) in rows.iter().enumerate() {
        if Identity::from_owner(row) != next.identity
            || row.phase != first.phase
            || row.position != first.position + index as u32
            || row.position > i32::MAX as u32
            || row.max_tokens != next.max_tokens
            || row.generated_tokens != next.generated
            || row.reply.as_str() != next.reply.as_ref()
            || row.options.as_str() != next.options.as_ref()
            || (atomic
                && (row.speculative_id == 0
                    || row.speculative_id != first.speculative_id
                    || row.speculative_index != index as u32
                    || row.speculative_count != count))
            || (!atomic
                && (row.speculative_id != 0
                    || row.speculative_index != 0
                    || row.speculative_count != 0))
        {
            return Err(
                "frontier rows changed identity, phase, generation or contiguous range".into(),
            );
        }
    }
    if first.position != next.next {
        return Err("frontier row starts at an old position or future gap".into());
    }
    match (&next.mode, first.phase) {
        (Mode::Prompt, Phase::Prefill) => {
            if rows[..rows.len() - 1].iter().any(|row| row.output) {
                return Err("frontier prompt output must be the last row".into());
            }
            if rows.last().unwrap().output {
                next.generated = next
                    .generated
                    .checked_add(1)
                    .ok_or("frontier generation overflow")?;
                next.mode = Mode::Ready(Expectation::Any);
            }
        }
        (Mode::Ready(expected), Phase::Decode | Phase::Verify) => {
            match expected {
                Expectation::Any => {}
                Expectation::First(token) if first.input_token == *token => {}
                Expectation::Exact(tokens)
                    if tokens.len() == rows.len()
                        && tokens
                            .iter()
                            .zip(rows)
                            .all(|(token, row)| *token == row.input_token)
                        && (tokens.len() > 1) == (first.phase == Phase::Verify) => {}
                _ => return Err("frontier next rows differ from the sampled proposal".into()),
            }
            advance_generation(next, rows, end)?;
        }
        (Mode::Verified(_), Phase::Decode | Phase::Verify) => {
            // The configured head has seen the previous tail result. A next
            // append exactly at its end confirms full acceptance implicitly.
            advance_generation(next, rows, end)?;
        }
        (
            Mode::Replay {
                start,
                end: replay_end,
                round,
                tokens,
            },
            Phase::Replay,
        ) => {
            if *start != first.position
                || *replay_end != end
                || *round != first.speculative_id
                || rows.len() > next.max_tokens.saturating_sub(next.generated) as usize
                || tokens.len() != rows.len()
                || tokens
                    .iter()
                    .zip(rows)
                    .any(|(token, row)| *token != row.input_token || row.output)
            {
                return Err("frontier Replay differs from its exact SETTLE permit".into());
            }
            next.generated = next
                .generated
                .checked_add(count)
                .ok_or("frontier generation overflow")?;
            next.mode = Mode::Ready(Expectation::Any);
        }
        _ => return Err("frontier phase is not authorized by its current state".into()),
    }
    next.next = end;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::{GeneratedToken, Invocation, PhysicalCapsule, Tensor, TensorDescriptor};

    #[test]
    fn continuation_width_checks_proposal_and_replay_without_truncation() {
        assert!(validate_continuation_width(&[], &[], 0).is_ok());
        for width in [1, 2] {
            assert!(validate_continuation_width(&vec![7; width], &[], 2).is_ok());
            assert!(validate_continuation_width(&[], &vec![7; width], 2).is_ok());
        }
        assert!(validate_continuation_width(&[7], &[], 0).is_err());
        assert!(validate_continuation_width(&[7, 8, 9], &[], 2).is_err());
        assert!(validate_continuation_width(&[], &[7, 8, 9], 2).is_err());
    }

    fn rows(slot: u32, phase: Phase, start: u32, generated: u32, tokens: &[i32]) -> Vec<RowOwner> {
        tokens
            .iter()
            .enumerate()
            .map(|(index, token)| RowOwner {
                load_generation: 1,
                incarnation: 1,
                request_id: format!("request-{slot}"),
                sequence_key: format!("session\0request-{slot}"),
                session_id: "session".into(),
                reply: "reply".into(),
                sequence_id: slot,
                phase,
                position: start + index as u32,
                max_tokens: 32,
                generated_tokens: generated,
                output: if phase == Phase::Prefill {
                    index + 1 == tokens.len()
                } else {
                    phase != Phase::Replay
                },
                input_token: *token,
                speculative_id: if matches!(phase, Phase::Verify | Phase::Replay) {
                    7
                } else {
                    0
                },
                speculative_index: if matches!(phase, Phase::Verify | Phase::Replay) {
                    index as u32
                } else {
                    0
                },
                speculative_count: if matches!(phase, Phase::Verify | Phase::Replay) {
                    tokens.len() as u32
                } else {
                    0
                },
                options: String::new(),
            })
            .collect()
    }

    fn capsule(
        id: u64,
        rows: &[RowOwner],
        terminal: bool,
        outcomes: Vec<PhysicalOutcome>,
    ) -> PhysicalCapsule {
        PhysicalCapsule {
            execution_id: id,
            terminal,
            invocation: Invocation {
                flags: 0,
                n_seq_tokens: rows.len() as u32,
                n_seqs: 1,
                n_seqs_unq: 1,
                n_pos: 1,
                positions: rows.iter().map(|row| row.position as i32).collect(),
                sequence_counts: vec![1; rows.len()],
                sequence_ids: rows.iter().map(|row| row.sequence_id as i32).collect(),
                output: rows.iter().map(|row| row.output).collect(),
            },
            owners: rows.to_vec(),
            tensors: if terminal {
                vec![]
            } else {
                vec![Tensor {
                    descriptor: TensorDescriptor {
                        tensor_type: 0,
                        dimensions: vec![1],
                        strides: vec![4],
                        nbytes: 4,
                        view_offset: 0,
                        alias_of: None,
                        name: "activation".into(),
                    },
                    data: vec![0; 4],
                }]
            },
            outcomes,
        }
    }

    fn decision(rows: &[RowOwner], tokens: &[i32], proposal: &[i32]) -> PhysicalOutcome {
        let owner_index = if rows[0].phase == Phase::Prefill {
            rows.len() - 1
        } else {
            0
        };
        let owner = &rows[owner_index];
        PhysicalOutcome {
            owner_index: owner_index as u32,
            generated: tokens
                .iter()
                .enumerate()
                .map(|(index, token)| GeneratedToken {
                    token: *token,
                    text: String::new(),
                    position: owner.position + index as u32 + 1,
                    stop: None,
                })
                .collect(),
            proposal: proposal.to_vec(),
            retain_from: None,
            replay_tokens: vec![],
            replay_position: 0,
        }
    }

    fn prepare(ledger: &StageFrontiers, rows: &[RowOwner]) -> Result<FrontierDelta, String> {
        ledger.prepare_rows(1, 8, &rows.iter().collect::<Vec<_>>())
    }

    fn execute(
        ledger: &mut StageFrontiers,
        rows: &[RowOwner],
        terminal: bool,
        outcomes: Vec<PhysicalOutcome>,
    ) {
        let pending = prepare(ledger, rows).unwrap();
        let complete = ledger
            .complete_rows(
                pending,
                &CapsuleSet(vec![capsule(1, rows, terminal, outcomes)]),
                terminal,
            )
            .unwrap();
        ledger.commit(complete).unwrap();
    }

    fn tail_prompt(ledger: &mut StageFrontiers, slot: u32, proposal: &[i32]) -> Vec<RowOwner> {
        let prompt = rows(slot, Phase::Prefill, 0, 0, &[5, 6]);
        execute(
            ledger,
            &prompt,
            true,
            vec![decision(&prompt, &[101], proposal)],
        );
        prompt
    }

    fn snapshot(ledger: &StageFrontiers) -> Vec<(u32, u64, Option<Frontier>)> {
        ledger
            .cells
            .iter()
            .map(|(slot, cell)| (*slot, cell.revision, cell.value.as_deref().cloned()))
            .collect()
    }

    fn rejected<T: std::fmt::Debug>(result: Result<T, String>) {
        assert!(result.unwrap_err().contains("frontier"));
    }

    #[test]
    fn partial_prompt_advances_only_after_exact_complete_and_final_output_enables_decode() {
        let mut ledger = StageFrontiers::default();
        let mut prompt = rows(0, Phase::Prefill, 0, 0, &[5, 6]);
        prompt.last_mut().unwrap().output = false;
        let pending = prepare(&ledger, &prompt).unwrap();
        assert!(ledger.cells.is_empty());
        rejected(ledger.commit(pending));
        execute(&mut ledger, &prompt, false, vec![]);
        let before = snapshot(&ledger);
        for (phase, position, generated) in [
            (Phase::Decode, 2, 0),
            (Phase::Prefill, 1, 0),
            (Phase::Prefill, 3, 0),
        ] {
            rejected(prepare(&ledger, &rows(0, phase, position, generated, &[7])));
        }
        assert_eq!(snapshot(&ledger), before);
        let final_prompt = rows(0, Phase::Prefill, 2, 0, &[7]);
        execute(
            &mut ledger,
            &final_prompt,
            true,
            vec![decision(&final_prompt, &[101], &[101])],
        );
        rejected(prepare(&ledger, &rows(0, Phase::Prefill, 3, 1, &[101])));
        let decode = rows(0, Phase::Decode, 3, 1, &[101]);
        execute(
            &mut ledger,
            &decode,
            true,
            vec![decision(&decode, &[102], &[102])],
        );
        assert_eq!(
            (
                ledger.cells[&0].value.as_ref().unwrap().next,
                ledger.cells[&0].value.as_ref().unwrap().generated
            ),
            (4, 2)
        );
    }

    #[test]
    fn whole_row_set_rejection_preserves_every_slot_and_all_metadata() {
        let mut ledger = StageFrontiers::default();
        tail_prompt(&mut ledger, 0, &[101]);
        tail_prompt(&mut ledger, 1, &[101]);
        let before = snapshot(&ledger);
        let first = rows(0, Phase::Decode, 2, 1, &[101]);
        let second = rows(1, Phase::Decode, 2, 1, &[101]);
        for mutation in 0..10 {
            let mut bad = second.clone();
            match mutation {
                0 => bad[0].position += 1,
                1 => bad[0].generated_tokens += 1,
                2 => bad[0].max_tokens += 1,
                3 => bad[0].options = "different".into(),
                4 => bad[0].reply = "different".into(),
                5 => bad[0].incarnation += 1,
                6 => bad[0].load_generation += 1,
                7 => bad[0].sequence_key = "session\0different".into(),
                8 => bad[0].input_token += 1,
                9 => bad[0].phase = Phase::Prefill,
                _ => unreachable!(),
            }
            rejected(prepare(&ledger, &[first.clone(), bad].concat()));
            assert_eq!(snapshot(&ledger), before);
        }
        let pending = prepare(&ledger, &[first.clone(), second.clone()].concat()).unwrap();
        let mut wrong = decision(&second, &[102], &[102]);
        wrong.generated[0].position += 1;
        let result = CapsuleSet(vec![
            capsule(1, &first, true, vec![decision(&first, &[102], &[102])]),
            capsule(2, &second, true, vec![wrong]),
        ]);
        rejected(ledger.complete_rows(pending, &result, true));
        assert_eq!(snapshot(&ledger), before);
        execute(
            &mut ledger,
            &first,
            true,
            vec![decision(&first, &[102], &[102])],
        );
    }

    #[test]
    fn physical_results_cannot_reorder_contiguous_chunks_or_split_atomic_groups() {
        let ledger = StageFrontiers::default();
        let mut prompt = rows(0, Phase::Prefill, 0, 0, &[5, 6, 7, 8]);
        prompt.last_mut().unwrap().output = false;
        let result = CapsuleSet(vec![
            capsule(1, &prompt[2..], false, vec![]),
            capsule(2, &prompt[..2], false, vec![]),
        ]);
        rejected(ledger.complete_rows(prepare(&ledger, &prompt).unwrap(), &result, false));
        assert!(ledger.cells.is_empty());
        let result = CapsuleSet(vec![
            capsule(1, &prompt[..2], false, vec![]),
            capsule(2, &prompt[2..], false, vec![]),
        ]);
        assert!(
            ledger
                .complete_rows(prepare(&ledger, &prompt).unwrap(), &result, false)
                .is_ok()
        );
        let mut ledger = ledger;
        tail_prompt(&mut ledger, 0, &[101, 201, 202]);
        let verify = rows(0, Phase::Verify, 2, 1, &[101, 201, 202]);
        let split = CapsuleSet(vec![
            capsule(1, &verify[..1], false, vec![]),
            capsule(2, &verify[1..], false, vec![]),
        ]);
        let error = ledger
            .complete_rows(prepare(&ledger, &verify).unwrap(), &split, false)
            .unwrap_err();
        assert!(error.contains("atomic group was split"), "{error}");
    }

    #[test]
    fn nonterminal_verify_full_acceptance_is_implicit_but_old_round_or_generation_is_not() {
        let mut ledger = StageFrontiers::default();
        let prompt = rows(0, Phase::Prefill, 0, 0, &[5, 6]);
        execute(&mut ledger, &prompt, false, vec![]);
        let verify = rows(0, Phase::Verify, 2, 1, &[101, 201, 202]);
        execute(&mut ledger, &verify, false, vec![]);
        let before = snapshot(&ledger);
        rejected(prepare(&ledger, &rows(0, Phase::Decode, 5, 1, &[301])));
        rejected(prepare(&ledger, &rows(0, Phase::Verify, 5, 4, &[301, 302])));
        assert_eq!(snapshot(&ledger), before);
        execute(
            &mut ledger,
            &rows(0, Phase::Decode, 5, 4, &[301]),
            false,
            vec![],
        );
        rejected(ledger.prepare_settlement(&Identity::from_owner(&verify[0]), 3, 0, &[]));
    }

    #[test]
    fn terminal_full_acceptance_binds_the_entire_next_proposal_and_phase() {
        let mut ledger = StageFrontiers::default();
        tail_prompt(&mut ledger, 0, &[101, 201, 202]);
        let verify = rows(0, Phase::Verify, 2, 1, &[101, 201, 202]);
        let mut bad = verify.clone();
        bad[2].input_token = 203;
        rejected(prepare(&ledger, &bad));
        rejected(prepare(&ledger, &rows(0, Phase::Decode, 2, 1, &[101])));
        execute(
            &mut ledger,
            &verify,
            true,
            vec![decision(&verify, &[201, 202, 301], &[301, 302])],
        );
        let mut next = rows(0, Phase::Verify, 5, 4, &[301, 302]);
        rejected(prepare(&ledger, &next)); // same round is not a new proposal
        next.iter_mut().for_each(|row| row.speculative_id += 1);
        assert!(prepare(&ledger, &next).is_ok());
        next[1].input_token += 1;
        rejected(prepare(&ledger, &next));
    }

    fn partial(ledger: &mut StageFrontiers, slot: u32, checkpoint: bool) -> Identity {
        tail_prompt(ledger, slot, &[101, 201, 202]);
        let verify = rows(slot, Phase::Verify, 2, 1, &[101, 201, 202]);
        let mut outcome = decision(&verify, if checkpoint { &[] } else { &[201] }, &[]);
        outcome.retain_from = Some(if checkpoint { 4 } else { 3 });
        if checkpoint {
            outcome.replay_position = 2;
            outcome.replay_tokens = vec![101, 201];
        }
        execute(ledger, &verify, true, vec![outcome]);
        Identity::from_owner(&verify[0])
    }

    #[test]
    fn direct_partial_requires_exact_settlement_and_sampled_proposal_before_append() {
        let mut ledger = StageFrontiers::default();
        let identity = partial(&mut ledger, 0, false);
        let before = snapshot(&ledger);
        rejected(prepare(&ledger, &rows(0, Phase::Decode, 3, 2, &[201])));
        for retain in [1, 2, 4, 5, 6] {
            rejected(ledger.prepare_settlement(&identity, retain, 0, &[]));
        }
        rejected(ledger.prepare_settlement(&identity, 3, 1, &[]));
        let delta = ledger.prepare_settlement(&identity, 3, 0, &[]).unwrap();
        rejected(ledger.complete_settlement(delta, &[202], true));
        assert_eq!(snapshot(&ledger), before);
        let delta = ledger.prepare_settlement(&identity, 3, 0, &[]).unwrap();
        let delta = ledger
            .complete_settlement(delta, &[201, 302], true)
            .unwrap();
        ledger.commit(delta).unwrap();
        assert_eq!(
            (
                ledger.cells[&0].value.as_ref().unwrap().next,
                ledger.cells[&0].value.as_ref().unwrap().generated
            ),
            (3, 2)
        );
        rejected(prepare(&ledger, &rows(0, Phase::Decode, 3, 2, &[201])));
        let mut next = rows(0, Phase::Verify, 3, 2, &[201, 302]);
        next.iter_mut().for_each(|row| row.speculative_id = 8);
        assert!(prepare(&ledger, &next).is_ok());
    }

    #[test]
    fn checkpoint_settlement_restores_start_and_authorizes_only_exact_replay() {
        let mut ledger = StageFrontiers::default();
        let identity = partial(&mut ledger, 0, true);
        let before = snapshot(&ledger);
        for (retain, start, tokens) in [
            (4, 2, vec![101, 202]),
            (3, 2, vec![101]),
            (4, 3, vec![101, 201]),
            (5, 2, vec![101, 201, 202]),
        ] {
            rejected(ledger.prepare_settlement(&identity, retain, start, &tokens));
        }
        assert_eq!(snapshot(&ledger), before);
        let delta = ledger
            .prepare_settlement(&identity, 4, 2, &[101, 201])
            .unwrap();
        rejected(ledger.complete_settlement(delta, &[201], true));
        let delta = ledger
            .prepare_settlement(&identity, 4, 2, &[101, 201])
            .unwrap();
        let delta = ledger.complete_settlement(delta, &[], true).unwrap();
        ledger.commit(delta).unwrap();
        assert_eq!(
            (
                ledger.cells[&0].value.as_ref().unwrap().next,
                ledger.cells[&0].value.as_ref().unwrap().generated
            ),
            (2, 1)
        );
        let replay = rows(0, Phase::Replay, 2, 1, &[101, 201]);
        for mutation in 0..6 {
            let mut bad = replay.clone();
            match mutation {
                0 => bad[1].input_token += 1,
                1 => bad[0].output = true,
                2 => bad.iter_mut().for_each(|row| row.position += 1),
                3 => bad.iter_mut().for_each(|row| row.speculative_id += 1),
                4 => bad.iter_mut().for_each(|row| row.phase = Phase::Verify),
                5 => {
                    bad.pop();
                }
                _ => unreachable!(),
            }
            rejected(prepare(&ledger, &bad));
        }
        execute(
            &mut ledger,
            &replay,
            true,
            vec![decision(&replay, &[201, 301], &[301])],
        );
        rejected(prepare(&ledger, &replay));
        assert!(prepare(&ledger, &rows(0, Phase::Decode, 4, 3, &[301])).is_ok());
    }

    #[test]
    fn distinct_slot_controls_commit_from_one_preflight_but_stale_foreign_and_aba_deltas_do_not() {
        let mut ledger = StageFrontiers::default();
        let first = tail_prompt(&mut ledger, 0, &[101]);
        let second = tail_prompt(&mut ledger, 1, &[101]);
        let stale = ledger
            .prepare_release(&Identity::from_owner(&first[0]))
            .unwrap();
        let a = ledger
            .prepare_release(&Identity::from_owner(&first[0]))
            .unwrap();
        let b = ledger
            .prepare_release(&Identity::from_owner(&second[0]))
            .unwrap();
        ledger.commit(a).unwrap();
        ledger.commit(b).unwrap();
        rejected(ledger.commit(stale));
        let mut replacement = first.clone();
        replacement.iter_mut().for_each(|row| row.incarnation = 2);
        let old_empty = prepare(&ledger, &replacement).unwrap();
        execute(
            &mut ledger,
            &replacement,
            true,
            vec![decision(&replacement, &[101], &[101])],
        );
        let release = ledger
            .prepare_release(&Identity::from_owner(&replacement[0]))
            .unwrap();
        ledger.commit(release).unwrap();
        rejected(ledger.complete_rows(
            old_empty,
            &CapsuleSet(vec![capsule(
                1,
                &replacement,
                true,
                vec![decision(&replacement, &[101], &[101])],
            )]),
            true,
        ));
        let mut another = StageFrontiers::default();
        tail_prompt(&mut another, 0, &[101]);
        let foreign = another
            .prepare_release(&Identity::from_owner(&first[0]))
            .unwrap();
        assert!(ledger.commit(foreign).unwrap_err().contains("foreign"));
    }

    #[test]
    fn capacity_generation_width_and_revision_exhaustion_fail_before_mutation() {
        let mut ledger = StageFrontiers::default();
        let prompt = rows(0, Phase::Prefill, 0, 0, &[5, 6]);
        rejected(ledger.prepare_rows(0, 8, &prompt.iter().collect::<Vec<_>>()));
        rejected(ledger.prepare_rows(1, 0, &prompt.iter().collect::<Vec<_>>()));
        rejected(prepare(&ledger, &rows(8, Phase::Prefill, 0, 0, &[5])));
        let mut low_budget = prompt.clone();
        low_budget.iter_mut().for_each(|row| row.max_tokens = 3);
        execute(&mut ledger, &low_budget, false, vec![]);
        let before = snapshot(&ledger);
        let mut verify = rows(0, Phase::Verify, 2, 1, &[101, 201, 202]);
        verify.iter_mut().for_each(|row| row.max_tokens = 3);
        rejected(prepare(&ledger, &verify));
        let mut wrong_load = rows(1, Phase::Prefill, 0, 0, &[5]);
        wrong_load[0].load_generation = 2;
        rejected(ledger.prepare_rows(2, 8, &wrong_load.iter().collect::<Vec<_>>()));
        assert_eq!(snapshot(&ledger), before);
        ledger.cells.get_mut(&0).unwrap().revision = u64::MAX;
        let identity = Identity::from_owner(&prompt[0]);
        assert!(
            ledger
                .prepare_release(&identity)
                .unwrap_err()
                .contains("exhausted")
        );
        assert_eq!(ledger.cells[&0].revision, u64::MAX);
    }

    #[test]
    fn stopped_sequences_require_release_and_replay_has_no_unsolicited_entry() {
        let mut ledger = StageFrontiers::default();
        let prompt = rows(0, Phase::Prefill, 0, 0, &[5, 6]);
        let mut stopped = decision(&prompt, &[101], &[]);
        stopped.generated[0].stop = Some("eos".into());
        execute(&mut ledger, &prompt, true, vec![stopped]);
        for phase in [Phase::Prefill, Phase::Decode, Phase::Verify, Phase::Replay] {
            rejected(prepare(&ledger, &rows(0, phase, 2, 1, &[101])));
        }
        rejected(ledger.prepare_settlement(&Identity::from_owner(&prompt[0]), 1, 0, &[]));
        assert!(
            ledger
                .prepare_release(&Identity::from_owner(&prompt[0]))
                .is_ok()
        );
        let fresh = StageFrontiers::default();
        rejected(prepare(&fresh, &rows(1, Phase::Replay, 0, 0, &[101, 201])));
    }

    #[test]
    fn shutdown_counts_stopped_kv_until_release_but_not_empty_revision_cells() {
        let mut ledger = StageFrontiers::default();
        assert_eq!(ledger.active_slots(), 0);
        let prompt = rows(0, Phase::Prefill, 0, 0, &[5, 6]);
        let mut stopped = decision(&prompt, &[101], &[]);
        stopped.generated[0].stop = Some("eos".into());
        execute(&mut ledger, &prompt, true, vec![stopped]);
        assert!(matches!(
            ledger.cells[&0].value.as_ref().unwrap().mode,
            Mode::Stopped
        ));
        assert_eq!(ledger.active_slots(), 1, "stopped generation still owns KV");
        let release = ledger
            .prepare_release(&Identity::from_owner(&prompt[0]))
            .unwrap();
        assert_eq!(
            ledger.active_slots(),
            1,
            "preparing release is not releasing KV"
        );
        ledger.commit(release).unwrap();
        assert_eq!(
            ledger.cells.len(),
            1,
            "the ABA revision tombstone survives release"
        );
        assert!(ledger.cells[&0].value.is_none());
        assert_eq!(ledger.active_slots(), 0);
    }
}

fn advance_generation(next: &mut Frontier, rows: &[RowOwner], end: u32) -> Result<(), String> {
    let first = &rows[0];
    if rows.iter().any(|row| !row.output)
        || (first.phase == Phase::Decode && rows.len() != 1)
        || rows.len() > next.max_tokens.saturating_sub(next.generated) as usize
    {
        return Err("frontier decode/Verify row width, output or budget is invalid".into());
    }
    next.mode = if first.phase == Phase::Verify {
        if first.speculative_id <= next.last_round {
            return Err("frontier Verify round was already consumed".into());
        }
        next.last_round = first.speculative_id;
        Mode::Verified(VerifyWindow {
            start: first.position,
            end,
            round: first.speculative_id,
            generated: next.generated,
            first_token: first.input_token,
        })
    } else {
        Mode::Ready(Expectation::Any)
    };
    next.generated = next
        .generated
        .checked_add(rows.len() as u32)
        .ok_or("frontier generation overflow")?;
    Ok(())
}

fn apply_decision(
    next: &mut Frontier,
    rows: &[RowOwner],
    outcome: &PhysicalOutcome,
) -> Result<(), String> {
    if outcome
        .generated
        .last()
        .is_some_and(|token| token.stop.is_some())
    {
        next.mode = Mode::Stopped;
    } else if let Some(retain) = outcome.retain_from {
        let Mode::Verified(window) = &next.mode else {
            return Err("frontier rollback result has no Verify window".into());
        };
        let settlement = if outcome.replay_tokens.is_empty() {
            Settlement::Direct {
                retain,
                token: outcome
                    .generated
                    .last()
                    .expect("validated direct decision emits tokens")
                    .token,
            }
        } else {
            Settlement::Checkpoint {
                retain,
                tokens: Arc::from(outcome.replay_tokens.as_slice()),
            }
        };
        next.mode = Mode::AwaitSettlement(window.clone(), settlement);
    } else {
        next.mode = Mode::Ready(Expectation::Exact(Arc::from(outcome.proposal.as_slice())));
    }
    // The predicted written end belongs to all submitted rows, including
    // tentative Verify rows. SETTLE, not the sample, moves the local KV end.
    let first = &rows[0];
    next.generated = first
        .generated_tokens
        .checked_add(outcome.generated.len() as u32)
        .ok_or("frontier generated count overflow")?;
    Ok(())
}

/// A continuation must fit one physical invocation. Token budget alone does
/// not authorize a wider Verify/Replay group; silently truncating it would
/// change the native decision. Empty terminal continuations need no capacity.
pub(crate) fn validate_continuation_width(
    proposal: &[i32],
    replay_tokens: &[i32],
    physical_capacity: usize,
) -> Result<(), String> {
    if proposal.len() > physical_capacity || replay_tokens.len() > physical_capacity {
        return Err("proposal or replay continuation exceeds atomic physical capacity".into());
    }
    Ok(())
}

pub(crate) fn validate_outcome(
    owner: &RowOwner,
    outcome: &PhysicalOutcome,
    rows: usize,
) -> Result<(), String> {
    let phase = owner.phase;
    let count = outcome.generated.len();
    let remaining = owner
        .max_tokens
        .checked_sub(owner.generated_tokens)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| "decision has no remaining token budget".to_owned())?;
    if count > remaining as usize {
        return Err("decision exceeds the request token budget".into());
    }
    let mut stopped = false;
    for (index, token) in outcome.generated.iter().enumerate() {
        let offset = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1));
        if offset.and_then(|offset| owner.position.checked_add(offset)) != Some(token.position)
            || token.token < 0
            || stopped
            || token
                .stop
                .as_deref()
                .is_some_and(|reason| !matches!(reason, "stop" | "eos" | "length"))
        {
            return Err("generated token position, identity or stop ordering is invalid".into());
        }
        stopped = token.stop.is_some();
        if token.stop.as_deref() == Some("length") && index + 1 != remaining as usize {
            return Err("length stop precedes the token budget".into());
        }
    }
    if count == remaining as usize && !stopped {
        return Err("token budget exhausted without a terminal decision".into());
    }
    if outcome
        .proposal
        .iter()
        .chain(&outcome.replay_tokens)
        .any(|token| *token < 0)
    {
        return Err("continuation contains an invalid token".into());
    }
    if stopped
        && (count == 0
            || outcome.retain_from.is_some()
            || !outcome.proposal.is_empty()
            || !outcome.replay_tokens.is_empty()
            || outcome.replay_position != 0)
    {
        return Err("stopped decision contains a continuation".into());
    }

    match phase {
        Phase::Prefill | Phase::Decode => {
            if count != 1
                || outcome.retain_from.is_some()
                || !outcome.replay_tokens.is_empty()
                || outcome.replay_position != 0
            {
                return Err(
                    "ordinary output must generate exactly one token without rollback".into(),
                );
            }
        }
        Phase::Verify => {
            if count > rows {
                return Err("verification generated more tokens than its atomic group".into());
            }
            if !outcome.replay_tokens.is_empty() {
                // Full/recurrent checkpoint restore has not emitted accepted
                // tokens yet. The replay begins with the original input token,
                // followed by the accepted tokens (not necessarily the draft).
                let replay_count = u32::try_from(outcome.replay_tokens.len()).ok();
                if count != 0
                    || !outcome.proposal.is_empty()
                    || outcome.replay_tokens.len() < 2
                    || outcome.replay_tokens.len() > rows
                    || outcome.replay_tokens.first() != Some(&owner.input_token)
                    || outcome.replay_position != owner.position
                    || replay_count.and_then(|count| owner.position.checked_add(count))
                        != outcome.retain_from
                {
                    return Err("checkpoint replay decision is inconsistent".into());
                }
                return Ok(());
            }
            if count == 0 || outcome.replay_position != 0 {
                return Err("verification without checkpoint replay must emit tokens".into());
            }
            if let Some(retain) = outcome.retain_from {
                if stopped
                    || count >= rows
                    || !outcome.proposal.is_empty()
                    || owner.position.checked_add(count as u32) != Some(retain)
                {
                    return Err("direct verification rollback boundary is inconsistent".into());
                }
                return Ok(());
            }
            if !stopped && count != rows {
                return Err("partial verification acceptance omitted its rollback".into());
            }
        }
        Phase::Replay => {
            if count == 0
                || count > rows
                || (!stopped && count != rows)
                || outcome.retain_from.is_some()
                || !outcome.replay_tokens.is_empty()
                || outcome.replay_position != 0
            {
                return Err(
                    "replay must finish its accepted group without another rollback".into(),
                );
            }
        }
    }
    if !stopped
        && (outcome.proposal.first() != outcome.generated.last().map(|token| &token.token)
            || outcome.proposal.is_empty()
            || outcome.proposal.len() > remaining as usize - count)
    {
        return Err("continuing decision has an invalid proposal or budget".into());
    }
    Ok(())
}
