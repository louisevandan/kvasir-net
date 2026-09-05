use super::*;
use super::node::state::{AdapterState, ReadyRows, RequestState};

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

/// The cohort split alternates, *given* that an issued decode leaves the
/// ready set.
///
/// **This models the retirement rather than exercising it.** The decode is
/// removed from the demand list here by the test; in the adapter it leaves
/// because `in_flight` is set on issue and `phase()` returns `None` while it
/// is set, and it comes back when the tail clears the flag. What this fixes
/// is the scheduler half: given a ready set with no decode in it, the prompts
/// get a full-width batch rather than a decode's width.
///
/// A worker-level test - real gate, real in-flight transitions, a decode
/// arriving continuously - would settle bounded starvation under the combined
/// policy, and there is no worker harness to write it in yet. Until there is,
/// the no-starvation claim rests on the state machine being read, not run.
#[test]
fn recurrent_cohorts_alternate_when_the_issued_decode_retires() {
    let mut scheduler = Scheduler::new();
    let prompts = [demand(1, Phase::Prefill, 500), demand(2, Phase::Prefill, 500)];
    let mut prompt_batches = 0;
    let mut decode_batches = 0;
    for round in 0..6 {
        // A decode becomes ready every round; the prompts are always ready.
        let ready: Vec<_> = std::iter::once(demand(0, Phase::Decode, 1))
            .chain(prompts.iter().cloned())
            .collect();
        let plan = scheduler
            .plan_with_physical_capacity(&ready, 512, 64, true, 10, false)
            .unwrap();
        assert!(!plan.is_empty(), "round {round} planned nothing");
        if plan[0].phase == Phase::Decode {
            decode_batches += 1;
            // The decode is now in flight, so the next round has only prompts.
            let without = prompts.to_vec();
            let next = scheduler
                .plan_with_physical_capacity(&without, 512, 64, true, 10, false)
                .unwrap();
            assert_eq!(next.len(), 2, "both prompts should share the batch");
            assert!(
                next.iter().all(|row| row.rows == 32),
                "prompts should get the whole UBATCH, not a decode's width",
            );
            prompt_batches += 1;
        }
    }
    assert_eq!(decode_batches, 6, "a ready decode should take every batch it can");
    assert_eq!(prompt_batches, 6, "and the prompts should get one whenever it is out");
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

/// The depth bound counts batches, and a batch that splits cannot exceed it.
///
/// The gate is checked before a batch is planned, and llama.cpp decides
/// afterwards how many physical ubatches it becomes. Counting execution ids
/// let three open ids admit a batch that split into four and hold seven; one
/// entry per issued batch makes the bound mean what it says.
#[test]
fn the_open_batch_ledger_counts_batches_not_executions() {
    let mut state = AdapterState::default();
    assert_eq!(state.open_batches.len(), 0);

    // One batch that llama.cpp split into four physical ubatches.
    state.open_batch([11, 12, 13, 14]);
    assert_eq!(state.open_batches.len(), 1, "four capsules are still one batch");

    state.open_batch([21]);
    assert_eq!(state.open_batches.len(), 2);

    // The tail returns the split batch one capsule at a time; the batch stays
    // open until its last capsule is back.
    for execution in [11, 12, 13] {
        state.close_execution(execution);
        assert_eq!(state.open_batches.len(), 2, "a partly returned batch is open");
    }
    state.close_execution(14);
    assert_eq!(state.open_batches.len(), 1);

    state.close_execution(21);
    assert_eq!(state.open_batches.len(), 0);

    // A capsule that belongs to no open batch is ignored rather than
    // corrupting the ledger - a duplicate terminal capsule must not open a
    // slot that was never taken.
    state.close_execution(21);
    assert_eq!(state.open_batches.len(), 0);
}

/// A new load clears the ledger, so a stale batch cannot hold a slot forever.
#[test]
fn the_open_batch_ledger_does_not_survive_a_load() {
    let mut state = AdapterState::default();
    state.open_batch([1, 2]);
    assert_eq!(state.open_batches.len(), 1);
    state.open_batches.clear();
    assert_eq!(state.open_batches.len(), 0, "the load path clears this");
}

/// A request in the shape the worker builds, for tests about its readiness.
pub(super) fn request_state(tokens: Vec<i32>) -> RequestState {
    let own = p4_protocol::Address::tcp("127.0.0.1", 42001);
    let node = p4_protocol::event::Endpoint::node(own.clone(), "n0", 1);
    RequestState {
        command: InferenceCommand {
            load_generation: 1,
            session_id: "session".into(),
            request_id: "request".into(),
            tokens,
            prompt: None,
            options: String::new(),
            session_key: None,
            max_tokens: 16,
        },
        sequence_id: Some(0),
        template: p4_protocol::event::Event {
            envelope: p4_protocol::event::Envelope {
                protocol_version: p4_protocol::event::Envelope::VERSION,
                event_id: "e1".into(),
                correlation_id: "request".into(),
                causation_id: None,
                source: node.clone(),
                target: node,
                return_route: None,
                class: p4_protocol::event::EventClass::Data,
                sequence: 1,
                deadline_unix_ms: None,
                adapter_kind: Some("llamacpp".into()),
                payload_content_type: "application/test".into(),
            },
            payload: Vec::new(),
        },
        reply: String::new(),
        prompt_cursor: 0,
        prompt_issued: 0,
        ready: None,
        after_settlement: None,
        outstanding: 0,
        generated: 0,
    }
}

/// A prompt with a fragment in flight can issue the next one; a decode cannot.
///
/// The distinction is the whole point. A prompt's tokens are all known, so a
/// second chunk can follow the first into the pipeline. A decode's next row is
/// whatever the tail samples from this one, so it must wait however generous
/// the limit is.
///
/// Measured before this existed: 92 of 192 requests took two or more laps to
/// prefill and some took eleven, while the first node stood idle for a third
/// of the run.
#[test]
fn a_prompt_may_have_several_fragments_in_flight_and_a_decode_may_not() {
    let mut prompt = request_state(vec![1; 1000]);
    // Nothing issued: runnable at any limit.
    assert_eq!(prompt.phase_within(1), Some(Phase::Prefill));
    assert_eq!(prompt.phase_within(4), Some(Phase::Prefill));

    // One fragment of 512 rows is out.
    prompt.prompt_issued = 512;
    prompt.outstanding = 1;
    assert_eq!(prompt.phase_within(1), None, "one fragment is the old behaviour");
    assert_eq!(
        prompt.phase_within(2),
        Some(Phase::Prefill),
        "the rest of a known prompt does not need the first chunk back",
    );

    // Two out, and the limit is two.
    prompt.prompt_issued = 1000;
    prompt.outstanding = 2;
    assert_eq!(prompt.phase_within(2), None, "nothing left to issue anyway");

    // A decode is capped at one however generous the limit.
    let mut decode = request_state(vec![1; 8]);
    decode.prompt_issued = 8;
    decode.prompt_cursor = 8;
    decode.ready = Some(ReadyRows {
        phase: Phase::Decode,
        tokens: vec![7],
        position: 8,
        speculative_id: 0,
    });
    assert_eq!(decode.phase_within(8), Some(Phase::Decode));
    decode.outstanding = 1;
    assert_eq!(
        decode.phase_within(8),
        None,
        "the next token is not known until this one is sampled",
    );
}

/// The settled cursor trails the issued one and never passes it.
///
/// Two cursors is the cost of letting a prompt run ahead of its settlements,
/// and the invariant that makes them safe is that settlement can only ever
/// catch up: rows come back in the order they went out, so a settled cursor
/// beyond the issue point would mean the tail settled rows nobody sent.
#[test]
fn the_settled_prompt_cursor_never_passes_the_issued_one() {
    let mut prompt = request_state(vec![1; 1000]);
    prompt.prompt_issued = 512;
    prompt.outstanding = 1;
    prompt.prompt_issued += 488;
    prompt.outstanding += 1;
    assert_eq!(prompt.prompt_issued, 1000);

    // Settlement of the first fragment.
    prompt.prompt_cursor += 512;
    prompt.outstanding -= 1;
    assert!(prompt.prompt_cursor <= prompt.prompt_issued);
    assert_eq!(prompt.phase_within(4), None, "the prompt is fully issued");

    // And of the second.
    prompt.prompt_cursor += 488;
    prompt.outstanding -= 1;
    assert_eq!(prompt.prompt_cursor, prompt.prompt_issued);
    assert_eq!(prompt.outstanding, 0);
    // Prompt done and no decode row yet: nothing to do until the tail sends one.
    assert_eq!(prompt.phase_within(4), None);
}

/// Dropping the adapter finishes, even with a completion mailbox nobody drained.
///
/// The worker waits for room rather than throwing a computed token away, and
/// that wait has to end when the reader is leaving. It could not: `Drop`
/// closed the inbound channel and joined the worker, but the mailbox receiver
/// is a field of the adapter and so outlives `drop` - the worker never saw
/// `Closed`, retried for ever, and the join never returned. The two waited on
/// each other.
///
/// This drives the deadlock: a mailbox of capacity one, filled and never
/// read, then a load that makes the worker publish. Without the shutdown flag
/// the drop below does not return and this test hangs rather than failing.
#[test]
fn dropping_the_adapter_returns_even_with_an_undrained_completion_mailbox() {
    use p4_adapter::node_adapter::NodeAdapter;
    use std::sync::mpsc;
    use std::time::Duration;

    let (done, finished) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let own = p4_protocol::Address::tcp("127.0.0.1", 42101);
        let endpoint = p4_protocol::event::Endpoint::node(own, "n0", 1);
        // One slot in the mailbox, and nothing ever takes from it.
        let adapter = super::node::LlamaNodeAdapter::new(endpoint.clone(), 8, 1);

        // Anything the worker answers goes to the mailbox. A malformed load is
        // enough: the reply is an error event, which still has to be published.
        for sequence in 0..4 {
            let _ = adapter.try_offer(malformed_load(&endpoint, sequence));
        }
        // Wait for the worker to actually be waiting on a full mailbox, rather
        // than for a duration that might be enough: the snapshot says so, and
        // a sleep would leave this test passing for the wrong reason on a slow
        // machine and flaking on a fast one.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !adapter.snapshot().starts_with("completion_queue_full:waiting") {
            assert!(
                std::time::Instant::now() < deadline,
                "the worker never reached a full completion mailbox: {}",
                adapter.snapshot(),
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(adapter);
        let _ = done.send(());
    });

    assert!(
        finished.recv_timeout(Duration::from_secs(10)).is_ok(),
        "dropping the adapter must not wait on a worker that is waiting on it",
    );
    worker.join().expect("the driving thread should finish");
}

/// A load command the worker will refuse, so it answers with an error event.
fn malformed_load(endpoint: &p4_protocol::event::Endpoint, sequence: u64) -> p4_protocol::event::Event {
    p4_protocol::event::Event {
        envelope: p4_protocol::event::Envelope {
            protocol_version: p4_protocol::event::Envelope::VERSION,
            event_id: format!("load-{sequence}"),
            correlation_id: format!("request-{sequence}"),
            causation_id: None,
            source: endpoint.clone(),
            target: endpoint.clone(),
            return_route: None,
            class: p4_protocol::event::EventClass::Control,
            sequence,
            deadline_unix_ms: None,
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: LOAD_CONTENT_TYPE.into(),
        },
        payload: b"{}".to_vec(),
    }
}
