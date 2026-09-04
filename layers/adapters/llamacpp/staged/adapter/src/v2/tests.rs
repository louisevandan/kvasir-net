use super::*;

#[test]
fn unload_requires_an_explicit_model_generation() {
    assert!(UnloadCommand { load_generation: 0 }.validate().is_err());
    assert!(UnloadCommand { load_generation: 7 }.validate().is_ok());
}

fn capsule() -> PhysicalCapsule {
    PhysicalCapsule {
        execution_id: 7,
        terminal: false,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: 1,
            n_seqs: 2,
            n_seqs_unq: 2,
            n_pos: 1,
            positions: vec![4, 9],
            sequence_counts: vec![1, 1],
            sequence_ids: vec![3, 8],
            output: vec![true, false],
        },
        owners: vec![
            RowOwner {
                load_generation: 1,
                request_id: "r1".into(),
                sequence_key: "s1".into(),
                session_id: "pipeline-a".into(),
                reply: "reply-1".into(),
                sequence_id: 3,
                phase: Phase::Decode,
                position: 4,
                max_tokens: 500,
                generated_tokens: 4,
                output: true,
                input_token: 41,
                speculative_id: 0,
                speculative_index: 0,
                speculative_count: 0,
                options: "{}".into(),
            },
            RowOwner {
                load_generation: 1,
                request_id: "r2".into(),
                sequence_key: "s2".into(),
                session_id: "pipeline-a".into(),
                reply: "reply-2".into(),
                sequence_id: 8,
                phase: Phase::Prefill,
                position: 9,
                max_tokens: 500,
                generated_tokens: 0,
                output: false,
                input_token: 42,
                speculative_id: 0,
                speculative_index: 0,
                speculative_count: 0,
                options: "{}".into(),
            },
        ],
        tensors: vec![Tensor {
            descriptor: TensorDescriptor {
                tensor_type: 0,
                dimensions: vec![2, 4],
                strides: vec![4, 8],
                nbytes: 8,
                view_offset: 0,
                alias_of: None,
                name: "cut.0".into(),
            },
            data: vec![1; 8],
        }],
        outcomes: Vec::new(),
    }
}

#[test]
fn physical_capsule_round_trips_exact_invocation_and_owners() {
    let expected = CapsuleSet(vec![capsule()]);
    let encoded = expected.encode().unwrap();
    assert_eq!(CapsuleSet::decode(&encoded).unwrap(), expected);
}

#[test]
fn checkpoint_only_outcome_round_trips_without_premature_tokens() {
    let mut replay = capsule();
    replay.terminal = true;
    replay.tensors.clear();
    replay.invocation.positions.truncate(1);
    replay.invocation.sequence_counts.truncate(1);
    replay.invocation.sequence_ids.truncate(1);
    replay.invocation.output.truncate(1);
    replay.owners.truncate(1);
    replay.owners[0].phase = Phase::Verify;
    replay.owners[0].speculative_id = 17;
    replay.owners[0].speculative_count = 1;
    replay.outcomes.push(PhysicalOutcome {
        owner_index: 0,
        generated: Vec::new(),
        proposal: Vec::new(),
        retain_from: Some(7),
        replay_tokens: vec![41, 99, 100],
        replay_position: 4,
    });
    let expected = CapsuleSet(vec![replay]);
    let encoded = expected.encode().unwrap();
    assert_eq!(CapsuleSet::decode(&encoded).unwrap(), expected);
}

#[test]
fn physical_capsule_rejects_owner_output_reconstruction() {
    let mut invalid = capsule();
    invalid.owners[0].output = false;
    assert_eq!(invalid.validate(), Err(CapsuleError::InvalidOwner));
}

#[test]
fn physical_capsule_rejects_trailing_data() {
    let mut encoded = CapsuleSet(vec![capsule()]).encode().unwrap();
    encoded.push(0);
    assert_eq!(
        CapsuleSet::decode(&encoded),
        Err(CapsuleError::TrailingBytes)
    );
}

#[test]
fn logical_batch_round_trips_mixed_rows_without_p4_semantics() {
    let rows = LogicalBatch(vec![
        LogicalRow {
            owner: capsule().owners[0].clone(),
            token: 41,
        },
        LogicalRow {
            owner: capsule().owners[1].clone(),
            token: 42,
        },
    ]);
    assert_eq!(LogicalBatch::decode(&rows.encode().unwrap()).unwrap(), rows);
}

