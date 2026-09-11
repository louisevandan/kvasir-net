//! Measured per-stage prefill service admission. This is a prediction policy,
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
        // Keep decode issue identity to distinguish known open work, but the
        // current predictor does not charge or learn decode-only service.
        if sample.shape.prefill_rows == 0 {
            return Ok(true);
        }
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
        self.profiles
            .iter()
            .rev()
            .find(|p| {
                p.load == load
                    && p.session == session
                    && p.stage == stage
                    && comparable(&p.shape, shape)
            })?
            .recent_us
            .iter()
            .copied()
            .max()
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
        decision.verdict = if !complete {
            ServiceVerdict::Cold
        } else if decision.max_with_candidate_us <= budget_us {
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
