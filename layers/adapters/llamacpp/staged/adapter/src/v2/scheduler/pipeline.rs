//! Ordinary attention's bounded pipeline policy. Model execution, KV authority
//! and edge storage belong to the worker/ledgers; these values cannot grant them.
use super::{Demand, OrdinaryLimits, Phase, SchedulerError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PipelinePolicy {
    /// Prefill rows allowed while any request is decoding, including a decode
    /// currently in flight. Pure-prefill can use the full ordinary token budget.
    /// This is a non-preemptive work quantum, not a wall-clock latency promise.
    pub mixed_prefill_rows: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PipelineSelection {
    pub window: usize,
    pub open: usize,
    pub decoding_active: bool,
    pub mixed_prefill_rows: usize,
    pub effective_limits: OrdinaryLimits,
}

impl PipelinePolicy {
    /// Divide eligible requests over the unoccupied flight slots, not all over
    /// one batch. Recompute from the immutable snapshot after each accepted issue.
    /// Full-width prefill and the number of participating requests are separate:
    /// 16 long requests / 8 slots -> 2 requests x 256 tokens in a 512-row call.
    pub fn select(
        self,
        demands: &[Demand],
        configured: OrdinaryLimits,
        window: usize,
        open: usize,
        decoding_active: bool,
    ) -> Result<PipelineSelection, SchedulerError> {
        if window == 0 || open >= window || self.mixed_prefill_rows == 0 {
            return Err(SchedulerError::InvalidDemand);
        }
        let free = window - open;
        let members = |phase| demands.iter().filter(|d| d.phase == phase).count().div_ceil(free).max(1);
        let cap = |configured: usize, derived: usize| {
            if configured == 0 { derived } else { configured.min(derived) }
        };
        let mut limits = configured;
        limits.prefill_members = cap(configured.prefill_members, members(Phase::Prefill));
        limits.decode_members = cap(configured.decode_members, members(Phase::Decode));
        if decoding_active {
            limits.prefill_rows = cap(configured.prefill_rows, self.mixed_prefill_rows);
        }
        Ok(PipelineSelection { window, open, decoding_active,
            mixed_prefill_rows: self.mixed_prefill_rows, effective_limits: limits })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::scheduler::Scheduler;

    fn demand(i: u32, phase: Phase, rows: usize) -> Demand {
        Demand { request_id: format!("r{i}"), sequence_id: i, compatibility: "session".into(),
            phase, available_rows: rows, atomic: false }
    }

    #[test]
    fn full_width_prefill_keeps_eight_disjoint_batches_available_without_fragments() {
        let mut ready: Vec<_> = (0..16).map(|i| demand(i, Phase::Prefill, 100_000)).collect();
        let mut scheduler = Scheduler::new();
        let policy = PipelinePolicy { mixed_prefill_rows: 128 };
        for open in 0..8 {
            let selection = policy.select(&ready, OrdinaryLimits::default(), 8, open, false).unwrap();
            let plan = scheduler.prepare_plan_with_limits(&ready, 512, 512, false, 1, false,
                selection.effective_limits).unwrap();
            assert_eq!(plan.allocations().len(), 2);
            assert!(plan.allocations().iter().all(|a| a.rows == 256));
            let accepted = scheduler.commit_plan(plan).unwrap();
            ready.retain(|d| !accepted.iter().any(|a| a.sequence_id == d.sequence_id));
        }
        assert!(ready.is_empty());
    }

    #[test]
    fn mixed_quantum_applies_even_while_decode_is_in_flight_and_pure_prefill_recovers_width() {
        let ready = [demand(0, Phase::Prefill, 100_000)];
        let scheduler = Scheduler::new();
        let policy = PipelinePolicy { mixed_prefill_rows: 128 };
        for (decoding_active, expected) in [(true, 128), (false, 512)] {
            let selection = policy.select(&ready, OrdinaryLimits::default(), 8, 1, decoding_active).unwrap();
            let plan = scheduler.prepare_plan_with_limits(&ready, 512, 512, false, 1, false,
                selection.effective_limits).unwrap();
            assert_eq!(plan.allocations()[0].rows, expected);
        }
    }

    #[test]
    fn an_explicit_smaller_budget_survives_and_no_unbounded_window_is_accepted() {
        let ready: Vec<_> = (0..16).map(|i| demand(i, Phase::Prefill, 100_000)).collect();
        let policy = PipelinePolicy { mixed_prefill_rows: 128 };
        let configured = OrdinaryLimits { prefill_rows: 64, prefill_members: 1,
            ..OrdinaryLimits::default() };
        let result = policy.select(&ready, configured, 8, 0, true).unwrap();
        assert_eq!(result.effective_limits.prefill_rows, 64);
        assert_eq!(result.effective_limits.prefill_members, 1);
        for (window, open) in [(0, 0), (8, 8), (8, 9)] {
            assert!(policy.select(&ready, configured, window, open, false).is_err());
        }
    }
}
