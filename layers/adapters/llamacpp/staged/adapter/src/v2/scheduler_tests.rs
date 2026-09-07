//! L3 transaction tests. These do not stand in for worker/native acceptance.
use super::*;

fn demand(sequence_id: u32, phase: Phase, rows: usize) -> Demand {
    Demand {
        request_id: format!("r{sequence_id}"),
        sequence_id,
        compatibility: "pipeline".into(),
        phase,
        available_rows: rows,
        atomic: matches!(phase, Phase::Verify | Phase::Replay),
    }
}

fn mixed() -> Vec<Demand> {
    vec![
        demand(0, Phase::Decode, 1),
        demand(1, Phase::Decode, 1),
        demand(2, Phase::Prefill, 128),
    ]
}

fn equal(scheduler: &Scheduler, demands: &[Demand]) -> PreparedPlan {
    scheduler
        .prepare_plan_with_physical_capacity(demands, 1, 1, true, 1, false)
        .unwrap()
}

fn snapshot(scheduler: &Scheduler) -> (PolicyState, u64) {
    (scheduler.state.clone(), scheduler.revision)
}

#[test]
fn refused_or_cancelled_candidates_do_not_spend_cohort_or_member_fairness() {
    let mut scheduler = Scheduler::new();
    let demands = mixed();
    let before = snapshot(&scheduler);
    let expected = equal(&scheduler, &demands).allocations().to_vec();
    // More rejected opportunities than the patience limit must still select
    // the very same decode member; neither counter nor resume may move.
    for _ in 0..64 {
        let refused = equal(&scheduler, &demands);
        assert_eq!(refused.allocations(), expected);
        drop(refused);
        assert_eq!(snapshot(&scheduler), before);
    }
    let accepted = equal(&scheduler, &demands);
    assert_eq!(scheduler.commit_plan(accepted).unwrap(), expected);
    assert_eq!(scheduler.state.decode_runs, 1);
    assert_eq!(scheduler.state.decode_resume, 1);
    assert_eq!(scheduler.revision, 1);
    assert_eq!(equal(&scheduler, &demands).allocations()[0].sequence_id, 1);
}

#[test]
fn only_accepted_batches_advance_the_prefill_patience_bound() {
    let mut scheduler = Scheduler::new();
    let demands = mixed();
    for accepted in 0..PREFILL_PATIENCE {
        let before = snapshot(&scheduler);
        for _ in 0..11 {
            let refused = equal(&scheduler, &demands);
            assert_eq!(refused.allocations()[0].phase, Phase::Decode);
        }
        assert_eq!(snapshot(&scheduler), before);
        let next = equal(&scheduler, &demands);
        assert_eq!(scheduler.commit_plan(next).unwrap()[0].phase, Phase::Decode);
        assert_eq!(scheduler.state.decode_runs, accepted + 1);
    }
    let before = snapshot(&scheduler);
    for _ in 0..11 {
        assert_eq!(
            equal(&scheduler, &demands).allocations()[0].phase,
            Phase::Prefill
        );
    }
    assert_eq!(snapshot(&scheduler), before);
    let prompt = equal(&scheduler, &demands);
    assert_eq!(
        scheduler.commit_plan(prompt).unwrap()[0].phase,
        Phase::Prefill
    );
    assert_eq!(scheduler.state.decode_runs, 0);
}

#[test]
fn a_sibling_candidate_is_stale_even_if_policy_values_cycle_back() {
    let mut scheduler = Scheduler::new();
    let demands = vec![demand(0, Phase::Decode, 1)];
    let stale = scheduler.prepare_plan(&demands, 1).unwrap();
    let accepted = scheduler.prepare_plan(&demands, 1).unwrap();
    let original_policy = scheduler.state.clone();
    scheduler.commit_plan(accepted).unwrap();
    // One member makes the ordinary cursor return to zero. Comparing policy
    // values alone would accept this stale candidate (an ABA state).
    assert_eq!(scheduler.state, original_policy);
    let before = snapshot(&scheduler);
    assert_eq!(
        scheduler.validate_prepared(&stale),
        Err(SchedulerError::StalePlan)
    );
    assert_eq!(scheduler.commit_plan(stale), Err(SchedulerError::StalePlan));
    assert_eq!(snapshot(&scheduler), before);
}

