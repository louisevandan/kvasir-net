//! Measured pipeline RPC service admission. This is a prediction policy,
//! not execution, KV, transport credit or a hard response-time guarantee.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const HISTORY: usize = 128;
const PROFILE: usize = 256;
const MAX_STAGES: usize = 64;
const MAX_EXECUTIONS: usize = 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceShape {
    pub prefill_rows: usize,
    pub decode_rows: usize,
    pub members: usize,
    /// Last input position in this call, not the backend's actual n_kv view.
    pub last_position: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceSample {
    pub load_generation: u64,
    pub session_id: String,
    pub execution_ids: Vec<u64>,
    pub stage_index: usize,
    pub shape: ServiceShape,
    /// Local monotonic Frame request duration; not GPU kernel time.
    pub rpc_us: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceVerdict {
    PurePrefill,
    DecodeOnly,
    Cold,
    CalibrationWait,
    Admit,
    DeferPrefill,
    ProgressProbe,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ServiceDecision {
    pub verdict: ServiceVerdict,
    pub budget_us: u64,
    pub max_pending_us: u64,
    pub max_with_candidate_us: u64,
    pub known_stages: usize,
    pub stages: usize,
    /// FIFO projection through every stage, including unconfirmed open work.
    /// Excludes unmeasured transfer/return delay; never a client ITL guarantee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_tail_rpc_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examined_prefill_rows: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_prefill_rows: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Issued {
    load: u64,
    session: String,
    ordinal: u64,
    executions: Vec<u64>,
    shape: ServiceShape,
    samples: Vec<Option<u64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Profile {
    load: u64,
    session: String,
    stage: usize,
    shape: ServiceShape,
    recent_us: VecDeque<u64>,
}

fn comparable(a: &ServiceShape, b: &ServiceShape) -> bool {
    a.prefill_rows == b.prefill_rows
        && a.decode_rows == b.decode_rows
        && a.members == b.members
        && a.last_position.leading_zeros() == b.last_position.leading_zeros()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ServiceBudget {
    budget_us: Option<u64>,
    issued: VecDeque<Issued>,
    profiles: VecDeque<Profile>,
}

impl ServiceBudget {
    pub fn new(budget_us: u64) -> Self {
        assert!(
            budget_us > 0,
            "service budget is a validated positive duration"
        );
        Self {
            budget_us: Some(budget_us),
            ..Self::default()
        }
    }
    pub fn enabled(&self) -> bool {
        self.budget_us.is_some()
    }
    pub fn validate_session(&self, session: &str) -> Result<(), String> {
        if self.enabled() && session.len() > 4096 {
            return Err("service policy session identity exceeds 4096 bytes".into());
        }
        Ok(())
    }
    pub fn validate(&self, stages: usize, window: usize, ordinary: bool) -> Result<(), String> {
        if self.enabled()
            && (!(2..=MAX_STAGES).contains(&stages) || window == 0 || window > HISTORY || !ordinary)
        {
            return Err("service policy requires ordinary attention, fragment one, pipeline policy and an open window 1..128, stages 2..64".into());
        }
        Ok(())
    }
    pub fn clear(&mut self) {
        self.issued.clear();
        self.profiles.clear();
    }

    pub fn register(
        &mut self,
        sample: ServiceSample,
        ordinal: u64,
        stages: usize,
        open: &BTreeMap<u64, BTreeSet<u64>>,
    ) {
        // Optimization history may expire, but live issued authority cannot.
        // Missing late samples then remain unknown rather than becoming zero.
        while self.issued.len() >= HISTORY {
            let Some(i) = self
                .issued
                .iter()
                .position(|r| !open.contains_key(&r.ordinal))
            else {
                return;
            };
            self.issued.remove(i);
        }
        if sample.execution_ids.is_empty()
            || sample.execution_ids.len() > MAX_EXECUTIONS
            || stages > MAX_STAGES
        {
            return;
        }
        self.issued.push_back(Issued {
            load: sample.load_generation,
            session: sample.session_id.clone(),
            ordinal,
            executions: sample.execution_ids.clone(),
            shape: sample.shape.clone(),
            samples: vec![None; stages],
        });
        self.observe(&sample)
            .expect("locally registered head sample must match");
    }

    /// The caller validates the declared sender before reaching this method.
    /// Samples never retire flight/KV/edge authority or publish model output.
    pub fn observe(&mut self, sample: &ServiceSample) -> Result<bool, String> {
        if sample.execution_ids.is_empty() || sample.execution_ids.len() > MAX_EXECUTIONS {
            return Err("invalid service sample extent or duration".into());
        }
        let Some(record) = self.issued.iter_mut().find(|r| {
            r.load == sample.load_generation
                && r.session == sample.session_id
                && r.executions == sample.execution_ids
        }) else {
            return Ok(false);
        };
        if record.shape != sample.shape || sample.stage_index >= record.samples.len() {
            return Err("service sample differs from accepted issue membership".into());
        }
        let value = &mut record.samples[sample.stage_index];
        if let Some(old) = value {
            return if *old == sample.rpc_us {
                Ok(false)
            } else {
                Err("conflicting service sample".into())
            };
        }
        *value = Some(sample.rpc_us);
        // Decode-only flights consume stage service too. They must participate
        // in calibration and in the unconfirmed-work projection below.
        let index = self.profiles.iter().position(|p| {
            p.load == sample.load_generation
                && p.session == sample.session_id
                && p.stage == sample.stage_index
                && comparable(&p.shape, &sample.shape)
        });
        let mut profile = index
            .and_then(|i| self.profiles.remove(i))
            .unwrap_or_else(|| Profile {
                load: sample.load_generation,
                session: sample.session_id.clone(),
                stage: sample.stage_index,
                shape: sample.shape.clone(),
                recent_us: VecDeque::new(),
            });
        if profile.recent_us.len() == 8 {
            profile.recent_us.pop_front();
        }
        profile.recent_us.push_back(sample.rpc_us);
        if self.profiles.len() == PROFILE {
            self.profiles.pop_front();
        }
        self.profiles.push_back(profile);
        Ok(true)
    }

    fn predict(&self, load: u64, session: &str, stage: usize, shape: &ServiceShape) -> Option<u64> {
        let profiles: Vec<_> = self.profiles
            .iter()
            .rev()
            .filter(|p| {
                p.load == load
                    && p.session == session
                    && p.stage == stage
                    && (p.shape.prefill_rows > 0) == (shape.prefill_rows > 0)
                    && (p.shape.decode_rows > 0) == (shape.decode_rows > 0)
                    && p.shape.last_position.leading_zeros() == shape.last_position.leading_zeros()
            }).collect();
        if let Some(p) = profiles.iter().find(|p| comparable(&p.shape, shape)) {
            return p.recent_us.iter().copied().max();
        }
        // A bounded empirical extrapolation, not a certified backend model.
        // Never assume a smaller candidate is cheaper than its source sample.
        // A rejected smallest quantum is measured by the progress probe; its
        // profile then allows growth without repeatedly admitting a cold full
        // quantum. Context-bucket transitions require fresh calibration.
        let scaled = profiles.iter().filter_map(|p| {
            let mut estimate = u128::from(*p.recent_us.iter().max()?);
            let base = estimate;
            for (wanted, observed) in [(shape.prefill_rows, p.shape.prefill_rows),
                (shape.decode_rows, p.shape.decode_rows), (shape.members, p.shape.members)] {
                if wanted > observed {
                    if observed == 0 { return None; }
                    estimate = estimate.max(base.saturating_mul(wanted as u128).div_ceil(observed as u128));
                }
            }
            if shape.last_position > p.shape.last_position {
                estimate = estimate.saturating_mul(u128::from(shape.last_position) + 1)
                    .div_ceil(u128::from(p.shape.last_position) + 1);
            }
            Some(estimate.min(u128::from(u64::MAX)) as u64)
        }).min();
        // Repeated measurements at two widths reveal the fixed cost that a
        // proportional model otherwise pays again for every added row. Use
        // only equal decode/member shapes, a common context bucket, increasing
        // measured cost and at least a doubling of prefill width. This remains
        // an empirical estimate; row/credit limits do not depend on its truth.
        let mut affine = None;
        for low in &profiles {
            for high in &profiles {
                if low.recent_us.len() < 2 || high.recent_us.len() < 2
                    || low.shape.prefill_rows == 0
                    || high.shape.prefill_rows <= low.shape.prefill_rows
                    || high.shape.prefill_rows < low.shape.prefill_rows.saturating_mul(2)
                    || shape.prefill_rows < high.shape.prefill_rows
                    || low.shape.decode_rows != shape.decode_rows || high.shape.decode_rows != shape.decode_rows
                    || low.shape.members != shape.members || high.shape.members != shape.members { continue; }
                let lower = u128::from(*low.recent_us.iter().max().unwrap());
                let upper = u128::from(*high.recent_us.iter().max().unwrap());
                if upper <= lower { continue; }
                let width = (high.shape.prefill_rows - low.shape.prefill_rows) as u128;
                let extra = (shape.prefill_rows - high.shape.prefill_rows) as u128;
                let mut estimate = upper.saturating_add((upper - lower).saturating_mul(extra).div_ceil(width));
                let position = low.shape.last_position.min(high.shape.last_position);
                if shape.last_position > position {
                    estimate = estimate.saturating_mul(u128::from(shape.last_position) + 1)
                        .div_ceil(u128::from(position) + 1);
                }
                let estimate = estimate.min(u128::from(u64::MAX)) as u64;
                affine = Some(affine.map_or(estimate, |old: u64| old.max(estimate)));
            }
        }
        affine.or(scaled)
    }

    pub fn has_open_prefill(&self, open: &BTreeMap<u64, BTreeSet<u64>>) -> bool {
        open.keys().any(|id| self.issued.iter().find(|r| r.ordinal == *id)
            .is_none_or(|r| r.shape.prefill_rows > 0))
    }

    pub fn has_open_calibration_probe(&self, open: &BTreeMap<u64, BTreeSet<u64>>) -> bool {
        open.keys().any(|id| self.issued.iter().find(|r| r.ordinal == *id)
            .is_none_or(|r| r.shape.prefill_rows == 1))
    }

    fn project_tail(&self, load: u64, session: &str, stages: usize, shape: &ServiceShape,
        open: &BTreeMap<u64, BTreeSet<u64>>) -> Option<u64> {
        if stages == 0 || stages > MAX_STAGES { return None; }
        let mut free = vec![0u64; stages];
        for ordinal in open.keys() {
            let issued = self.issued.iter().find(|r| r.ordinal == *ordinal
                && r.load == load && r.session == session)?;
            // A downstream completion proves the earlier stages have run,
            // even if their telemetry is late. This only updates an estimate:
            // it cannot return KV, flight, edge or output authority.
            let completed = issued.samples.iter().rposition(Option::is_some);
            let mut arrival = 0;
            for (stage, available) in free.iter_mut().enumerate() {
                if completed.is_some_and(|last| stage <= last) { continue; }
                let cost = self.predict(load, session, stage, &issued.shape)?;
                arrival = arrival.max(*available).saturating_add(cost);
                *available = arrival;
            }
        }
        let mut arrival = 0u64;
        for (stage, available) in free.into_iter().enumerate() {
            arrival = arrival.max(available).saturating_add(self.predict(load, session, stage, shape)?);
        }
        Some(arrival)
    }

    pub fn decide(
        &self,
        load: u64,
        session: &str,
        stages: usize,
        shape: &ServiceShape,
        decoding: bool,
        open: &BTreeMap<u64, BTreeSet<u64>>,
    ) -> Option<ServiceDecision> {
        let budget_us = self.budget_us?;
        let mut decision = ServiceDecision {
            verdict: ServiceVerdict::PurePrefill,
            budget_us,
            max_pending_us: 0,
            max_with_candidate_us: 0,
            known_stages: 0,
            stages,
            predicted_tail_rpc_us: None,
            examined_prefill_rows: Vec::new(),
            selected_prefill_rows: None,
        };
        if shape.prefill_rows == 0 {
            decision.verdict = ServiceVerdict::DecodeOnly;
            return Some(decision);
        }
        if !decoding {
            return Some(decision);
        }
        if open.keys().any(|ordinal| {
            !self
                .issued
                .iter()
                .any(|r| r.ordinal == *ordinal && r.load == load && r.session == session)
        }) {
            decision.verdict = ServiceVerdict::Cold;
            return Some(decision);
        }
        let live: Vec<_> = self
            .issued
            .iter()
            .filter(|r| {
                r.load == load
                    && r.session == session
                    && r.shape.prefill_rows > 0
                    && open.contains_key(&r.ordinal)
            })
            .collect();
        // A finite service share: do not let an unattainably small soft target
        // starve prefill forever. Admit one bounded configured quantum once
        // previous prefill flights settle, and expose the over-budget probe.
        let mut complete = true;
        for stage in 0..stages {
            let Some(candidate) = self.predict(load, session, stage, shape) else {
                complete = false;
                continue;
            };
            let mut pending = 0u64;
            let mut known = true;
            for issued in &live {
                if issued.samples.get(stage).is_some_and(Option::is_some) {
                    continue;
                }
                match self.predict(load, session, stage, &issued.shape) {
                    Some(us) => pending = pending.saturating_add(us),
                    None => {
                        known = false;
                        break;
                    }
                }
            }
            if !known {
                complete = false;
                continue;
            }
            decision.known_stages += 1;
            decision.max_pending_us = decision.max_pending_us.max(pending);
            decision.max_with_candidate_us = decision
                .max_with_candidate_us
                .max(pending.saturating_add(candidate));
        }
        decision.predicted_tail_rpc_us = self.project_tail(load, session, stages, shape, open);
        decision.verdict = if !complete || decision.predicted_tail_rpc_us.is_none() {
            ServiceVerdict::Cold
        } else if decision.predicted_tail_rpc_us.is_some_and(|us| us <= budget_us) {
            ServiceVerdict::Admit
        } else if live.is_empty() {
            ServiceVerdict::ProgressProbe
        } else {
            ServiceVerdict::DeferPrefill
        };
        Some(decision)
    }
}

#[cfg(test)]
mod tests;
