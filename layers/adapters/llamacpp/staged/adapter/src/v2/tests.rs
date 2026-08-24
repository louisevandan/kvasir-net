use super::*;

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
                options: "{}".into(),
            },
            RowOwner {
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