#[test]
fn an_identically_configured_scheduler_cannot_commit_a_foreign_candidate() {
    let origin = Scheduler::new();
    let mut other = Scheduler::new();
    let plan = equal(&origin, &mixed());
    let before = snapshot(&other);
    assert_eq!(
        other.validate_prepared(&plan),
        Err(SchedulerError::ForeignPlan)
    );
    assert_eq!(other.commit_plan(plan), Err(SchedulerError::ForeignPlan));
    assert_eq!(snapshot(&other), before);
}

#[test]
fn empty_candidates_and_invalid_preparations_do_not_invalidate_pending_work() {
    let mut scheduler = Scheduler::new();
    let pending = equal(&scheduler, &mixed());
    let before = snapshot(&scheduler);
    let empty = scheduler.prepare_plan(&[], 8).unwrap();
    assert!(scheduler.commit_plan(empty).unwrap().is_empty());
    assert_eq!(
        scheduler.prepare_plan(&mixed(), 0).unwrap_err(),
        SchedulerError::ZeroCapacity
    );
    let mut invalid = mixed();
    invalid[1].sequence_id = invalid[0].sequence_id;
    assert_eq!(
        scheduler.prepare_plan(&invalid, 8).unwrap_err(),
        SchedulerError::DuplicateSequence(0)
    );
    let too_wide = vec![demand(4, Phase::Verify, 9)];
    assert_eq!(
        scheduler.prepare_plan(&too_wide, 8).unwrap_err(),
        SchedulerError::AtomicDemandExceedsCapacity
    );
    assert_eq!(snapshot(&scheduler), before);
    assert_eq!(scheduler.validate_prepared(&pending), Ok(()));
    scheduler.commit_plan(pending).unwrap();
    assert_eq!(scheduler.revision, 1);
}

#[test]
fn policy_revision_exhaustion_refuses_before_any_fairness_write() {
    let mut scheduler = Scheduler::new();
    scheduler.revision = u64::MAX - 1;
    let last = equal(&scheduler, &mixed());
    scheduler.commit_plan(last).unwrap();
    let before = snapshot(&scheduler);
    assert_eq!(
        scheduler.prepare_plan(&mixed(), 8).unwrap_err(),
        SchedulerError::PolicyRevisionExhausted
    );
    let empty = scheduler.prepare_plan(&[], 8).unwrap();
    assert!(scheduler.commit_plan(empty).unwrap().is_empty());
    assert_eq!(snapshot(&scheduler), before);
}

#[test]
fn explicit_acceptance_preserves_all_existing_planner_modes() {
    for (demands, equal_ubatch, exclusive) in [
        (mixed(), false, false),
        (mixed(), true, false),
        (
            vec![demand(0, Phase::Prefill, 64), demand(1, Phase::Verify, 4)],
            false,
            false,
        ),
        (
            vec![demand(0, Phase::Prefill, 64), demand(1, Phase::Verify, 4)],
            true,
            false,
        ),
        (
            vec![demand(0, Phase::Replay, 4), demand(1, Phase::Verify, 4)],
            true,
            true,
        ),
    ] {
        let mut immediate = Scheduler::new();
        let mut explicit = Scheduler::new();
        for _ in 0..40 {
            let before = snapshot(&explicit);
            let prepared = explicit
                .prepare_plan_with_physical_capacity(&demands, 32, 8, equal_ubatch, 2, exclusive)
                .unwrap();
            assert_eq!(snapshot(&explicit), before);
            let expected = immediate
                .plan_with_physical_capacity(&demands, 32, 8, equal_ubatch, 2, exclusive)
                .unwrap();
            assert_eq!(prepared.allocations(), expected);
            assert_eq!(explicit.commit_plan(prepared).unwrap(), expected);
            assert_eq!(snapshot(&explicit), snapshot(&immediate));
        }
    }
}
