//! Issued authority and terminal receipts. No transport or engine calls live here.
//! A logical request fragment can span several physical executions. Arrival is
//! recorded independently from applying that fragment's request transition.
use crate::v2::capsule::{CapsuleSet, Invocation, PhysicalCapsule, PhysicalOutcome, RowOwner};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

const RECEIPT_BYTES: usize = 64 * 1024 * 1024;
const RECEIPT_COUNT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Authority {
    invocation: Invocation,
    owners: Vec<RowOwner>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Batch {
    expected: BTreeMap<u64, Arc<Authority>>,
    received: BTreeMap<u64, Arc<PhysicalCapsule>>,
    pending: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FlightLedger {
    batches: BTreeMap<u64, Batch>,
    execution_batch: BTreeMap<u64, u64>,
    // IDs from the native context are monotonically allocated. A high-water
    // mark prevents reuse even after a bounded duplicate receipt is evicted.
    execution_high_water: u64,
    receipts: BTreeMap<u64, Arc<PhysicalCapsule>>,
    receipt_order: VecDeque<(u64, usize)>,
    receipt_bytes: usize,
}

pub(crate) struct SettledFragment {
    pub key: String,
    pub owners: Vec<RowOwner>,
    pub outcome: Option<(RowOwner, PhysicalOutcome)>,
}

pub(crate) struct ReturnPlan {
    updates: BTreeMap<u64, Batch>,
    pub fragments: Vec<SettledFragment>,
    pub decisions: Vec<SettledFragment>,
}

impl FlightLedger {
    /// Live issued work only; terminal duplicate receipts are not pending work.
    pub fn active_counts(&self) -> (usize, usize) {
        (self.batches.len(), self.execution_batch.len())
    }

    /// Authority registration is all-or-none. Tensor bytes are deliberately
    /// not retained: they change at every cut and do not identify issued rows.
    pub fn register(
        &mut self,
        ordinal: u64,
        generation: u64,
        set: &CapsuleSet,
    ) -> Result<(), String> {
        if ordinal == 0 || self.batches.contains_key(&ordinal) || set.0.is_empty() {
            return Err("invalid or reused logical batch identity".into());
        }
        let mut batch = Batch {
            expected: BTreeMap::new(),
            received: BTreeMap::new(),
            pending: BTreeSet::new(),
        };
        let mut rows = BTreeSet::new();
        let mut high = self.execution_high_water;
        let mut session: Option<&str> = None;
        for capsule in &set.0 {
            if capsule.terminal
                || !capsule.outcomes.is_empty()
                || capsule.execution_id <= self.execution_high_water
                || batch.expected.contains_key(&capsule.execution_id)
                || capsule.owners.is_empty()
            {
                return Err("invalid or reused physical issue identity".into());
            }
            validate_membership(capsule)?;
            for owner in &capsule.owners {
                if owner.load_generation != generation
                    || generation == 0
                    || owner.sequence_key
                        != super::state::request_key(&owner.session_id, &owner.request_id)
                    || session.is_some_and(|id| id != owner.session_id)
                    || !rows.insert((owner.sequence_key.clone(), owner.position))
                {
                    return Err("invalid or duplicate issued row ownership".into());
                }
                session = Some(&owner.session_id);
                batch.pending.insert(owner.sequence_key.clone());
            }
            high = high.max(capsule.execution_id);
            batch.expected.insert(
                capsule.execution_id,
                Arc::new(Authority {
                    invocation: capsule.invocation.clone(),
                    owners: capsule.owners.clone(),
                }),
            );
        }
        for id in batch.expected.keys() {
            self.execution_batch.insert(*id, ordinal);
        }
        self.batches.insert(ordinal, batch);
        self.execution_high_water = high;
        Ok(())
    }

    /// Validate the complete event against issued authority before writing any
    /// receipt. Only touched batches are copied; tensor bytes/history are not.
    pub fn prepare_return(&self, set: &CapsuleSet) -> Result<ReturnPlan, String> {
        let mut updates = BTreeMap::<u64, Batch>::new();
        let mut event_ids = BTreeSet::new();
        for capsule in &set.0 {
            if !capsule.terminal || !event_ids.insert(capsule.execution_id) {
                return Err("tail must contain unique terminal executions".into());
            }
            if let Some(receipt) = self.receipts.get(&capsule.execution_id) {
                if receipt.as_ref() != capsule {
                    return Err("terminal receipt conflicts with an earlier result".into());
                }
                continue;
            }
            let ordinal = *self
                .execution_batch
                .get(&capsule.execution_id)
                .ok_or_else(|| "tail execution was not issued or its receipt expired".to_owned())?;
            let original = self
                .batches
                .get(&ordinal)
                .expect("execution belongs to a batch");
            let authority = original
                .expected
                .get(&capsule.execution_id)
                .expect("issued authority");
            if authority.invocation != capsule.invocation || authority.owners != capsule.owners {
                return Err("tail membership differs from issued authority".into());
            }
            let batch = updates.entry(ordinal).or_insert_with(|| original.clone());
            if let Some(receipt) = batch.received.get(&capsule.execution_id) {
                if receipt.as_ref() != capsule {
                    return Err("terminal receipt conflicts with an earlier result".into());
                }
            } else {
                batch
                    .received
                    .insert(capsule.execution_id, Arc::new(capsule.clone()));
            }
        }

        // A later logical fragment can arrive first, but cannot advance a
        // request past an earlier one. Other requests remain independent.
        let mut blocked = BTreeSet::new();
        let mut fragments = Vec::new();
        let mut decisions = Vec::new();
        for (ordinal, original) in &self.batches {
            let candidate = updates.get(ordinal).unwrap_or(original);
            let mut completed = Vec::new();
            for key in &candidate.pending {
                let members: Vec<_> = candidate
                    .expected
                    .iter()
                    .filter(|(_, authority)| {
                        authority
                            .owners
                            .iter()
                            .any(|owner| &owner.sequence_key == key)
                    })
                    .map(|(id, _)| *id)
                    .collect();
                let mut owners = Vec::new();
                let mut outcome = None;
                for id in &members {
                    owners.extend(
                        candidate.expected[id]
                            .owners
                            .iter()
                            .filter(|owner| &owner.sequence_key == key)
                            .cloned(),
                    );
                    let Some(capsule) = candidate.received.get(id) else {
                        continue;
                    };
                    for decision in &capsule.outcomes {
                        let owner = capsule
                            .owners
                            .get(decision.owner_index as usize)
                            .ok_or_else(|| "tail outcome owner is missing".to_owned())?;
                        if &owner.sequence_key == key
                            && outcome.replace((owner.clone(), decision.clone())).is_some()
                        {
                            return Err("tail returned duplicate request decisions".into());
                        }
                    }
                }
                owners.sort_by_key(|owner| owner.position);
                if outcome.is_some() {
                    decisions.push(SettledFragment {
                        key: key.clone(),
                        owners: owners.clone(),
                        outcome: outcome.clone(),
                    });
                }
                if blocked.contains(key)
                    || members
                        .iter()
                        .any(|id| !candidate.received.contains_key(id))
                {
                    blocked.insert(key.clone());
                    continue;
                }
                fragments.push(SettledFragment {
                    key: key.clone(),
                    owners,
                    outcome,
                });
                completed.push(key.clone());
            }
            if !completed.is_empty() {
                let batch = updates.entry(*ordinal).or_insert_with(|| original.clone());
                for key in completed {
                    batch.pending.remove(&key);
                }
            }
        }
        Ok(ReturnPlan {
            updates,
            fragments,
            decisions,
        })
    }

    /// Caller commits this only after every request/outcome/effect was validated.
    pub fn commit_return(&mut self, plan: ReturnPlan) {
        for (ordinal, batch) in plan.updates {
            if batch.pending.is_empty() {
                for (id, receipt) in batch.received {
                    self.execution_batch.remove(&id);
                    let size = CapsuleSet(vec![receipt.as_ref().clone()])
                        .encode()
                        .expect("validated terminal receipt")
                        .len();
                    if size <= RECEIPT_BYTES {
                        self.receipts.insert(id, receipt);
                        self.receipt_order.push_back((id, size));
                        self.receipt_bytes += size;
                    }
                }
                self.batches.remove(&ordinal);
            } else {
                self.batches.insert(ordinal, batch);
            }
        }
        while self.receipt_bytes > RECEIPT_BYTES || self.receipt_order.len() > RECEIPT_COUNT {
            if let Some((id, size)) = self.receipt_order.pop_front() {
                self.receipts.remove(&id);
                self.receipt_bytes -= size;
            }
        }
    }

    pub fn open_batches(&self) -> BTreeMap<u64, BTreeSet<u64>> {
        self.batches
            .iter()
            .map(|(id, batch)| (*id, batch.expected.keys().copied().collect()))
            .collect()
    }

    /// Independent identity-derived counts, never copied from request counters.
    pub fn outstanding(&self, prepared: Option<&ReturnPlan>) -> BTreeMap<String, u32> {
        let mut counts = BTreeMap::new();
        for (ordinal, batch) in &self.batches {
            let batch = prepared
                .and_then(|plan| plan.updates.get(ordinal))
                .unwrap_or(batch);
            for key in &batch.pending {
                *counts.entry(key.clone()).or_insert(0) += 1;
            }
        }
        counts
    }
}

/// Rows currently have one sequence owner; never let invocation metadata name
/// a different sequence from the owner authorized by the logical submission.
pub(crate) fn validate_membership(capsule: &PhysicalCapsule) -> Result<(), String> {
    let rows = capsule.owners.len();
    let invocation = &capsule.invocation;
    if invocation.n_pos == 0
        || invocation.n_pos > 4
        || invocation.positions.len() != rows * invocation.n_pos as usize
        || invocation.output.len() != rows
        || invocation.sequence_counts.len() != rows
        || invocation.sequence_ids.len() != rows
        || invocation.sequence_counts.iter().any(|count| *count != 1)
    {
        return Err("physical invocation has unsupported ownership shape".into());
    }
    for (index, owner) in capsule.owners.iter().enumerate() {
        if i32::try_from(owner.sequence_id).ok() != Some(invocation.sequence_ids[index])
            || i32::try_from(owner.position).ok() != Some(invocation.positions[index])
            || owner.output != invocation.output[index]
        {
            return Err("physical invocation does not match its row owners".into());
        }
    }
    let mut index = 0;
    while index < rows {
        let first = &capsule.owners[index];
        if !matches!(
            first.phase,
            crate::v2::Phase::Verify | crate::v2::Phase::Replay
        ) {
            index += 1;
            continue;
        }
        let count = first.speculative_count as usize;
        if first.speculative_id == 0
            || first.speculative_index != 0
            || count == 0
            || count > rows - index
        {
            return Err("atomic group was split across physical executions".into());
        }
        for offset in 0..count {
            let owner = &capsule.owners[index + offset];
            if owner.sequence_key != first.sequence_key
                || owner.phase != first.phase
                || owner.speculative_id != first.speculative_id
                || owner.speculative_count != first.speculative_count
                || owner.speculative_index as usize != offset
                || first.position.checked_add(offset as u32) != Some(owner.position)
            {
                return Err("atomic physical group was interleaved or reordered".into());
            }
        }
        index += count;
    }
    Ok(())
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    use crate::v2::{
        Phase,
        capsule::{Tensor, TensorDescriptor},
    };

    #[test]
    fn shutdown_counts_open_flights_but_not_retired_terminal_receipts() {
        let mut ledger = FlightLedger::default();
        assert_eq!(ledger.active_counts(), (0, 0));
        let capsules: Vec<_> = (0..2)
            .map(|position| PhysicalCapsule {
                execution_id: u64::from(position) + 1,
                terminal: false,
                invocation: Invocation {
                    flags: 0,
                    n_seq_tokens: 1,
                    n_seqs: 1,
                    n_seqs_unq: 1,
                    n_pos: 1,
                    positions: vec![position as i32],
                    sequence_counts: vec![1],
                    sequence_ids: vec![0],
                    output: vec![false],
                },
                owners: vec![RowOwner {
                    load_generation: 1,
                    incarnation: 1,
                    request_id: "request".into(),
                    sequence_key: "session\0request".into(),
                    session_id: "session".into(),
                    reply: "reply".into(),
                    sequence_id: 0,
                    phase: Phase::Prefill,
                    position,
                    max_tokens: 16,
                    generated_tokens: 0,
                    output: false,
                    input_token: 7,
                    speculative_id: 0,
                    speculative_index: 0,
                    speculative_count: 0,
                    options: String::new(),
                }],
                tensors: vec![Tensor {
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
                }],
                outcomes: vec![],
            })
            .collect();
        ledger
            .register(1, 1, &CapsuleSet(capsules.clone()))
            .unwrap();
        assert_eq!(ledger.active_counts(), (1, 2));
        for (index, mut capsule) in capsules.into_iter().enumerate() {
            capsule.terminal = true;
            capsule.tensors.clear();
            let returned = CapsuleSet(vec![capsule]);
            let plan = ledger.prepare_return(&returned).unwrap();
            assert_eq!(
                ledger.active_counts(),
                (1, 2),
                "preparation never retires authority"
            );
            assert_eq!(plan.fragments.len(), index);
            ledger.commit_return(plan);
            if index == 0 {
                assert_eq!(
                    ledger.active_counts(),
                    (1, 2),
                    "partial return keeps the logical fragment open"
                );
            } else {
                assert_eq!(ledger.active_counts(), (0, 0));
                assert_eq!(ledger.receipts.len(), 2);
                assert!(ledger.receipt_bytes > 0);
                let duplicate = ledger.prepare_return(&returned).unwrap();
                assert!(duplicate.fragments.is_empty());
                ledger.commit_return(duplicate);
                assert_eq!(
                    ledger.active_counts(),
                    (0, 0),
                    "exact replay does not reopen work"
                );
            }
        }
    }
}

/// Native splitting must conserve every logical owner exactly once, including
/// token, phase, options and all identity fields, not merely the row count.
pub(crate) fn validate_split(
    logical: &crate::v2::logical::LogicalBatch,
    physical: &CapsuleSet,
) -> Result<(), String> {
    let mut expected = BTreeMap::new();
    for row in &logical.0 {
        if row.token != row.owner.input_token
            || expected
                .insert(
                    (row.owner.sequence_key.clone(), row.owner.position),
                    &row.owner,
                )
                .is_some()
        {
            return Err("logical issue contains duplicate or inconsistent rows".into());
        }
    }
    for capsule in &physical.0 {
        validate_membership(capsule)?;
        for owner in &capsule.owners {
            if expected.remove(&(owner.sequence_key.clone(), owner.position)) != Some(owner) {
                return Err("physical split changed, duplicated or invented a logical row".into());
            }
        }
    }
    if !expected.is_empty() {
        return Err("physical split omitted logical rows".into());
    }
    Ok(())
}

/// Validate the plan against resident authority BEFORE native execution. A
/// well-formed wire row is not sufficient: it may name the wrong admitted
/// sequence, token range, reply or speculative round.
pub(crate) fn validate_planned_request(
    request: &super::state::RequestState,
    rows: &[&crate::v2::logical::LogicalRow],
) -> Result<(), String> {
    use crate::v2::Phase;
    let first = rows
        .first()
        .ok_or_else(|| "planned fragment has no rows".to_owned())?;
    let phase = first.owner.phase;
    let atomic = matches!(phase, Phase::Verify | Phase::Replay);
    let key = super::state::request_key(&request.command.session_id, &request.command.request_id);
    if request.generated >= request.command.max_tokens {
        return Err("planned fragment has no remaining token budget".into());
    }
    let (position, tokens, speculative_id) = if phase == Phase::Prefill {
        let position =
            u32::try_from(request.prompt_issued).map_err(|_| "prompt position overflow")?;
        let end = request
            .prompt_issued
            .checked_add(rows.len())
            .ok_or("prompt range overflow")?;
        let tokens = request
            .command
            .tokens
            .get(request.prompt_issued..end)
            .ok_or("planned fragment exceeds remaining prompt")?;
        (position, tokens, 0)
    } else {
        let ready = request
            .ready
            .as_ref()
            .ok_or("planned fragment is not ready")?;
        if ready.phase != phase
            || ready.tokens.len() != rows.len()
            || (phase == Phase::Decode && rows.len() != 1)
        {
            return Err("planned fragment split or changed ready work".into());
        }
        (
            ready.position,
            ready.tokens.as_slice(),
            ready.speculative_id,
        )
    };
    let count = u32::try_from(rows.len()).map_err(|_| "planned fragment row count overflow")?;
    for (index, (row, token)) in rows.iter().zip(tokens).enumerate() {
        let owner = &row.owner;
        let offset = u32::try_from(index).map_err(|_| "planned fragment offset overflow")?;
        let output = match phase {
            Phase::Prefill => request.prompt_issued + index + 1 == request.command.tokens.len(),
            Phase::Replay => false,
            _ => true,
        };
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
            || owner.phase != phase
            || position.checked_add(offset) != Some(owner.position)
            || owner.input_token != *token
            || row.token != *token
            || owner.output != output
            || (atomic
                && (speculative_id == 0
                    || owner.speculative_id != speculative_id
                    || owner.speculative_index != offset
                    || owner.speculative_count != count))
            || (!atomic
                && (owner.speculative_id != 0
                    || owner.speculative_index != 0
                    || owner.speculative_count != 0))
        {
            return Err("planned row differs from resident request authority".into());
        }
    }
    Ok(())
}
