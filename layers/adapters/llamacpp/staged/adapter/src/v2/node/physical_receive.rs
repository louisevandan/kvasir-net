//! Worker/load-local PHYSICAL execution receipts, not KV prefix ordering.
//!
//! Within a bound worker load, (SESSION.first endpoint, execution ID) binds every
//! canonical input byte, including session and request incarnation. Only
//! retained receipts can be replayed. The configured issuer, not capsule data,
//! is authoritative. Distinct heads have independent numeric receive windows.
//! Expired/uncertain IDs are never executed again. The sparse numeric receive
//! window permits unseen out-of-order IDs; advancing its floor deliberately
//! expires older IDs, including gaps that never arrived. This is NOT a durable
//! restart contract, an authorization of per-sequence row order, or a bound on
//! active requests, encoding temporaries, pinned plans, transport, or total RSS.
//! Eviction provides at-most-once execution with fail-closed expiration; it does
//! not promise lossless retry after expiration. Recreating the worker recreates
//! this ledger even if the hosting process is unchanged. Issuer identities are
//! never automatically forgotten: their count and all seen executions have
//! load-wide bounds, separate from the shared replay-byte/count bounds.

use super::flight::validate_membership;
use crate::v2::capsule::{CapsuleSet, Invocation, PhysicalCapsule, RowOwner};
use p4_protocol::event::Endpoint;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

const CACHE_BYTES: usize = 64 * 1024 * 1024;
const CACHE_COUNT: usize = 4096;
const RECEIVE_WINDOW: u64 = 65_536;
const ISSUER_COUNT: usize = 1024;

