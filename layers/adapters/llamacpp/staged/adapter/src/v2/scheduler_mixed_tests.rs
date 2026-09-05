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

/// A ready prompt is selected within `PREFILL_PATIENCE + 1` plans, whatever
/// the ratio of ready sequences to batch capacity.
///
/// The first version of this bound was vacuous. It read "is a prompt waiting?"
/// from the capacity-truncated candidate window, so with more ready sequences
/// than room, the rotation carried the prompt out of the window, the code
/// cleared the counter, and the prompt was never selected at all. The three
/// cases below are the three that matter: fewer ready sequences than the batch
/// holds, exactly as many, and more - and only the last one exposed it.
#[test]
fn a_ready_prompt_is_selected_within_the_patience_at_every_ratio() {
    for decoders in [4usize, 7, 8, 16, 64] {
        let capacity = 8;
        let mut scheduler = Scheduler::new();
        let mut demands: Vec<Demand> = (0..decoders)
            .map(|index| demand(index as u32, Phase::Decode, 1))
            .collect();
        demands.push(demand(decoders as u32, Phase::Prefill, 2000));

        // The prompt is ready and stays ready: nothing here consumes it, so a
        // scheduler that never selects it will simply never select it.
        let mut plans_until_prompt = None;
        for plan in 0..64 {
            let allocations = scheduler
                .plan_with_physical_capacity(&demands, capacity, capacity, true, 16, false)
                .expect("a ready set plans");
            if allocations
                .iter()
                .any(|allocation| allocation.phase == Phase::Prefill && allocation.rows > 0)
            {
                plans_until_prompt = Some(plan);
                break;
            }
        }
        let Some(plans) = plans_until_prompt else {
            panic!(
                "{decoders} ready decodes and a capacity of {capacity}: 64 plans selected the \
                 prompt zero times",
            );
        };
        assert!(
            plans <= PREFILL_PATIENCE as usize,
            "{decoders} ready decodes: the prompt waited {plans} plans",
        );
    }
}

/// Serving the prompts does not stall the decodes either.
///
/// The bound above is one-directional on its own: a scheduler that always
/// chose the prompts would pass it. This is the other half, and together they
/// say every cohort is served within a bounded number of plans.
#[test]
fn serving_a_prompt_does_not_stall_the_decodes() {
    let capacity = 8;
    let mut scheduler = Scheduler::new();
    let mut demands: Vec<Demand> = (0..16)
        .map(|index| demand(index, Phase::Decode, 1))
        .collect();
    demands.push(demand(16, Phase::Prefill, 2000));

    let mut decode_plans = 0;
    let mut prefill_plans = 0;
    for _ in 0..64 {
        let allocations = scheduler
            .plan_with_physical_capacity(&demands, capacity, capacity, true, 16, false)
            .expect("a ready set plans");
        if allocations.iter().any(|a| a.phase == Phase::Prefill) {
            prefill_plans += 1;
        } else {
            decode_plans += 1;
        }
    }
    assert_eq!(prefill_plans + decode_plans, 64);
    // Eight decode plans for every prompt plan, which is what a patience of
    // eight means. Stated as a range so the constant can move without this
    // test becoming a copy of it.
    assert!(
        (6..=10).contains(&prefill_plans),
        "the prompt took {prefill_plans} of 64 plans; the decodes took {decode_plans}",
    );
}

/// Every waiting prompt advances, not just the prompt cohort.
///
/// The cohort bound above is satisfied by a scheduler that serves the same
/// eight prompts on every prompt turn and never the ninth. That is what the
/// shipped one did: the cohort decision and the member rotation shared the
/// global cursor, which steps once per plan while the prompts get every
/// ninth, so with eighteen demands the prompt turns landed on two starting
/// offsets forever. Seventeen ready prompts, nine hundred plans, sixteen of
/// them fifty rows in and one still at zero.
///
/// So this asserts per request - every prompt makes progress, and none goes
/// longer than a stated gap without being selected - rather than counting
/// prompt batches, which the defect left untouched at a hundred.
#[test]
fn every_waiting_prompt_advances_not_just_the_prompt_cohort() {
    let capacity = 8;
    let prompts = 17;
    let sequences = prompts + 1;
    let plans = 900;
    let mut scheduler = Scheduler::new();
    // Rows are decremented as they are served, so a scheduler cannot pass by
    // handing the same prompt the same rows forever.
    let mut remaining = vec![2000usize; sequences];
    let mut progress = vec![0usize; sequences];
    let mut last_served = vec![0usize; sequences];
    let mut longest_gap = vec![0usize; sequences];

    for plan in 0..plans {
        let demands: Vec<Demand> = (0..sequences)
            .map(|index| {
                if index == 0 {
                    demand(0, Phase::Decode, 1)
                } else {
                    demand(index as u32, Phase::Prefill, remaining[index])
                }
            })
            .collect();
        let allocations = scheduler
            .plan_with_physical_capacity(&demands, capacity, capacity, true, 16, false)
            .expect("a ready set plans");
        for allocation in &allocations {
            let index = allocation.sequence_id as usize;
            if allocation.phase != Phase::Prefill {
                continue;
            }
            remaining[index] = remaining[index].saturating_sub(allocation.rows);
            progress[index] += allocation.rows;
            longest_gap[index] = longest_gap[index].max(plan - last_served[index]);
            last_served[index] = plan;
        }
    }

    let stalled: Vec<usize> = (1..sequences).filter(|index| progress[*index] == 0).collect();
    assert!(
        stalled.is_empty(),
        "{plans} plans left prompts {stalled:?} at zero rows; the rest reached {progress:?}",
    );
    // A prompt cohort every ninth plan, eight members a turn, seventeen
    // prompts: a little over two turns to come round, so about twenty plans.
    // Stated with room, because the point is that the gap is bounded at all.
    let worst = (1..sequences).map(|index| longest_gap[index]).max().unwrap();
    assert!(
        worst <= 40,
        "a ready prompt went {worst} plans without being selected; gaps {longest_gap:?}",
    );
}
