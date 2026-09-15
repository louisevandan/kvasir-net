//! Ordinary attention's bounded pipeline policy. Model execution, KV authority
//! and edge storage belong to the worker/ledgers; these values cannot grant them.
use super::{Demand, OrdinaryLimits, Phase, SchedulerError};

/// Maximum intentional decode-only coalescing delay, not a native/RPC SLO.
pub(crate) const DECODE_COALESCE_WAIT_MS: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PipelinePolicy {
    /// Optional profiled total token budget while generation is active.
    /// Unlike mixed_prefill_rows this includes the selected decode rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixed_batch_rows: Option<usize>,
    /// Prefill rows allowed while any request is decoding, including a decode
    /// currently in flight. Pure-prefill can use the full ordinary token budget.
    /// This is a non-preemptive work quantum, not a wall-clock latency promise.
    pub mixed_prefill_rows: usize,
}

/// Session-local admitted requests, counted before eligibility filtering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PhasePopulation {
    pub ready: usize,
    pub in_flight: usize,
    pub waiting: usize,
}

impl PhasePopulation {
    fn active(self) -> Result<usize, SchedulerError> {
        self.ready
            .checked_add(self.in_flight)
            .and_then(|n| n.checked_add(self.waiting))
            .ok_or(SchedulerError::InvalidDemand)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PipelinePopulation {
    /// Requests with unissued prompt tokens, including an outstanding chunk.
    pub prefill: PhasePopulation,
    pub decode: PhasePopulation,
    /// All prompt tokens issued, final prompt work not settled. These requests
    /// cannot supply another prefill chunk and do not enlarge prefill cohorts.
    pub prefill_draining: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PipelineSelection {
    pub window: usize,
    pub open: usize,
    pub decoding_active: bool,
    pub mixed_prefill_rows: usize,
    pub effective_limits: OrdinaryLimits,
    /// Absent in historical observations made before timer-backed pacing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_coalesce_max_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub population: Option<PipelinePopulation>,
    /// Desired independent prefill groups inside the existing finite window.
    /// This is not a reservation or a measured optimal flight target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill_groups: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_groups: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixed_batch_rows: Option<usize>,
}

impl PipelinePolicy {
    /// Keep prefill membership independent of transient free slots. Requests
    /// already in flight still belong to the population of unfinished prompts.
    /// Row width, execution and all resource admission remain separate.
    pub fn select(
        self,
        demands: &[Demand],
        configured: OrdinaryLimits,
        window: usize,
        open: usize,
        population: PipelinePopulation,
    ) -> Result<PipelineSelection, SchedulerError> {
        if window == 0
            || open >= window
            || self.mixed_prefill_rows == 0
            || self.mixed_batch_rows == Some(0)
        {
            return Err(SchedulerError::InvalidDemand);
        }
        let ready = |phase| demands.iter().filter(|d| d.phase == phase).count();
        if population.prefill.ready != ready(Phase::Prefill)
            || population.decode.ready != ready(Phase::Decode)
        {
            return Err(SchedulerError::InvalidDemand);
        }
        let active_prefill = population.prefill.active()?;
        let active_decode = population.decode.active()?;
        let decoding_active = active_decode > 0;
        let decode_groups = active_decode.min(window);
        let decode_members = active_decode.div_ceil(decode_groups.max(1)).max(1);
        let prefill_groups = active_prefill.min(window);
        // Final prompt chunks still occupy their original pipeline cohorts.
        // Keep initial width stable until their results turn into decode work.
        let prefill_population = active_prefill
            .checked_add(population.prefill_draining)
            .ok_or(SchedulerError::InvalidDemand)?;
        let prefill_members = prefill_population
            .div_ceil(window)
            .max(1)
            .min(active_prefill.max(1));
        let cap = |configured: usize, derived: usize| {
            if configured == 0 {
                derived
            } else {
                configured.min(derived)
            }
        };
        let mut limits = configured;
        limits.prefill_members = cap(configured.prefill_members, prefill_members);
        // Keep generation cohorts independent even after prefill has ended.
        // Population, not a simultaneous return or the last vacancy, sets width.
        limits.decode_members = cap(configured.decode_members, decode_members);
        if decoding_active {
            limits.prefill_rows = cap(configured.prefill_rows, self.mixed_prefill_rows);
        }
        Ok(PipelineSelection {
            window,
            open,
            decoding_active,
            mixed_prefill_rows: self.mixed_prefill_rows,
            effective_limits: limits,
            decode_coalesce_max_ms: Some(DECODE_COALESCE_WAIT_MS),
            population: Some(population),
            prefill_groups: Some(prefill_groups),
            decode_groups: Some(decode_groups),
            mixed_batch_rows: self.mixed_batch_rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::scheduler::Scheduler;

    fn demand(i: u32, phase: Phase, rows: usize) -> Demand {
        Demand {
            request_id: format!("r{i}"),
            sequence_id: i,
            compatibility: "session".into(),
            phase,
            available_rows: rows,
            atomic: false,
        }
    }

    #[test]
    fn full_width_prefill_keeps_eight_disjoint_batches_available_without_fragments() {
        let mut ready: Vec<_> = (0..16)
            .map(|i| demand(i, Phase::Prefill, 100_000))
            .collect();
        let mut scheduler = Scheduler::new();
        let policy = PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 128,
        };
        for open in 0..8 {
            let population = PipelinePopulation {
                prefill: PhasePopulation {
                    ready: ready.len(),
                    in_flight: 16 - ready.len(),
                    waiting: 0,
                },
                ..Default::default()
            };
            let selection = policy
                .select(&ready, OrdinaryLimits::default(), 8, open, population)
                .unwrap();
            let plan = scheduler
                .prepare_plan_with_limits(
                    &ready,
                    512,
                    512,
                    false,
                    1,
                    false,
                    selection.effective_limits,
                )
                .unwrap();
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
        let policy = PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 128,
        };
        for (decoding_active, expected) in [(true, 128), (false, 512)] {
            let population = PipelinePopulation {
                prefill: PhasePopulation {
                    ready: 1,
                    ..Default::default()
                },
                decode: PhasePopulation {
                    in_flight: usize::from(decoding_active),
                    ..Default::default()
                },
                ..Default::default()
            };
            let selection = policy
                .select(&ready, OrdinaryLimits::default(), 8, 1, population)
                .unwrap();
            let plan = scheduler
                .prepare_plan_with_limits(
                    &ready,
                    512,
                    512,
                    false,
                    1,
                    false,
                    selection.effective_limits,
                )
                .unwrap();
            assert_eq!(plan.allocations()[0].rows, expected);
        }
    }

    #[test]
    fn an_explicit_smaller_budget_survives_and_no_unbounded_window_is_accepted() {
        let ready: Vec<_> = (0..16)
            .map(|i| demand(i, Phase::Prefill, 100_000))
            .collect();
        let policy = PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 128,
        };
        let configured = OrdinaryLimits {
            prefill_rows: 64,
            prefill_members: 1,
            ..OrdinaryLimits::default()
        };
        let population = PipelinePopulation {
            prefill: PhasePopulation {
                ready: 16,
                ..Default::default()
            },
            decode: PhasePopulation {
                in_flight: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = policy.select(&ready, configured, 8, 0, population).unwrap();
        assert_eq!(result.effective_limits.prefill_rows, 64);
        assert_eq!(result.effective_limits.prefill_members, 1);
        for (window, open) in [(0, 0), (8, 8), (8, 9)] {
            assert!(
                policy
                    .select(&ready, configured, window, open, population)
                    .is_err()
            );
        }
    }

    #[test]
    fn the_last_vacancy_does_not_merge_independent_prefills_and_draining_is_not_future_work() {
        let policy = PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 128,
        };
        let ready: Vec<_> = (0..8).map(|i| demand(i, Phase::Prefill, 100_000)).collect();
        let population = PipelinePopulation {
            prefill: PhasePopulation {
                ready: 8,
                ..Default::default()
            },
            decode: PhasePopulation {
                in_flight: 7,
                ..Default::default()
            },
            ..Default::default()
        };
        for open in 0..8 {
            let selection = policy
                .select(&ready, OrdinaryLimits::default(), 8, open, population)
                .unwrap();
            assert_eq!(selection.effective_limits.prefill_members, 1);
            assert_eq!(selection.effective_limits.prefill_rows, 128);
            assert_eq!(selection.prefill_groups, Some(8));
        }
        let draining = PipelinePopulation {
            prefill: PhasePopulation {
                ready: 1,
                ..Default::default()
            },
            prefill_draining: 15,
            ..Default::default()
        };
        let selection = policy
            .select(&ready[..1], OrdinaryLimits::default(), 8, 7, draining)
            .unwrap();
        assert_eq!(selection.effective_limits.prefill_members, 1);
        assert_eq!(selection.prefill_groups, Some(1));
    }

    #[test]
    fn an_inconsistent_population_is_rejected_without_spending_scheduler_state() {
        let policy = PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 128,
        };
        let ready = [demand(0, Phase::Prefill, 100_000)];
        assert!(
            policy
                .select(&ready, OrdinaryLimits::default(), 8, 7, Default::default())
                .is_err()
        );
        let overflow = PipelinePopulation {
            prefill: PhasePopulation {
                ready: 1,
                in_flight: usize::MAX,
                waiting: 0,
            },
            ..Default::default()
        };
        assert!(
            policy
                .select(&ready, OrdinaryLimits::default(), 8, 7, overflow)
                .is_err()
        );
    }

    #[test]
    fn generation_cohorts_own_capacity_before_prefill_independent_of_flight_vacancies() {
        let policy = PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 128,
        };
        let mut ready: Vec<_> = (0..16).map(|i| demand(i, Phase::Decode, 1)).collect();
        ready.push(demand(16, Phase::Prefill, 100_000));
        let population = PipelinePopulation {
            prefill: PhasePopulation {
                ready: 1,
                ..Default::default()
            },
            decode: PhasePopulation {
                ready: 16,
                ..Default::default()
            },
            ..Default::default()
        };
        for open in 0..8 {
            let selected = policy
                .select(&ready, OrdinaryLimits::default(), 8, open, population)
                .unwrap();
            let scheduler = Scheduler::new();
            let plan = scheduler
                .prepare_plan_with_limits(
                    &ready,
                    512,
                    512,
                    false,
                    1,
                    false,
                    selected.effective_limits,
                )
                .unwrap();
            assert_eq!(
                plan.allocations()
                    .iter()
                    .filter(|a| a.phase == Phase::Decode)
                    .count(),
                2
            );
            assert_eq!(
                plan.allocations()
                    .iter()
                    .filter(|a| a.phase == Phase::Prefill)
                    .map(|a| a.rows)
                    .sum::<usize>(),
                128
            );
        }
        let explicit = policy
            .select(
                &ready,
                OrdinaryLimits {
                    decode_members: 1,
                    ..Default::default()
                },
                8,
                0,
                population,
            )
            .unwrap();
        assert_eq!(explicit.effective_limits.decode_members, 1);
    }
}