fn demand(sequence_id: u32, phase: Phase, rows: usize) -> Demand {
    Demand {
        request_id: format!("request-{sequence_id}"),
        sequence_id,
        compatibility: "completion|tokens|no-lora".into(),
        phase,
        available_rows: rows,
        atomic: matches!(phase, Phase::Verify | Phase::Replay),
    }
}

#[test]
fn speculative_verify_is_allocated_as_one_physical_transaction() {
    let mut scheduler = Scheduler::new();
    let demands = vec![demand(0, Phase::Verify, 5), demand(1, Phase::Prefill, 20)];
    let plan = scheduler.plan(&demands, 10).unwrap();
    assert_eq!(plan.last().unwrap().phase, Phase::Verify);
    assert_eq!(plan.last().unwrap().rows, 5);
    assert_eq!(plan[0].phase, Phase::Prefill);
    assert_eq!(plan[0].rows, 5);
    assert_eq!(
        scheduler.plan(&[demand(0, Phase::Verify, 9)], 8),
        Err(SchedulerError::AtomicDemandExceedsCapacity)
    );
}

#[test]
fn atomic_group_trails_ordinary_rows_in_the_same_ubatch() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(0, Phase::Verify, 5),
        demand(1, Phase::Prefill, 20),
        demand(2, Phase::Decode, 1),
    ];
    let mixed = scheduler
        .plan_with_physical_capacity(&demands, 16, 12, false, 10, false)
        .unwrap();
    assert_eq!(mixed.last().unwrap().phase, Phase::Verify);
    assert_eq!(mixed.last().unwrap().rows, 5);
    assert!(
        mixed
            .iter()
            .all(|allocation| allocation.phase != Phase::Decode)
    );
    assert!(
        mixed
            .iter()
            .any(|allocation| allocation.phase == Phase::Prefill)
    );
    assert_eq!(
        mixed
            .iter()
            .map(|allocation| allocation.rows)
            .sum::<usize>(),
        10
    );
    assert!(mixed.iter().all(|allocation| allocation.rows == 5));
}

#[test]
fn one_generation_round_batches_multiple_verify_sequences() {
    let mut scheduler = Scheduler::new();
    let demands: Vec<_> = (0..10).map(|id| demand(id, Phase::Verify, 4)).collect();
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 64, 64, true, 10, false)
        .unwrap();
    assert_eq!(plan.len(), 10);
    assert_eq!(plan.iter().map(|row| row.rows).sum::<usize>(), 40);
    assert!(
        plan.iter()
            .all(|row| row.phase == Phase::Verify && row.rows == 4)
    );
}

#[test]
fn recurrent_mixed_generation_round_is_one_contiguous_ubatch() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(0, Phase::Verify, 4),
        demand(1, Phase::Prefill, 20),
        demand(2, Phase::Verify, 4),
        demand(3, Phase::Prefill, 20),
    ];
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 16, 16, true, 10, false)
        .unwrap();
    assert_eq!(
        plan.iter().map(|row| row.sequence_id).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(
        plan.iter().filter(|row| row.phase == Phase::Verify).count(),
        2
    );
    assert_eq!(
        plan.iter()
            .filter(|row| row.phase == Phase::Prefill)
            .count(),
        2
    );
    assert_eq!(plan.iter().map(|row| row.rows).sum::<usize>(), 16);
    assert!(plan.iter().all(|row| row.rows == 4));
}

#[test]
fn generation_round_does_not_mix_recurrent_width_classes() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(0, Phase::Verify, 4),
        demand(1, Phase::Verify, 3),
        demand(2, Phase::Verify, 4),
    ];
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 16, 16, true, 10, false)
        .unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].sequence_id, 0);
    assert!(plan.iter().all(|row| row.rows == 4));
}

#[test]
fn recurrent_atomic_round_stops_at_a_sequence_id_gap() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(2, Phase::Verify, 4),
        demand(3, Phase::Prefill, 20),
        demand(4, Phase::Verify, 4),
        demand(6, Phase::Prefill, 20),
    ];
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 16, 16, true, 10, false)
        .unwrap();
    assert_eq!(
        plan.iter().map(|row| row.sequence_id).collect::<Vec<_>>(),
        vec![2, 3, 4]
    );
    assert!(plan.iter().all(|row| row.rows == 4));
}