type ExecutionKey = (usize, u64);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Limits {
    bytes: usize,
    count: usize,
    window: u64,
    seen: usize,
    issuers: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Namespace {
    issuer: Endpoint,
    highest: u64,
    floor: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Seen {
    Running {
        attempt: u64,
        input: Arc<[u8]>,
    },
    Completed {
        input: Arc<[u8]>,
        response: Arc<[u8]>,
    },
    Expired,
    Uncertain,
}

impl Seen {
    fn retained_bytes(&self) -> usize {
        match self {
            Self::Completed { input, response } => input.len() + response.len(),
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    generation: u64,
    revision: u64,
    next_attempt: u64,
    active_attempt: Option<u64>,
    fenced: bool,
    namespaces: Vec<Namespace>,
    seen: BTreeMap<ExecutionKey, Seen>,
    cache_order: VecDeque<ExecutionKey>,
    cache_bytes: usize,
    limits: Limits,
}

#[derive(Debug)]
pub(crate) struct PhysicalReceiveLedger {
    origin: Arc<()>,
    state: State,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ReceiveWorkStatus {
    pub running: usize,
    pub uncertain: usize,
    pub active_attempt: bool,
    pub fenced: bool,
}

impl ReceiveWorkStatus {
    pub fn has_pending(self) -> bool {
        self.running != 0 || self.uncertain != 0 || self.active_attempt || self.fenced
    }
}

#[derive(Debug)]
struct Authority {
    invocation: Invocation,
    owners: Vec<RowOwner>,
}

#[derive(Debug)]
enum Planned {
    Fresh {
        id: u64,
        input: Arc<[u8]>,
        authority: Authority,
    },
    Replay {
        id: u64,
        response: Arc<[u8]>,
    },
}

#[derive(Debug)]
pub(crate) struct ReceivePlan {
    origin: Arc<()>,
    generation: u64,
    revision: u64,
    issuer: Endpoint,
    namespace: usize,
    highest: u64,
    floor: u64,
    entries: Vec<Planned>,
    fresh_indices: Vec<usize>,
}

impl ReceivePlan {
    pub fn fresh_indices(&self) -> &[usize] {
        &self.fresh_indices
    }

    #[cfg(test)]
    pub fn is_replay_only(&self) -> bool {
        self.fresh_indices.is_empty()
    }
}

#[derive(Debug)]
pub(crate) struct ReceiveAttempt {
    plan: ReceivePlan,
    id: u64,
    revision: u64,
}

#[cfg(test)]
impl ReceiveAttempt {
    pub fn fresh_indices(&self) -> &[usize] {
        self.plan.fresh_indices()
    }

    pub fn is_replay_only(&self) -> bool {
        self.plan.is_replay_only()
    }
}

impl Default for PhysicalReceiveLedger {
    fn default() -> Self {
        Self {
            origin: Arc::new(()),
            state: State {
                generation: 0,
                revision: 0,
                next_attempt: 1,
                active_attempt: None,
                fenced: false,
                namespaces: Vec::new(),
                seen: BTreeMap::new(),
                cache_order: VecDeque::new(),
                cache_bytes: 0,
                limits: Limits {
                    bytes: CACHE_BYTES,
                    count: CACHE_COUNT,
                    window: RECEIVE_WINDOW,
                    seen: RECEIVE_WINDOW as usize,
                    issuers: ISSUER_COUNT,
                },
            },
        }
    }
}

impl PhysicalReceiveLedger {
    /// Inspect only at lifecycle boundaries, not on the hot scheduling path.
    /// Completed/Expired retry history is not unfinished native execution.
    pub fn shutdown_status(&self) -> ReceiveWorkStatus {
        ReceiveWorkStatus {
            running: self
                .state
                .seen
                .values()
                .filter(|state| matches!(state, Seen::Running { .. }))
                .count(),
            uncertain: self
                .state
                .seen
                .values()
                .filter(|state| matches!(state, Seen::Uncertain))
                .count(),
            active_attempt: self.state.active_attempt.is_some(),
            fenced: self.state.fenced,
        }
    }

    /// Install only after the worker has successfully bound this native load.
    /// A fresh ledger cannot itself establish cross-worker generation freshness.
    pub fn new(generation: u64) -> Result<Self, String> {
        if generation == 0 {
            return Err("physical receive load generation is zero".into());
        }
        let mut ledger = Self::default();
        ledger.state.generation = generation;
        Ok(ledger)
    }

    /// No state changes, including eviction and the receive floor. A later bad
    /// capsule cannot burn an earlier valid ID or discard a retained receipt.
    pub fn prepare(&self, issuer: &Endpoint, input: &CapsuleSet) -> Result<ReceivePlan, String> {
        let state = &self.state;
        if state.generation == 0 || state.fenced || state.active_attempt.is_some() {
            return Err("physical receive ledger is unbound, busy, or uncertain".into());
        }
        if input.0.is_empty() || input.0.len() as u64 > state.limits.window {
            return Err("physical receive event is empty or exceeds its ID window".into());
        }
        if !matches!(issuer, Endpoint::Node { agent, node, generation }
            if *generation != 0 && !node.is_empty() && node.len() <= 4096
                && !agent.host.is_empty() && agent.host.len() <= 4096 && agent.port != 0)
        {
            return Err("physical receive issuer is not a bound node endpoint".into());
        }
        let namespace = state
            .namespaces
            .iter()
            .position(|value| &value.issuer == issuer)
            .unwrap_or(state.namespaces.len());
        if namespace == state.namespaces.len() && namespace >= state.limits.issuers {
            return Err("physical receive issuer identity capacity exhausted".into());
        }
        let current = state.namespaces.get(namespace);
        state
            .revision
            .checked_add(2)
            .ok_or("physical receive revision exhausted")?;
        state
            .next_attempt
            .checked_add(1)
            .ok_or("physical receive attempt identity exhausted")?;
        let highest = input
            .0
            .iter()
            .map(|capsule| capsule.execution_id)
            .max()
            .unwrap()
            .max(current.map_or(0, |value| value.highest));
        let floor = highest
            .saturating_sub(state.limits.window)
            .max(current.map_or(0, |value| value.floor));
        let mut ids = BTreeSet::new();
        let mut rows = BTreeSet::new();
        let mut session = None;
        let mut entries = Vec::with_capacity(input.0.len());
        let mut fresh_indices = Vec::new();
        for (index, capsule) in input.0.iter().enumerate() {
            capsule
                .validate()
                .map_err(|error| format!("invalid physical receive capsule: {error:?}"))?;
            validate_membership(capsule)?;
            let id = capsule.execution_id;
            if capsule.terminal || !capsule.outcomes.is_empty() || !ids.insert(id) {
                return Err("physical receive requires unique nonterminal executions".into());
            }
            // Validate every member against the prospective window, not an
            // incrementally advanced maximum that depends on event ordering.
            if id <= floor {
                return Err("physical receive execution is outside its retained ID window".into());
            }
            for owner in &capsule.owners {
                if owner.load_generation != state.generation
                    || session.is_some_and(|value| value != owner.session_id)
                    || !rows.insert((owner.sequence_id, owner.incarnation, owner.position))
                {
                    return Err("physical receive mixes loads/sessions or duplicates rows".into());
                }
                session = Some(owner.session_id.as_str());
            }
            let bytes = encode_one(capsule)?;
            match state.seen.get(&(namespace, id)) {
                Some(Seen::Completed { input, response }) => {
                    if input.as_ref() != bytes.as_ref() {
                        return Err(
                            "physical receive execution conflicts with its canonical input".into(),
                        );
                    }
                    entries.push(Planned::Replay {
                        id,
                        response: Arc::clone(response),
                    });
                }
                Some(Seen::Running { .. } | Seen::Uncertain) => {
                    return Err("physical receive execution is running or uncertain".into());
                }
                Some(Seen::Expired) => {
                    return Err("physical receive execution receipt has expired".into());
                }
                None => {
                    fresh_indices.push(index);
                    entries.push(Planned::Fresh {
                        id,
                        input: bytes,
                        authority: Authority {
                            invocation: capsule.invocation.clone(),
                            owners: capsule.owners.clone(),
                        },
                    });
                }
            }
        }
        let obsolete = state
            .seen
            .range((namespace, 0)..=(namespace, floor))
            .count();
        if state
            .seen
            .len()
            .checked_sub(obsolete)
            .and_then(|count| count.checked_add(fresh_indices.len()))
            .is_none_or(|count| count > state.limits.seen)
        {
            return Err("physical receive aggregate seen identity capacity exhausted".into());
        }
        Ok(ReceivePlan {
            origin: Arc::clone(&self.origin),
            generation: state.generation,
            revision: state.revision,
            issuer: issuer.clone(),
            namespace,
            highest,
            floor,
            entries,
            fresh_indices,
        })
    }

    /// This is the last transition before native execution. It burns all fresh
    /// IDs together. Only one synchronous native attempt may own this ledger.
    pub fn begin(&mut self, plan: ReceivePlan) -> Result<ReceiveAttempt, String> {
        let state = &self.state;
        if !Arc::ptr_eq(&self.origin, &plan.origin)
            || plan.generation != state.generation
            || plan.revision != state.revision
            || state.generation == 0
            || state.fenced
            || state.active_attempt.is_some()
        {
            return Err("physical receive plan is foreign, stale, busy, or uncertain".into());
        }
        let revision = state
            .revision
            .checked_add(1)
            .ok_or("physical receive revision exhausted")?;
        let next_attempt = state
            .next_attempt
            .checked_add(1)
            .ok_or("physical receive attempt identity exhausted")?;
        let id = state.next_attempt;
        // Copy only the bounded index and Arc handles, never retained payloads.
        let mut candidate = state.clone();
        let obsolete: Vec<_> = candidate
            .seen
            .range((plan.namespace, 0)..=(plan.namespace, plan.floor))
            .map(|(id, _)| *id)
            .collect();
        for old in obsolete {
            let entry = candidate.seen.remove(&old).expect("existing receive entry");
            if matches!(entry, Seen::Running { .. } | Seen::Uncertain) {
                return Err("physical receive window cannot evict an unfinished execution".into());
            }
            candidate.cache_bytes = candidate
                .cache_bytes
                .checked_sub(entry.retained_bytes())
                .ok_or("physical receive cache accounting underflow")?;
        }
        candidate
            .cache_order
            .retain(|(issuer, id)| *issuer != plan.namespace || *id > plan.floor);
        for entry in &plan.entries {
            if let Planned::Fresh {
                id: execution,
                input,
                ..
            } = entry
                && candidate
                    .seen
                    .insert(
                        (plan.namespace, *execution),
                        Seen::Running {
                            attempt: id,
                            input: Arc::clone(input),
                        },
                    )
                    .is_some()
            {
                return Err("physical receive plan reuses an existing execution".into());
            }
        }
        let namespace = Namespace {
            issuer: plan.issuer.clone(),
            highest: plan.highest,
            floor: plan.floor,
        };
        if plan.namespace == candidate.namespaces.len() {
            candidate.namespaces.push(namespace);
        } else {
            candidate.namespaces[plan.namespace] = namespace;
        }
        candidate.revision = revision;
        candidate.next_attempt = next_attempt;
        candidate.active_attempt = Some(id);
        self.state = candidate;
        Ok(ReceiveAttempt { plan, id, revision })
    }

    /// Validate and encode the entire fresh result before committing any cache
    /// entry. Return cached/fresh capsules in the original incoming order.
    /// A response too large to retain is delivered now and becomes Expired;
    /// cache capacity is not an arbitrary rejection of otherwise valid work.
    pub fn complete(
        &mut self,
        attempt: ReceiveAttempt,
        fresh: &CapsuleSet,
        terminal: bool,
    ) -> Result<CapsuleSet, String> {
        self.validate_attempt(&attempt)?;
        let prepared = assemble(&attempt, fresh, terminal);
        let (assembled, receipts) = match prepared {
            Ok(value) => value,
            Err(error) => {
                self.fence_attempt(&attempt);
                return Err(error);
            }
        };
        let mut candidate = self.state.clone();
        for (id, input, response) in receipts {
            retain_receipt(
                &mut candidate,
                (attempt.plan.namespace, id),
                input,
                response,
            );
        }
        candidate.active_attempt = None;
        candidate.revision = candidate
            .revision
            .checked_add(1)
            .expect("prepare reserved the completion revision");
        self.state = candidate;
        Ok(assembled)
    }

    /// A native error or lost response does not prove that it did no work.
    /// There is no automatic retry or reconciliation in this in-memory slice.
    pub fn mark_uncertain(&mut self, attempt: ReceiveAttempt) -> Result<(), String> {
        self.validate_attempt(&attempt)?;
        self.fence_attempt(&attempt);
        Ok(())
    }

    fn validate_attempt(&self, attempt: &ReceiveAttempt) -> Result<(), String> {
        let state = &self.state;
        if !Arc::ptr_eq(&self.origin, &attempt.plan.origin)
            || attempt.plan.generation != state.generation
            || attempt.revision != state.revision
            || state.active_attempt != Some(attempt.id)
            || state.fenced
        {
            return Err("physical receive attempt is foreign, stale, or uncertain".into());
        }
        for entry in &attempt.plan.entries {
            if let Planned::Fresh { id, input, .. } = entry {
                match state.seen.get(&(attempt.plan.namespace, *id)) {
                    Some(Seen::Running {
                        attempt: active,
                        input: accepted,
                    }) if *active == attempt.id && accepted == input => {}
                    _ => return Err("physical receive attempt lost its running authority".into()),
                }
            }
        }
        Ok(())
    }

    fn fence_attempt(&mut self, attempt: &ReceiveAttempt) {
        for entry in &attempt.plan.entries {
            if let Planned::Fresh { id, .. } = entry {
                self.state
                    .seen
                    .insert((attempt.plan.namespace, *id), Seen::Uncertain);
            }
        }
        self.state.fenced = true;
        self.state.active_attempt = None;
        self.state.revision = self
            .state
            .revision
            .checked_add(1)
            .expect("prepare reserved the completion revision");
    }

    #[cfg(test)]
    pub(crate) fn with_limits(generation: u64, bytes: usize, count: usize, window: u64) -> Self {
        assert!(window > 0 && window <= RECEIVE_WINDOW && bytes <= usize::MAX / 2);
        let mut ledger = Self::new(generation).unwrap();
        ledger.state.limits = Limits {
            bytes,
            count,
            window,
            seen: window as usize,
            issuers: ISSUER_COUNT,
        };
        ledger
    }
}

type Receipt = (u64, Arc<[u8]>, Arc<[u8]>);

fn assemble(
    attempt: &ReceiveAttempt,
    fresh: &CapsuleSet,
    terminal: bool,
) -> Result<(CapsuleSet, Vec<Receipt>), String> {
    if fresh.0.len() != attempt.plan.fresh_indices.len() {
        return Err("physical receive result omitted or added an execution".into());
    }
    let mut received = fresh.0.iter();
    let mut assembled = Vec::with_capacity(attempt.plan.entries.len());
    let mut receipts = Vec::with_capacity(fresh.0.len());
    for entry in &attempt.plan.entries {
        match entry {
            Planned::Fresh {
                id,
                input,
                authority,
            } => {
                let result = received.next().expect("fresh count checked");
                result
                    .validate()
                    .map_err(|error| format!("invalid physical receive result: {error:?}"))?;
                validate_membership(result)?;
                if result.execution_id != *id
                    || result.owners != authority.owners
                    || result.invocation != authority.invocation
                    || result.terminal != terminal
                {
                    return Err(
                        "physical receive result changed its execution membership or role".into(),
                    );
                }
                let response = encode_one(result)?;
                receipts.push((*id, Arc::clone(input), response));
                assembled.push(result.clone());
            }
            Planned::Replay { id, response } => {
                let mut cached = CapsuleSet::decode(response).map_err(|error| {
                    format!("invalid physical receive cached response: {error:?}")
                })?;
                if cached.0.len() != 1
                    || cached.0[0].execution_id != *id
                    || cached.0[0].terminal != terminal
                {
                    return Err(
                        "physical receive cached response has the wrong identity or role".into(),
                    );
                }
                assembled.push(cached.0.remove(0));
            }
        }
    }
    Ok((CapsuleSet(assembled), receipts))
}

fn retain_receipt(state: &mut State, id: ExecutionKey, input: Arc<[u8]>, response: Arc<[u8]>) {
    let Some(size) = input.len().checked_add(response.len()) else {
        state.seen.insert(id, Seen::Expired);
        return;
    };
    if size > state.limits.bytes || state.limits.count == 0 {
        state.seen.insert(id, Seen::Expired);
        return;
    }
    while state.cache_order.len() >= state.limits.count
        || state
            .cache_bytes
            .checked_add(size)
            .is_none_or(|sum| sum > state.limits.bytes)
    {
        let old = state
            .cache_order
            .pop_front()
            .expect("fitting receipt can evict an existing one");
        let previous = state
            .seen
            .insert(old, Seen::Expired)
            .expect("cached receive entry");
        state.cache_bytes = state
            .cache_bytes
            .checked_sub(previous.retained_bytes())
            .expect("retained receipt was charged once");
    }
    state.cache_bytes = state
        .cache_bytes
        .checked_add(size)
        .expect("receipt budget checked");
    state.cache_order.push_back(id);
    state.seen.insert(id, Seen::Completed { input, response });
}

fn encode_one(capsule: &PhysicalCapsule) -> Result<Arc<[u8]>, String> {
    // Exact canonical bytes avoid a weak hash or handwritten cryptography.
    // The current codec requires an owned set: its extra temporary copy is
    // acknowledged separately from the strictly bounded retained cache.
    CapsuleSet(vec![capsule.clone()])
        .encode()
        .map(Arc::from)
        .map_err(|error| format!("cannot encode physical receive receipt: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::{
        Phase,
        capsule::{Tensor, TensorDescriptor},
    };

    // These exercise the actual receive transitions, not the worker/native
    // boundary. That boundary has separate tests in the worker harness.
    fn issuer() -> Endpoint {
        Endpoint::node(p4_protocol::Address::tcp("head", 1234), "head-node", 1)
    }

    fn capsule(id: u64, slot: u32) -> PhysicalCapsule {
        PhysicalCapsule {
            execution_id: id,
            terminal: false,
            invocation: Invocation {
                flags: 0,
                n_seq_tokens: 1,
                n_seqs: 1,
                n_seqs_unq: 1,
                n_pos: 1,
                positions: vec![0],
                sequence_counts: vec![1],
                sequence_ids: vec![slot as i32],
                output: vec![false],
            },
            owners: vec![RowOwner {
                load_generation: 1,
                incarnation: 1,
                request_id: format!("request-{slot}"),
                sequence_key: format!("session\0request-{slot}"),
                session_id: "session".into(),
                reply: "reply".into(),
                sequence_id: slot,
                phase: Phase::Prefill,
                position: 0,
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
                data: vec![1, 2, 3, 4],
            }],
            outcomes: vec![],
        }
    }

    fn result(input: &PhysicalCapsule) -> PhysicalCapsule {
        let mut output = input.clone();
        output.tensors[0].data[0] = 8;
        output
    }

    fn execute(ledger: &mut PhysicalReceiveLedger, input: &PhysicalCapsule) -> PhysicalCapsule {
        let plan = ledger
            .prepare(&issuer(), &CapsuleSet(vec![input.clone()]))
            .unwrap();
        assert_eq!(plan.fresh_indices(), &[0]);
        let attempt = ledger.begin(plan).unwrap();
        let expected = result(input);
        let got = ledger
            .complete(attempt, &CapsuleSet(vec![expected.clone()]), false)
            .unwrap();
        assert_eq!(got.0, vec![expected.clone()]);
        expected
    }

    #[test]
    fn shutdown_distinguishes_retry_history_running_work_and_uncertain_effects() {
        let mut ledger = PhysicalReceiveLedger::with_limits(1, 1024 * 1024, 1, 8);
        assert!(!ledger.shutdown_status().has_pending());
        execute(&mut ledger, &capsule(1, 0));
        execute(&mut ledger, &capsule(2, 1));
        assert!(matches!(ledger.state.seen[&(0, 1)], Seen::Expired));
        assert!(matches!(ledger.state.seen[&(0, 2)], Seen::Completed { .. }));
        assert!(ledger.state.cache_bytes > 0);
        let idle = ledger.shutdown_status();
        assert_eq!(
            (
                idle.running,
                idle.uncertain,
                idle.active_attempt,
                idle.fenced
            ),
            (0, 0, false, false)
        );
        assert!(
            !idle.has_pending(),
            "completed and expired receipts are not unfinished work"
        );

        let plan = ledger
            .prepare(&issuer(), &CapsuleSet(vec![capsule(3, 2)]))
            .unwrap();
        assert!(
            !ledger.shutdown_status().has_pending(),
            "prepare is not a native attempt"
        );
        let attempt = ledger.begin(plan).unwrap();
        let running = ledger.shutdown_status();
        assert_eq!(
            (
                running.running,
                running.uncertain,
                running.active_attempt,
                running.fenced
            ),
            (1, 0, true, false)
        );
        assert!(running.has_pending());
        ledger.mark_uncertain(attempt).unwrap();
        let uncertain = ledger.shutdown_status();
        assert_eq!(
            (
                uncertain.running,
                uncertain.uncertain,
                uncertain.active_attempt,
                uncertain.fenced
            ),
            (0, 1, false, true)
        );
        assert!(
            uncertain.has_pending(),
            "no active handle is not proof of native completion"
        );

        // Each independent witness prevents a false clean-shutdown report.
        for status in [
            ReceiveWorkStatus {
                running: 1,
                uncertain: 0,
                active_attempt: false,
                fenced: false,
            },
            ReceiveWorkStatus {
                running: 0,
                uncertain: 1,
                active_attempt: false,
                fenced: false,
            },
            ReceiveWorkStatus {
                running: 0,
                uncertain: 0,
                active_attempt: true,
                fenced: false,
            },
            ReceiveWorkStatus {
                running: 0,
                uncertain: 0,
                active_attempt: false,
                fenced: true,
            },
        ] {
            assert!(status.has_pending());
        }
    }

    #[test]
    fn a_bound_load_replays_exact_input_without_new_native_work() {
        let input = capsule(3, 0);
        assert!(PhysicalReceiveLedger::new(0).is_err());
        assert!(
            PhysicalReceiveLedger::default()
                .prepare(&issuer(), &CapsuleSet(vec![input.clone()]))
                .is_err()
        );
        let mut ledger = PhysicalReceiveLedger::new(1).unwrap();
        let output = execute(&mut ledger, &input);
        let plan = ledger.prepare(&issuer(), &CapsuleSet(vec![input])).unwrap();
        assert!(plan.is_replay_only());
        assert!(plan.fresh_indices().is_empty());
        let attempt = ledger.begin(plan).unwrap();
        assert!(attempt.is_replay_only());
        assert!(attempt.fresh_indices().is_empty());
        let got = ledger
            .complete(attempt, &CapsuleSet(vec![]), false)
            .unwrap();
        assert_eq!(got, CapsuleSet(vec![output]));
        assert_eq!(ledger.state.cache_order.len(), 1);
        assert!(!ledger.state.fenced);
    }

    #[test]
    fn canonical_input_binds_tensor_and_every_owner_identity_before_effect() {
        let input = capsule(3, 0);
        let mut ledger = PhysicalReceiveLedger::new(1).unwrap();
        execute(&mut ledger, &input);
        let before = ledger.state.clone();
        let mut variants = Vec::new();
        let mut changed = input.clone();
        changed.tensors[0].data[0] ^= 1;
        variants.push(changed);
        let mut changed = input.clone();
        changed.tensors[0].descriptor.name = "another-cut".into();
        variants.push(changed);
        let mut changed = input.clone();
        changed.owners[0].incarnation += 1;
        variants.push(changed);
        let mut changed = input.clone();
        changed.owners[0].session_id = "other".into();
        changed.owners[0].sequence_key = "other\0request-0".into();
        variants.push(changed);
        let mut changed = input.clone();
        changed.owners[0].request_id = "other".into();
        changed.owners[0].sequence_key = "session\0other".into();
        variants.push(changed);
        let mut changed = input.clone();
        changed.owners[0].options = "temperature=0".into();
        variants.push(changed);
        let mut changed = input.clone();
        changed.owners[0].input_token += 1;
        variants.push(changed);
        for changed in variants {
            changed.validate().unwrap();
            let error = ledger
                .prepare(&issuer(), &CapsuleSet(vec![changed]))
                .unwrap_err();
            assert!(error.contains("canonical input"), "{error}");
            assert_eq!(ledger.state, before);
        }
        let mut other_load = input;
        other_load.owners[0].load_generation = 2;
        assert!(
            ledger
                .prepare(&issuer(), &CapsuleSet(vec![other_load]))
                .is_err()
        );
        assert_eq!(ledger.state, before);
    }

    #[test]
    fn unseen_out_of_order_ids_are_allowed_but_expired_floor_is_not() {
        let mut ledger = PhysicalReceiveLedger::with_limits(1, CACHE_BYTES, 10, 8);
        execute(&mut ledger, &capsule(9, 0));
        execute(&mut ledger, &capsule(7, 1));
        assert_eq!(ledger.state.namespaces[0].highest, 9);
        assert_eq!(ledger.state.namespaces[0].floor, 1);
        assert!(
            ledger
                .prepare(&issuer(), &CapsuleSet(vec![capsule(1, 2)]))
                .is_err()
        );
        execute(&mut ledger, &capsule(17, 2));
        assert_eq!(ledger.state.namespaces[0].floor, 9);
        assert_eq!(ledger.state.seen.len(), 1);
        for old in [7, 8, 9] {
            assert!(
                ledger
                    .prepare(&issuer(), &CapsuleSet(vec![capsule(old, 3)]))
                    .is_err()
            );
        }
    }

    #[test]
    fn a_far_jump_is_sparse_and_whole_event_floor_is_order_independent() {
        let mut ledger = PhysicalReceiveLedger::with_limits(1, CACHE_BYTES, 10, 8);
        let before = ledger.state.clone();
        for ids in [[2, 20], [20, 2]] {
            let input = CapsuleSet(vec![capsule(ids[0], 0), capsule(ids[1], 1)]);
            assert!(ledger.prepare(&issuer(), &input).is_err());
            assert_eq!(ledger.state, before);
        }
        execute(&mut ledger, &capsule(u64::MAX, 0));
        assert_eq!(ledger.state.seen.len(), 1, "numeric gaps are not allocated");
        assert_eq!(ledger.state.namespaces[0].floor, u64::MAX - 8);
        execute(&mut ledger, &capsule(u64::MAX - 7, 1));
        assert_eq!(ledger.state.seen.len(), 2);
    }

    #[test]
    fn malformed_later_membership_or_duplicate_rows_never_burns_fresh_ids() {
        let ledger = PhysicalReceiveLedger::new(1).unwrap();
        let before = ledger.state.clone();
        let first = capsule(1, 0);
        let mut bad = capsule(2, 1);
        bad.invocation.sequence_ids[0] = 2;
        bad.validate().unwrap();
        let cases = [
            CapsuleSet(vec![first.clone(), bad]),
            CapsuleSet(vec![first.clone(), first.clone()]),
            CapsuleSet(vec![first, capsule(2, 0)]),
        ];
        for input in cases {
            assert!(ledger.prepare(&issuer(), &input).is_err());
            assert_eq!(ledger.state, before);
        }
    }

    #[test]
    fn a_mixed_event_replays_in_original_order_and_executes_only_fresh_members() {
        let mut ledger = PhysicalReceiveLedger::new(1).unwrap();
        let old = capsule(8, 0);
        let cached = execute(&mut ledger, &old);
        let a = capsule(7, 1);
        let b = capsule(9, 2);
        let input = CapsuleSet(vec![a.clone(), old, b.clone()]);
        let plan = ledger.prepare(&issuer(), &input).unwrap();
        assert_eq!(plan.fresh_indices(), &[0, 2]);
        let attempt = ledger.begin(plan).unwrap();
        let got = ledger
            .complete(attempt, &CapsuleSet(vec![result(&a), result(&b)]), false)
            .unwrap();
        assert_eq!(got.0, vec![result(&a), cached, result(&b)]);
        assert_eq!(ledger.state.cache_order.len(), 3);
    }

    #[test]
    fn fresh_result_late_error_fences_all_members_without_partial_receipts() {
        for malformed in 0..4 {
            let mut ledger = PhysicalReceiveLedger::new(1).unwrap();
            let old = capsule(1, 0);
            execute(&mut ledger, &old);
            let before_old = ledger.state.seen[&(0, 1)].clone();
            let before_bytes = ledger.state.cache_bytes;
            let a = capsule(2, 1);
            let b = capsule(3, 2);
            let input = CapsuleSet(vec![a.clone(), old.clone(), b.clone()]);
            let plan = ledger.prepare(&issuer(), &input).unwrap();
            let attempt = ledger.begin(plan).unwrap();
            let mut outputs = vec![result(&a), result(&b)];
            match malformed {
                0 => outputs[1].owners[0].reply = "wrong-owner".into(),
                1 => {
                    outputs.pop();
                }
                2 => outputs.swap(0, 1),
                3 => outputs[1].invocation.sequence_ids[0] = 99,
                _ => unreachable!(),
            }
            assert!(
                ledger
                    .complete(attempt, &CapsuleSet(outputs), false)
                    .is_err()
            );
            assert!(ledger.state.fenced);
            assert_eq!(ledger.state.seen[&(0, 2)], Seen::Uncertain);
            assert_eq!(ledger.state.seen[&(0, 3)], Seen::Uncertain);
            assert_eq!(ledger.state.seen[&(0, 1)], before_old);
            assert_eq!(ledger.state.cache_bytes, before_bytes);
            assert!(ledger.prepare(&issuer(), &CapsuleSet(vec![old])).is_err());
        }
    }

    #[test]
    fn lost_native_response_burns_ids_and_never_automatically_retries() {
        let mut ledger = PhysicalReceiveLedger::new(1).unwrap();
        let input = CapsuleSet(vec![capsule(1, 0), capsule(2, 1)]);
        let plan = ledger.prepare(&issuer(), &input).unwrap();
        let attempt = ledger.begin(plan).unwrap();
        assert!(
            ledger.prepare(&issuer(), &input).is_err(),
            "running work must not be reexecuted"
        );
        ledger.mark_uncertain(attempt).unwrap();
        for id in [1, 2] {
            assert_eq!(ledger.state.seen[&(0, id)], Seen::Uncertain);
        }
        assert!(ledger.prepare(&issuer(), &input).is_err());
        assert!(
            ledger
                .prepare(&issuer(), &CapsuleSet(vec![capsule(100_000, 2)]))
                .is_err()
        );
    }

    #[test]
    fn count_and_exact_byte_eviction_leave_non_reexecutable_tombstones() {
        let input = capsule(1, 0);
        let cost = encode_one(&input).unwrap().len() + encode_one(&result(&input)).unwrap().len();
        for (bytes, count) in [(CACHE_BYTES, 1), (cost, 100)] {
            let mut ledger = PhysicalReceiveLedger::with_limits(1, bytes, count, 16);
            execute(&mut ledger, &input);
            assert_eq!(
                ledger.state.cache_bytes, cost,
                "both input and output bytes count"
            );
            execute(&mut ledger, &capsule(2, 1));
            assert_eq!(ledger.state.seen[&(0, 1)], Seen::Expired);
            assert_eq!(ledger.state.cache_bytes, cost);
            let before = ledger.state.clone();
            assert!(
                ledger
                    .prepare(&issuer(), &CapsuleSet(vec![input.clone()]))
                    .unwrap_err()
                    .contains("expired")
            );
            assert_eq!(ledger.state, before);
        }
    }

    #[test]
    fn oversized_valid_work_is_delivered_once_but_never_recomputed() {
        let input = capsule(1, 0);
        let cost = encode_one(&input).unwrap().len() + encode_one(&result(&input)).unwrap().len();
        for (bytes, count) in [(cost - 1, 1), (CACHE_BYTES, 0)] {
            let mut ledger = PhysicalReceiveLedger::with_limits(1, bytes, count, 16);
            execute(&mut ledger, &input);
            assert_eq!(ledger.state.seen[&(0, 1)], Seen::Expired);
            assert_eq!(ledger.state.cache_bytes, 0);
            assert!(ledger.state.cache_order.is_empty());
            assert!(
                ledger
                    .prepare(&issuer(), &CapsuleSet(vec![input.clone()]))
                    .is_err()
            );
        }
    }

    #[test]
    fn foreign_stale_and_exhausted_plan_authorities_refuse_without_mutation() {
        let mut a = PhysicalReceiveLedger::new(1).unwrap();
        let mut b = PhysicalReceiveLedger::new(1).unwrap();
        let input = CapsuleSet(vec![capsule(1, 0)]);
        let wrong_origin = a.prepare(&issuer(), &input).unwrap();
        let before = b.state.clone();
        assert!(b.begin(wrong_origin).is_err());
        assert_eq!(b.state, before);
        let stale = a.prepare(&issuer(), &input).unwrap();
        execute(&mut a, &input.0[0]);
        let before = a.state.clone();
        assert!(a.begin(stale).is_err());
        assert_eq!(a.state, before);
        for revision in [u64::MAX - 1, u64::MAX] {
            b.state.revision = revision;
            let before = b.state.clone();
            assert!(b.prepare(&issuer(), &input).is_err());
            assert_eq!(b.state, before);
        }
        b.state.revision = 0;
        b.state.next_attempt = u64::MAX;
        let before = b.state.clone();
        assert!(b.prepare(&issuer(), &input).is_err());
        assert_eq!(b.state, before);
    }

    #[test]
    fn a_foreign_attempt_cannot_complete_another_ledgers_running_work() {
        let mut a = PhysicalReceiveLedger::new(1).unwrap();
        let mut b = PhysicalReceiveLedger::new(1).unwrap();
        let input = CapsuleSet(vec![capsule(1, 0)]);
        let attempt_a = a.begin(a.prepare(&issuer(), &input).unwrap()).unwrap();
        let attempt_b = b.begin(b.prepare(&issuer(), &input).unwrap()).unwrap();
        let before = b.state.clone();
        assert!(
            b.complete(attempt_a, &CapsuleSet(vec![result(&input.0[0])]), false)
                .is_err()
        );
        assert_eq!(b.state, before);
        b.mark_uncertain(attempt_b).unwrap();
    }

    fn execute_from(
        ledger: &mut PhysicalReceiveLedger,
        issuer: &Endpoint,
        input: &PhysicalCapsule,
    ) {
        let plan = ledger
            .prepare(issuer, &CapsuleSet(vec![input.clone()]))
            .unwrap();
        assert_eq!(
            plan.fresh_indices(),
            &[0],
            "a distinct configured head is a distinct producer"
        );
        let attempt = ledger.begin(plan).unwrap();
        assert_eq!(
            ledger
                .complete(attempt, &CapsuleSet(vec![result(input)]), false)
                .unwrap(),
            CapsuleSet(vec![result(input)])
        );
    }

    #[test]
    fn full_configured_head_identity_namespaces_ids_but_session_does_not() {
        let mut ledger = PhysicalReceiveLedger::new(1).unwrap();
        let input = capsule(5, 0);
        let heads = [
            issuer(),
            Endpoint::node(p4_protocol::Address::tcp("other", 1234), "head-node", 1),
            Endpoint::node(p4_protocol::Address::tcp("head", 1235), "head-node", 1),
            Endpoint::node(p4_protocol::Address::tcp("head", 1234), "other-node", 1),
            Endpoint::node(p4_protocol::Address::tcp("head", 1234), "head-node", 2),
        ];
        for head in &heads {
            execute_from(&mut ledger, head, &input);
        }
        assert_eq!(ledger.state.namespaces.len(), heads.len());
        assert_eq!(ledger.state.seen.len(), heads.len());
        for head in &heads {
            assert!(
                ledger
                    .prepare(head, &CapsuleSet(vec![input.clone()]))
                    .unwrap()
                    .is_replay_only()
            );
        }
        let mut other_session = input;
        other_session.owners[0].session_id = "other-session".into();
        other_session.owners[0].sequence_key = "other-session\0request-0".into();
        let before = ledger.state.clone();
        assert!(
            ledger
                .prepare(&heads[0], &CapsuleSet(vec![other_session]))
                .unwrap_err()
                .contains("canonical input")
        );
        assert_eq!(ledger.state, before);
    }

    #[test]
    fn advancing_one_head_window_does_not_expire_another_heads_ids() {
        let mut ledger = PhysicalReceiveLedger::with_limits(1, CACHE_BYTES, 10, 8);
        let a = issuer();
        let b = Endpoint::node(p4_protocol::Address::tcp("other", 1234), "head-node", 1);
        execute_from(&mut ledger, &a, &capsule(3, 0));
        execute_from(&mut ledger, &b, &capsule(3, 1));
        execute_from(&mut ledger, &a, &capsule(100, 0));
        assert_eq!(ledger.state.namespaces[0].floor, 92);
        assert_eq!(ledger.state.namespaces[1].floor, 0);
        assert!(
            ledger
                .prepare(&b, &CapsuleSet(vec![capsule(3, 1)]))
                .unwrap()
                .is_replay_only()
        );
        execute_from(&mut ledger, &b, &capsule(2, 1));
        assert!(
            ledger
                .prepare(&a, &CapsuleSet(vec![capsule(3, 0)]))
                .is_err()
        );
    }

    #[test]
    fn seen_identity_capacity_is_aggregate_across_heads_and_preserves_rejected_state() {
        let mut ledger = PhysicalReceiveLedger::with_limits(1, CACHE_BYTES, 10, 4);
        let a = issuer();
        let b = Endpoint::node(p4_protocol::Address::tcp("other", 1234), "head-node", 1);
        for id in 1..=3 {
            execute_from(&mut ledger, &a, &capsule(id, 0));
        }
        execute_from(&mut ledger, &b, &capsule(1, 1));
        let before = ledger.state.clone();
        assert!(
            ledger
                .prepare(&b, &CapsuleSet(vec![capsule(2, 1)]))
                .unwrap_err()
                .contains("aggregate seen")
        );
        assert_eq!(ledger.state, before);
        // Only this issuer's explicit numerical advance frees its obsolete IDs.
        execute_from(&mut ledger, &a, &capsule(10, 0));
        assert_eq!(ledger.state.seen.len(), 2);
        execute_from(&mut ledger, &b, &capsule(2, 1));
        assert_eq!(ledger.state.seen.len(), 3);
    }

    #[test]
    fn issuer_capacity_never_forgets_a_producer_after_its_receipts_expire() {
        let mut ledger = PhysicalReceiveLedger::with_limits(1, 0, 0, 8);
        ledger.state.limits.issuers = 2;
        let a = issuer();
        let b = Endpoint::node(p4_protocol::Address::tcp("other", 1234), "head-node", 1);
        let c = Endpoint::node(p4_protocol::Address::tcp("third", 1234), "head-node", 1);
        execute_from(&mut ledger, &a, &capsule(1, 0));
        execute_from(&mut ledger, &b, &capsule(1, 1));
        execute_from(&mut ledger, &a, &capsule(100, 0));
        assert_eq!(ledger.state.cache_bytes, 0);
        let before = ledger.state.clone();
        assert!(
            ledger
                .prepare(&c, &CapsuleSet(vec![capsule(1, 2)]))
                .unwrap_err()
                .contains("issuer identity capacity")
        );
        assert_eq!(ledger.state, before);
        assert_eq!(ledger.state.namespaces.len(), 2);
        assert!(
            ledger
                .prepare(&a, &CapsuleSet(vec![capsule(1, 0)]))
                .is_err()
        );
    }

    #[test]
    fn replay_cache_budget_is_load_wide_not_multiplied_by_head_count() {
        let input = capsule(1, 0);
        let cost = encode_one(&input).unwrap().len() + encode_one(&result(&input)).unwrap().len();
        for (bytes, count) in [(CACHE_BYTES, 1), (cost, 10)] {
            let mut ledger = PhysicalReceiveLedger::with_limits(1, bytes, count, 8);
            let a = issuer();
            let b = Endpoint::node(p4_protocol::Address::tcp("other", 1234), "head-node", 1);
            execute_from(&mut ledger, &a, &input);
            execute_from(&mut ledger, &b, &input);
            assert_eq!(ledger.state.cache_bytes, cost);
            assert_eq!(ledger.state.cache_order.len(), 1);
            assert!(
                ledger
                    .prepare(&a, &CapsuleSet(vec![input.clone()]))
                    .unwrap_err()
                    .contains("expired")
            );
            assert!(
                ledger
                    .prepare(&b, &CapsuleSet(vec![input.clone()]))
                    .unwrap()
                    .is_replay_only()
            );
        }
    }

    #[test]
    fn issuer_must_be_a_bounded_configured_node_endpoint() {
        let ledger = PhysicalReceiveLedger::new(1).unwrap();
        let input = CapsuleSet(vec![capsule(1, 0)]);
        let invalid = [
            Endpoint::agent(p4_protocol::Address::tcp("head", 1234)),
            Endpoint::node(p4_protocol::Address::tcp("head", 1234), "head-node", 0),
            Endpoint::node(p4_protocol::Address::tcp("head", 1234), "", 1),
            Endpoint::node(p4_protocol::Address::tcp("head", 0), "head-node", 1),
            Endpoint::node(p4_protocol::Address::tcp("head", 1234), "x".repeat(4097), 1),
        ];
        let before = ledger.state.clone();
        for endpoint in &invalid {
            assert!(ledger.prepare(endpoint, &input).is_err());
            assert_eq!(ledger.state, before);
        }
    }
}
