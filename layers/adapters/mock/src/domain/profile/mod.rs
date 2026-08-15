//! The declared behaviour of a deployment that does not exist.
//!
//! Every figure here is stated by the caller in `MODEL_LOAD.stage_plan`, under
//! a `mock` object the way a real backend's knobs sit under `load_options`.
//! Nothing is measured and nothing is random: two runs of the same plan
//! produce the same timings and the same token text, so a difference between
//! runs is a difference in P4.

use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Profile {
    /// Concurrent executions this deployment admits. Taken from the same
    /// `load_options.batching.max_sequences` a real adapter reads, so the
    /// admission gate under test is the one production uses.
    pub(crate) max_sequences: usize,
    /// Wall time the simulated load occupies, spread over `load_steps`
    /// progress reports.
    pub(crate) load: Duration,
    pub(crate) load_steps: u32,
    /// Delay before a request's first token, standing in for prefill.
    pub(crate) prefill: Duration,
    /// Delay between tokens after the first.
    pub(crate) token: Duration,
    /// Tokens to emit. `None` follows the request's own `max_tokens`.
    pub(crate) tokens: Option<u32>,
    /// Bytes this deployment claims to have reserved, reported per simulated
    /// stage so DRAFT_REPORT carries a shape worth reading.
    pub(crate) stages: usize,
    pub(crate) reserved_per_stage: u64,
    pub(crate) fault: Fault,
}

/// Failures a caller can ask for on purpose. Each one is a terminal P4 state
/// that is otherwise hard to reach without breaking a real backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    None,
    /// `MODEL_LOAD` reports ERROR instead of binding.
    Load,
    /// Execution reports ERROR after emitting this many tokens.
    AfterTokens(u32),
    /// Execution never answers, to exercise deadlines and cancellation.
    Hang,
}

const DEFAULT_MAX_SEQUENCES: usize = 8;
const MAX_MAX_SEQUENCES: usize = 4096;
const DEFAULT_LOAD_STEPS: u32 = 4;
const MAX_TOKENS: u32 = 100_000;

impl Default for Profile {
    fn default() -> Self {
        Self {
            max_sequences: DEFAULT_MAX_SEQUENCES,
            load: Duration::ZERO,
            load_steps: DEFAULT_LOAD_STEPS,
            prefill: Duration::ZERO,
            token: Duration::ZERO,
            tokens: None,
            stages: 1,
            reserved_per_stage: 0,
            fault: Fault::None,
        }
    }
}

impl Profile {
    /// Reads a plan. A malformed or absent field takes the default rather than
    /// failing the load: the caller is describing a simulation, and a silent
    /// refusal to start would be a worse answer than a stated default.
    pub(crate) fn parse(stage_plan: &str) -> Self {
        let Ok(plan) = serde_json::from_str::<Value>(stage_plan) else {
            return Self::default();
        };
        let default = Self::default();
        Self {
            max_sequences: plan
                .pointer("/load_options/batching/max_sequences")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| (1..=MAX_MAX_SEQUENCES).contains(value))
                .unwrap_or(default.max_sequences),
            load: millis(&plan, "load_ms"),
            load_steps: count(&plan, "load_steps").unwrap_or(default.load_steps).max(1),
            prefill: millis(&plan, "prefill_ms"),
            token: millis(&plan, "token_ms"),
            tokens: count(&plan, "tokens"),
            stages: count(&plan, "stages")
                .map(|value| value as usize)
                .unwrap_or(default.stages)
                .max(1),
            reserved_per_stage: plan
                .pointer("/mock/reserved_bytes_per_stage")
                .and_then(Value::as_u64)
                .unwrap_or(default.reserved_per_stage),
            fault: fault(&plan),
        }
    }

    /// Tokens this request will produce. A request asking for zero gets zero,
    /// which is a legitimate terminal outcome and not an error.
    pub(crate) fn token_count(&self, requested: u32) -> u32 {
        self.tokens.unwrap_or(requested).min(MAX_TOKENS)
    }
}

fn millis(plan: &Value, key: &str) -> Duration {
    plan.pointer(&format!("/mock/{key}"))
        .and_then(Value::as_u64)
        .map(Duration::from_millis)
        .unwrap_or(Duration::ZERO)
}

fn count(plan: &Value, key: &str) -> Option<u32> {
    plan.pointer(&format!("/mock/{key}"))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn fault(plan: &Value) -> Fault {
    match plan.pointer("/mock/fault").and_then(Value::as_str) {
        Some("load") => Fault::Load,
        Some("hang") => Fault::Hang,
        Some("after_tokens") => Fault::AfterTokens(
            count(plan, "fault_after_tokens").unwrap_or(0),
        ),
        _ => Fault::None,
    }
}

#[cfg(test)]
mod tests;