#[test]
fn recurrent_prefill_is_one_equal_physical_ubatch() {
    let mut scheduler = Scheduler::new();
    let demands: Vec<_> = (0..10)
        .map(|id| demand(9 - id, Phase::Prefill, 500))
        .collect();
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 512, 64, true, 10, false)
        .unwrap();
    assert_eq!(plan.len(), 10);
    assert!(plan.iter().all(|row| row.rows == 6));
    assert_eq!(plan.iter().map(|row| row.rows).sum::<usize>(), 60);
    assert!(
        plan.windows(2)
            .all(|rows| rows[0].sequence_id < rows[1].sequence_id)
    );
}

/// On a model that forces equal per-sequence widths, a batch carries decodes
/// or prompts and not both.
///
/// This test used to assert the opposite - that a ready decode set the common
/// width to one and the prompts came along at one row each. That is what the
/// scheduler did, and on a 35B hybrid it meant 944.6 rows ready at plan time
/// against 9.85 issued, with 54% of plans leaving rows behind. The prompts
/// were not short of work; they were being cut to a decode's width. The
/// contract changed on that measurement, so this test changed with it.
#[test]
fn recurrent_ready_decode_takes_the_batch_and_leaves_prompts_whole() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(0, Phase::Decode, 1),
        demand(1, Phase::Prefill, 500),
        demand(2, Phase::Prefill, 500),
    ];
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 512, 64, true, 10, false)
        .unwrap();
    // The decode alone, at its own width of one - not two prompts sliced to it.
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].sequence_id, 0);
    assert_eq!(plan[0].rows, 1);
    assert_eq!(plan[0].phase, Phase::Decode);
}

/// And with no decode ready the prompts get the whole UBATCH between them,
/// which is the width the previous contract was throwing away.
#[test]
fn recurrent_prompts_without_a_decode_share_a_wide_equal_ubatch() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(1, Phase::Prefill, 500),
        demand(2, Phase::Prefill, 500),
    ];
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 512, 64, true, 10, false)
        .unwrap();
    assert_eq!(plan.len(), 2);
    // 64 rows of physical capacity across two participants, equal widths.
    assert!(plan.iter().all(|row| row.rows == 32));
}

#[test]
fn negotiated_atomic_limit_serializes_staged_mtp_verification() {
    let mut scheduler = Scheduler::new();
    let demands: Vec<_> = (0..10).map(|id| demand(id, Phase::Verify, 4)).collect();
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 512, 64, true, 1, false)
        .unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].phase, Phase::Verify);
    assert_eq!(plan[0].rows, 4);
}

#[test]
fn negotiated_atomic_exclusivity_rejects_ordinary_cobatching() {
    let mut scheduler = Scheduler::new();
    let demands = vec![
        demand(0, Phase::Verify, 4),
        demand(1, Phase::Prefill, 500),
        demand(2, Phase::Decode, 1),
    ];
    let plan = scheduler
        .plan_with_physical_capacity(&demands, 512, 64, true, 1, true)
        .unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].phase, Phase::Verify);
    assert_eq!(plan[0].rows, 4);
}

#[test]
fn settlement_replay_must_end_exactly_at_retain_boundary() {
    let valid = SettlementCommand {
        load_generation: 1,
        session_id: "pipeline-a".into(),
        sequences: vec![SettlementSequence {
            key: "request".into(),
            id: 0,
            retain_from: 12,
            replay_tokens: vec![1, 2],
            replay_position: 10,
            proposal: Vec::new(),
        }],
    };
    assert_eq!(valid.validate(), Ok(()));
    let mut invalid = valid.clone();
    invalid.sequences[0].replay_position = 9;
    assert!(invalid.validate().is_err());
    invalid.sequences[0].replay_tokens.clear();
    invalid.sequences[0].replay_position = 1;
    assert!(invalid.validate().is_err());

    let mut completed = valid.clone();
    completed.sequences[0].replay_tokens.clear();
    completed.sequences[0].replay_position = 0;
    completed.sequences[0].proposal = vec![3, 4];
    assert_eq!(completed.validate(), Ok(()));
    completed.sequences[0].replay_tokens = vec![1, 2];
    completed.sequences[0].replay_position = 10;
    assert!(completed.validate().is_err());
}
