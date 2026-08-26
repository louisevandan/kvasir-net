use super::*;

fn demand(sequence_id: u32, phase: Phase, rows: usize) -> Demand {
    Demand {
        request_id: format!("request-{sequence_id}"),
        sequence_id,
        phase,
        available_rows: rows,
        atomic: matches!(phase, Phase::Verify | Phase::Replay),
        compatibility: "completion|tokens|no-lora".into(),
    }
}

#[test]
fn decode_rows_are_reserved_then_prefill_fills_the_same_capsule() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(0, Phase::Decode, 1),
        demand(1, Phase::Decode, 1),
        demand(2, Phase::Prefill, 20),
        demand(3, Phase::Prefill, 20),
    ];
    let plan = scheduler.plan(&demands, 10).unwrap();
    assert_eq!(plan.iter().map(|row| row.rows).sum::<usize>(), 10);
    assert_eq!(
        plan.iter()
            .filter(|row| row.phase == Phase::Decode)
            .map(|row| row.rows)
            .sum::<usize>(),
        2
    );
    assert_eq!(
        plan.iter()
            .filter(|row| row.phase == Phase::Prefill)
            .map(|row| row.rows)
            .sum::<usize>(),
        8
    );
}

#[test]
fn ten_parallel_slots_can_form_a_mixed_physical_batch() {
    let mut scheduler = Scheduler::new();
    let demands: Vec<_> = (0..10)
        .map(|id| {
            if id < 5 {
                demand(id, Phase::Decode, 1)
            } else {
                demand(id, Phase::Prefill, 500)
            }
        })
        .collect();
    let plan = scheduler.plan(&demands, 32).unwrap();
    assert_eq!(plan.len(), 10);
    assert_eq!(plan.iter().map(|row| row.rows).sum::<usize>(), 32);
    assert_eq!(
        plan.iter().filter(|row| row.phase == Phase::Decode).count(),
        5
    );
    assert_eq!(
        plan.iter()
            .filter(|row| row.phase == Phase::Prefill)
            .count(),
        5
    );
}

#[test]
fn cursor_rotates_the_first_prefill_residual() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(0, Phase::Prefill, 5),
        demand(1, Phase::Prefill, 5),
        demand(2, Phase::Prefill, 5),
    ];
    let first = scheduler.plan(&demands, 4).unwrap();
    let second = scheduler.plan(&demands, 4).unwrap();
    assert_eq!(first[0].sequence_id, 0);
    assert_eq!(second[0].sequence_id, 1);
}

#[test]
fn incompatible_rows_are_rejected_before_batch_construction() {
    let mut scheduler = Scheduler::new();
    let mut demands = vec![demand(0, Phase::Decode, 1), demand(1, Phase::Prefill, 5)];
    demands[1].compatibility = "embedding|tokens|no-lora".into();
    assert_eq!(
        scheduler.plan(&demands, 8),
        Err(SchedulerError::MixedCompatibility)
    );
}
