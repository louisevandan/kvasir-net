//! Actual handle/drive/Frame/retained-effect consumers. Post-LOAD injection is
//! explicit: no real engine, LOAD parser, broker/network timing, or run-loop claim.
use super::effects::CommittedEffect;
use super::observe::{PreparedTelemetry, TelemetryPayload};
use super::*;
use crate::process::{ReadyInfo, ServerControl};
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use p4_protocol::event::OuterEndpoint;

struct EchoStage {
    calls: Arc<Mutex<Vec<Operation>>>,
    next: u64,
}
impl ServerControl for EchoStage {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn wait_ready(&mut self, _: Instant) -> Result<Option<ReadyInfo>, String> {
        Ok(Some(ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "observe-fake".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 128,
            n_batch: 8,
            n_ubatch: 8,
            n_seq_max: 8,
            physical_result_payload_bytes: 0,
            physical_result_tensor_count: 0,
            max_physical_result_bytes: 33_554_432,
            upstream_commit: "fake".into(),
            patch_set: "fake".into(),
            backend_inventory: "no-engine".into(),
            stage_wire_abi: "unknown".into(),
        }))
    }
    fn request(&mut self, frame: Frame) -> Result<Frame, String> {
        self.calls.lock().unwrap().push(frame.header.operation);
        let set = match frame.header.operation {
            Operation::LogicalBatch => {
                let rows = LogicalBatch::decode(&frame.body)
                    .unwrap()
                    .0
                    .into_iter()
                    .map(|r| r.owner)
                    .collect();
                let set = CapsuleSet(vec![physical(self.next, rows)]);
                self.next += 1;
                set
            }
            Operation::PhysicalBatch => CapsuleSet::decode(&frame.body).unwrap(),
            other => panic!("unexpected native operation {other:?}"),
        };
        Frame::new(Operation::PhysicalResult, set.encode().unwrap()).map_err(|e| e.to_string())
    }
    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn endpoint(name: &str) -> Endpoint {
    Endpoint::node(Address::tcp("127.0.0.1", 42501), name, 1)
}
fn route(channel: &str, generation: u64) -> OuterEndpoint {
    OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 42500),
        channel: channel.into(),
        connection_generation: generation,
    }
}
fn submission(id: &str, outer: OuterEndpoint) -> Event {
    let mut request = crate::v2::tests::request_state(vec![7]);
    request.input_mut_for_test().command.load_generation = 1;
    request.input_mut_for_test().command.session_id = "observe".into();
    request.input_mut_for_test().command.request_id = id.into();
    request.input_mut_for_test().command.max_tokens = 2;
    let mut event = request.template.clone();
    event.envelope.event_id = format!("submit-{id}");
    event.envelope.correlation_id = format!("correlation-{id}");
    event.envelope.source = Endpoint::Outer(outer.clone());
    event.envelope.return_route = Some(outer);
    event.envelope.target = endpoint("first");
    event.envelope.payload_content_type = PREFILL_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&request.command).unwrap();
    event
}
fn reply(event: &Event) -> String {
    let outer = event.envelope.return_route.as_ref().unwrap();
    serde_json::to_string(&ReplySpec {
        ingress_agent: outer.ingress_agent.to_string(),
        channel: outer.channel.clone(),
        connection_generation: outer.connection_generation,
        correlation_id: event.envelope.correlation_id.clone(),
        deadline_unix_ms: event.envelope.deadline_unix_ms,
    })
    .unwrap()
}
fn owner(id: &str, slot: u32, outer: OuterEndpoint) -> RowOwner {
    RowOwner {
        load_generation: 1,
        incarnation: slot as u64 + 1,
        request_id: id.into(),
        sequence_key: request_key("observe", id),
        session_id: "observe".into(),
        reply: reply(&submission(id, outer)),
        sequence_id: slot,
        phase: Phase::Prefill,
        position: 0,
        max_tokens: 2,
        generated_tokens: 0,
        output: true,
        input_token: 7,
        speculative_id: 0,
        speculative_index: 0,
        speculative_count: 0,
        options: String::new(),
    }
}
fn physical(id: u64, owners: Vec<RowOwner>) -> PhysicalCapsule {
    let count = owners.len();
    PhysicalCapsule {
        execution_id: id,
        terminal: false,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: 1,
            n_seqs: count as u32,
            n_seqs_unq: count as u32,
            n_pos: 1,
            positions: owners.iter().map(|o| o.position as i32).collect(),
            sequence_counts: vec![1; count],
            sequence_ids: owners.iter().map(|o| o.sequence_id as i32).collect(),
            output: owners.iter().map(|o| o.output).collect(),
        },
        owners,
        tensors: vec![Tensor {
            descriptor: TensorDescriptor {
                tensor_type: 0,
                dimensions: vec![1],
                strides: vec![4],
                nbytes: 4,
                view_offset: 0,
                alias_of: None,
                name: "cut".into(),
            },
            data: vec![0; 4],
        }],
        outcomes: Vec::new(),
    }
}
fn incoming(set: &CapsuleSet) -> Event {
    let mut event = submission("carrier", route("carrier", 1));
    event.envelope.source = endpoint("first");
    event.envelope.target = endpoint("middle");
    event.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
    event.payload = set.encode().unwrap();
    event
}
fn drain(mailbox: &CompletionMailbox) -> Vec<Event> {
    let mut events = Vec::new();
    while let Poll::Event(event) = mailbox.try_take() {
        events.push(event);
    }
    events
}
fn fixture(head: bool) -> (Worker, Arc<CompletionMailbox>, Arc<Mutex<Vec<Operation>>>) {
    let (_, receiver) = mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(64);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut worker = Worker::new(
        endpoint(if head { "first" } else { "middle" }),
        receiver,
        publisher,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    )
    .with_stage_for_test(Box::new(EchoStage {
        calls: calls.clone(),
        next: 1,
    }))
    .unwrap();
    worker.state.load_generation = 1;
    worker.state.sequence_capacity = 8;
    worker.state.free_sequences = (0..8).collect();
    worker.state.context_size = 128;
    worker.state.batch_capacity = 8;
    worker.state.physical_capacity = 8;
    worker.state.max_atomic_sequences = 1;
    worker.state.prefill_fragments = 1;
    worker.state.min_batch_rows = 0;
    worker.state.max_open_batches = 0;
    worker.state.max_issue_rows = 0;
    worker.state.physical_receives =
        super::super::physical_receive::PhysicalReceiveLedger::new(1).unwrap();
    let mut event = submission("setup", route("setup", 1));
    event.envelope.target = worker.endpoint.clone();
    event.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&SessionCommand {
        load_generation: 1,
        session_id: "observe".into(),
        stages: ["first", "middle", "last"]
            .into_iter()
            .map(|node| NodeAddress {
                agent: Address::tcp("127.0.0.1", 42501).to_string(),
                node: node.into(),
                generation: 1,
            })
            .collect(),
        stage_index: usize::from(!head),
    })
    .unwrap();
    worker.handle(event).unwrap();
    drain(&mailbox);
    (worker, mailbox, calls)
}

#[test]
fn actual_head_projects_by_full_outer_not_correlation_and_binds_accepted_index() {
    let (mut worker, mailbox, calls) = fixture(true);
    for (id, outer) in [
        ("a", route("shared", 1)),
        ("b", route("shared", 1)),
        ("c", route("shared", 2)),
    ] {
        worker.handle(submission(id, outer)).unwrap();
    }
    assert!(drain(&mailbox).is_empty());
    let issued = worker.drive_one_batch();
    if issued != Ok(true) {
        panic!(
            "head issue {issued:?}: {}; {:?}",
            worker.snapshot.lock().unwrap(),
            drain(&mailbox)
        );
    }
    assert_eq!(*calls.lock().unwrap(), [Operation::LogicalBatch]);
    let events = drain(&mailbox);
    assert_eq!(events.len(), 5);
    assert_eq!(
        events[0].envelope.payload_content_type,
        PHYSICAL_BATCH_CONTENT_TYPE
    );
    let observations = events
        .iter()
        .filter(|e| e.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE)
        .collect::<Vec<_>>();
    let spans = events
        .iter()
        .filter(|e| e.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE)
        .collect::<Vec<_>>();
    assert_eq!((observations.len(), spans.len()), (2, 2));
    for event in observations {
        let Endpoint::Outer(outer) = &event.envelope.target else {
            panic!("not OUTER")
        };
        let expected = if outer.connection_generation == 1 {
            vec!["a", "b"]
        } else {
            vec!["c"]
        };
        let value: BatchObservation = serde_json::from_slice(&event.payload).unwrap();
        assert_eq!((value.logical_ordinal, value.logical_rows), (1, 3));
        assert_eq!(
            (
                value.physical_batches.len(),
                value.physical_batches[0].rows,
                value.physical_batches[0].request_count
            ),
            (1, 3, 3)
        );
        let owned = &value.physical_batches[0].owned_requests;
        assert_eq!(
            owned
                .iter()
                .map(|o| o.request_id.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        for detail in owned {
            assert_eq!(
                detail.submission_event_id,
                format!("submit-{}", detail.request_id)
            );
            assert_eq!((detail.request_issue_index, detail.prefill_rows), (1, 1));
            assert_eq!(
                detail.rows,
                [IssuedRow {
                    phase: Phase::Prefill,
                    position: 0
                }]
            );
            let witness = worker.state.requests[&request_key("observe", &detail.request_id)]
                .issued_work
                .unwrap();
            assert_eq!((witness.issue_count(), witness.last_ordinal()), (1, 1));
        }
        assert_eq!(
            event.envelope.correlation_id,
            format!("correlation-{}", expected[0])
        );
    }
    for event in spans {
        let span: StageSpan = serde_json::from_slice(&event.payload).unwrap();
        assert_eq!((span.rows, span.execution_ids), (3, vec![1]));
        assert!(span.forward_unix_ms > 0);
        let Endpoint::Outer(outer) = &event.envelope.target else {
            panic!()
        };
        assert_eq!(
            span.executions[0].owned_requests.len(),
            if outer.connection_generation == 1 {
                2
            } else {
                1
            }
        );
    }
}

#[test]
fn head_id_shortage_precedes_prepared_issue_and_exact_room_still_runs() {
    let (mut worker, mailbox, calls) = fixture(true);
    worker.handle(submission("a", route("a", 1))).unwrap();
    worker.state.next_event = u64::MAX - 2;
    let request_bookkeeping = |worker: &Worker| {
        worker
            .state
            .requests
            .iter()
            .map(|(key, r)| {
                (
                    key.clone(),
                    r.incarnation,
                    r.sequence_id,
                    r.prompt_cursor,
                    r.prompt_issued,
                    r.outstanding,
                    r.generated,
                    format!("{:?}", r.ready),
                    format!("{:?}", r.issued_work),
                )
            })
            .collect::<Vec<_>>()
    };
    let before_requests = request_bookkeeping(&worker);
    let before_owners = format!("{:?}", worker.state.stage_owners);
    let before_frontiers = format!("{:?}", worker.state.stage_frontiers);
    for _ in 0..2 {
        assert_eq!(worker.drive_one_batch(), Err(()));
        assert!(
            worker.state.prepared_issue.is_none(),
            "refusal cannot leave a prepared issue"
        );
        assert_eq!(request_bookkeeping(&worker), before_requests);
        assert_eq!(format!("{:?}", worker.state.stage_owners), before_owners);
        assert_eq!(
            format!("{:?}", worker.state.stage_frontiers),
            before_frontiers
        );
        assert_eq!(worker.state.flights.active_counts(), (0, 0));
        assert_eq!(worker.state.next_event, u64::MAX - 2);
        assert!(worker.effects.is_empty());
        assert!(calls.lock().unwrap().is_empty());
        assert!(drain(&mailbox).is_empty());
    }
    // One physical forward + one BatchObservation + one StageSpan, all from
    // the actual head drive/codec consumer. No native work ran in either refusal.
    worker.state.next_event = u64::MAX - 3;
    assert_eq!(worker.drive_one_batch(), Ok(true));
    assert_eq!(*calls.lock().unwrap(), [Operation::LogicalBatch]);
    let events = drain(&mailbox);
    assert_eq!(events.len(), 3);
    assert_eq!(
        events
            .iter()
            .map(|e| e.envelope.sequence)
            .collect::<Vec<_>>(),
        [u64::MAX - 3, u64::MAX - 2, u64::MAX - 1]
    );
    let observation: BatchObservation = serde_json::from_slice(&events[1].payload).unwrap();
    assert_eq!(
        (observation.logical_ordinal, observation.logical_rows),
        (1, 1)
    );
    assert_eq!(worker.state.next_event, u64::MAX);
}

#[test]
fn head_rejects_inconsistent_original_reply_before_native_or_issue_in_both_orders() {
    for bad_first in [false, true] {
        let (mut worker, mailbox, calls) = fixture(true);
        for id in ["a", "b"] {
            worker.handle(submission(id, route(id, 1))).unwrap();
        }
        let key = request_key("observe", if bad_first { "a" } else { "b" });
        let request = worker.state.requests.get_mut(&key).unwrap();
        let mut altered: ReplySpec = serde_json::from_str(&request.reply).unwrap();
        altered.correlation_id = "different-valid-correlation".into();
        request.input_mut_for_test().reply = serde_json::to_string(&altered).unwrap();
        assert_eq!(worker.drive_one_batch(), Err(()));
        assert!(calls.lock().unwrap().is_empty());
        assert!(
            worker
                .snapshot
                .lock()
                .unwrap()
                .contains("issue_observation_failed")
        );
        assert!(worker.state.prepared_issue.is_none());
        assert_eq!(worker.state.flights.active_counts(), (0, 0));
        assert!(
            worker
                .state
                .requests
                .values()
                .all(|r| r.prompt_issued == 0 && r.outstanding == 0 && r.issued_work.is_none())
        );
        assert!(worker.effects.is_empty());
        assert!(drain(&mailbox).is_empty());
    }
}

#[test]
fn middle_validates_every_recipient_before_receipt_begin_or_native_in_both_orders() {
    for (bad_first, malformed_json) in [(false, true), (true, true), (false, false), (true, false)]
    {
        let (mut worker, mailbox, calls) = fixture(false);
        let mut rows = vec![owner("a", 0, route("a", 1)), owner("b", 1, route("b", 1))];
        let bad_owner = &mut rows[usize::from(!bad_first)];
        bad_owner.reply = if malformed_json {
            "not-json".into()
        } else {
            let mut reply: ReplySpec = serde_json::from_str(&bad_owner.reply).unwrap();
            reply.ingress_agent = "tcp://host\0suffix:42500".into();
            serde_json::to_string(&reply).unwrap()
        };
        let set = CapsuleSet(vec![physical(11, rows)]);
        assert!(
            worker
                .physical(incoming(&set))
                .unwrap_err()
                .contains("observation reply contract")
        );
        assert!(calls.lock().unwrap().is_empty());
        assert!(
            !worker
                .state
                .physical_receives
                .shutdown_status()
                .has_pending()
        );
        assert_eq!(
            (
                worker.state.stage_owners.active_slots(),
                worker.state.stage_frontiers.active_slots()
            ),
            (0, 0)
        );
        assert!(worker.effects.is_empty());
        assert!(!worker.effects_fenced);
        assert!(drain(&mailbox).is_empty());
        let valid = CapsuleSet(vec![physical(11, vec![owner("a", 0, route("a", 1))])]);
        worker.physical(incoming(&valid)).unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
}

#[test]
fn physical_replay_emits_no_span_and_mixed_replay_reports_only_fresh_execution() {
    let (mut worker, mailbox, calls) = fixture(false);
    let a = physical(11, vec![owner("a", 0, route("a", 1))]);
    let b = physical(12, vec![owner("b", 1, route("b", 1))]);
    worker
        .physical(incoming(&CapsuleSet(vec![a.clone()])))
        .unwrap();
    assert_eq!(drain(&mailbox).len(), 2);
    worker
        .physical(incoming(&CapsuleSet(vec![a.clone()])))
        .unwrap();
    let replay = drain(&mailbox);
    assert_eq!(replay.len(), 1);
    assert_eq!(calls.lock().unwrap().len(), 1);
    worker.physical(incoming(&CapsuleSet(vec![a, b]))).unwrap();
    let mixed = drain(&mailbox);
    assert_eq!(mixed.len(), 2);
    assert_eq!(calls.lock().unwrap().len(), 2);
    let span: StageSpan = serde_json::from_slice(&mixed[1].payload).unwrap();
    assert_eq!((span.rows, span.execution_ids), (1, vec![12]));
    assert_eq!(span.executions[0].owned_requests[0].request_id, "b");
    assert_eq!(mixed[1].envelope.target, Endpoint::Outer(route("b", 1)));
    assert_eq!(CapsuleSet::decode(&mixed[0].payload).unwrap().0.len(), 2);
}

fn delivery_snapshot(delivery: &PreparedTelemetry) -> Vec<u8> {
    match &delivery.payload {
        TelemetryPayload::Batch(v) => serde_json::to_vec(v).unwrap(),
        TelemetryPayload::Span(v) => serde_json::to_vec(v).unwrap(),
    }
}

#[test]
fn forward_success_then_id_exhaustion_retains_all_stamped_observations_without_reforward() {
    let (mut worker, mailbox, _) = fixture(false);
    let set = CapsuleSet(vec![physical(
        11,
        vec![owner("a", 0, route("a", 1)), owner("b", 1, route("b", 1))],
    )]);
    let base = incoming(&set);
    let telemetry = worker
        .prepare_stage_span(&base, "observe", &set, 10, 11, 12, false)
        .unwrap();
    worker.state.next_event = u64::MAX - 1;
    worker.effects.push_back(CommittedEffect::ForwardObserved {
        base: base.envelope,
        target: endpoint("last"),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body: set.encode().unwrap(),
        telemetry,
    });
    assert!(worker.flush_effects().is_err());
    assert_eq!(drain(&mailbox).len(), 1);
    assert_eq!(worker.effects.len(), 2);
    assert!(worker.effects_fenced);
    let saved = worker
        .effects
        .iter()
        .map(|e| {
            let CommittedEffect::Telemetry(delivery) = e else {
                panic!("forward was retained after success")
            };
            let TelemetryPayload::Span(span) = &delivery.payload else {
                panic!()
            };
            assert!(span.forward_unix_ms > 12);
            delivery_snapshot(delivery)
        })
        .collect::<Vec<_>>();
    let stamps = saved
        .iter()
        .map(|b| {
            serde_json::from_slice::<StageSpan>(b)
                .unwrap()
                .forward_unix_ms
        })
        .collect::<Vec<_>>();
    assert_eq!(stamps[0], stamps[1]);
    assert!(worker.flush_effects().is_err());
    assert!(drain(&mailbox).is_empty());
    // Test-only delivery recovery, not a production reconciliation API. The
    // pre-existing intents, not fresh clocks/state, supply both retry payloads.
    worker.state.next_event = 100;
    worker.effects_fenced = false;
    worker.flush_effects().unwrap();
    let retried = drain(&mailbox);
    assert_eq!(
        retried
            .iter()
            .map(|e| e.payload.clone())
            .collect::<Vec<_>>(),
        saved
    );
    assert!(
        retried
            .iter()
            .all(|e| e.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE)
    );
}

#[test]
fn closed_before_forward_retains_unstamped_result_and_all_recipients() {
    let (mut worker, _, _) = fixture(false);
    let (publisher, mailbox) = completion_mailbox(1);
    worker.publisher = publisher;
    drop(mailbox);
    let set = CapsuleSet(vec![physical(
        11,
        vec![owner("a", 0, route("a", 1)), owner("b", 1, route("b", 1))],
    )]);
    let base = incoming(&set);
    let bytes = set.encode().unwrap();
    let telemetry = worker
        .prepare_stage_span(&base, "observe", &set, 10, 11, 12, false)
        .unwrap();
    worker.effects.push_back(CommittedEffect::ForwardObserved {
        base: base.envelope,
        target: endpoint("last"),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body: bytes.clone(),
        telemetry,
    });
    assert!(worker.flush_effects().is_err());
    assert_eq!(worker.effects.len(), 1);
    let CommittedEffect::Publication {
        event,
        after: super::effects::PublicationAfter::Observed(telemetry),
    } = &worker.effects[0]
    else {
        panic!()
    };
    assert_eq!(event.payload, bytes);
    assert_eq!(event.envelope.target, endpoint("last"));
    assert_eq!(
        event.envelope.payload_content_type,
        PHYSICAL_BATCH_CONTENT_TYPE
    );
    assert_eq!(telemetry.len(), 2);
    for delivery in telemetry {
        let TelemetryPayload::Span(span) = &delivery.payload else {
            panic!()
        };
        assert_eq!(span.forward_unix_ms, 0);
    }
}

fn take_until(mailbox: &CompletionMailbox, deadline: Instant) -> Option<Event> {
    loop {
        if let Poll::Event(event) = mailbox.try_take() {
            return Some(event);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn actual_head_full_mailbox_recovers_without_reissuing_or_retimestamping_per_recipient() {
    let (mut worker, _, calls) = fixture(true);
    for id in ["a", "b"] {
        worker.handle(submission(id, route(id, 1))).unwrap();
    }
    let (publisher, mailbox) = completion_mailbox(1);
    publisher
        .try_publish(submission("occupied", route("occupied", 1)))
        .unwrap();
    worker.publisher = publisher;
    let snapshot = worker.snapshot.clone();
    let stop = worker.shutting_down.clone();
    let task = std::thread::spawn(move || {
        let result = worker.drive_one_batch();
        (worker, result)
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !snapshot
        .lock()
        .unwrap()
        .contains("completion_queue_full:waiting")
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(1));
    }
    let waited = snapshot
        .lock()
        .unwrap()
        .contains("completion_queue_full:waiting");
    let before_room = observe::unix_ms();
    let occupied = take_until(&mailbox, deadline);
    let mut events = Vec::new();
    for _ in 0..5 {
        if let Some(event) = take_until(&mailbox, deadline) {
            events.push(event);
        } else {
            break;
        }
    }
    if events.len() != 5 {
        stop.store(true, Ordering::Release);
    }
    drop(mailbox);
    let (worker, result) = task.join().unwrap();
    assert!(waited, "the first forward did not actually meet Full");
    assert_eq!(occupied.unwrap().envelope.event_id, "submit-occupied");
    assert_eq!(result, Ok(true));
    assert_eq!(*calls.lock().unwrap(), [Operation::LogicalBatch]);
    assert!(worker.effects.is_empty());
    assert!(!worker.effects_fenced);
    assert_eq!(events.len(), 5);
    assert_eq!(
        events[0].envelope.payload_content_type,
        PHYSICAL_BATCH_CONTENT_TYPE
    );
    let spans = events
        .iter()
        .filter(|e| e.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE)
        .map(|e| serde_json::from_slice::<StageSpan>(&e.payload).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].forward_unix_ms, spans[1].forward_unix_ms);
    assert!(spans[0].forward_unix_ms >= before_room);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.envelope.target == Endpoint::Outer(route("a", 1)))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.envelope.target == Endpoint::Outer(route("b", 1)))
            .count(),
        2
    );
}

#[test]
fn closed_after_one_observer_preserves_only_unsent_payload_with_the_same_forward_stamp() {
    let (mut worker, _, _) = fixture(false);
    let (publisher, mailbox) = completion_mailbox(4);
    worker.publisher = publisher;
    let set = CapsuleSet(vec![physical(
        11,
        vec![owner("a", 0, route("a", 1)), owner("b", 1, route("b", 1))],
    )]);
    let base = incoming(&set);
    let telemetry = worker
        .prepare_stage_span(&base, "observe", &set, 10, 11, 12, false)
        .unwrap();
    worker.effects.push_back(CommittedEffect::ForwardObserved {
        base: base.envelope,
        target: endpoint("last"),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body: set.encode().unwrap(),
        telemetry,
    });
    // Event-ID exhaustion supplies a deterministic partial-delivery barrier:
    // forward + first observer succeed, the second intent is not attempted.
    // Closing a mailbox after racing two sends would not fix this boundary.
    worker.state.next_event = u64::MAX - 2;
    assert!(worker.flush_effects().is_err());
    let delivered = drain(&mailbox);
    assert_eq!(delivered.len(), 2);
    assert_eq!(
        delivered[0].envelope.payload_content_type,
        PHYSICAL_BATCH_CONTENT_TYPE
    );
    assert_eq!(delivered[1].envelope.target, Endpoint::Outer(route("a", 1)));
    let first_span: StageSpan = serde_json::from_slice(&delivered[1].payload).unwrap();
    assert_eq!(worker.effects.len(), 1);
    let CommittedEffect::Telemetry(delivery) = &worker.effects[0] else {
        panic!("physical forward was repeated")
    };
    let saved = delivery_snapshot(delivery);
    let TelemetryPayload::Span(span) = &delivery.payload else {
        panic!()
    };
    assert_eq!(span.forward_unix_ms, first_span.forward_unix_ms);
    assert_eq!(delivery.reply.channel, "b");
    drop(mailbox);
    // Test-only attempt to deliver the retained suffix to a now-closed reader.
    // No production recovery/reconciliation API is claimed.
    worker.effects_fenced = false;
    worker.state.next_event = 100;
    assert!(worker.flush_effects().unwrap_err().contains("observation"));
    assert!(worker.effects_fenced);
    assert_eq!(worker.effects.len(), 1);
    let CommittedEffect::Publication {
        event,
        after: super::effects::PublicationAfter::Telemetry,
    } = &worker.effects[0]
    else {
        panic!("lost suffix")
    };
    assert_eq!(event.payload, saved);
    assert_eq!(event.envelope.target, Endpoint::Outer(route("b", 1)));
    assert_eq!(event.envelope.sequence, 100);
    let frozen = event.clone();
    let allocation = event.payload.as_ptr();
    assert!(worker.flush_effects().is_err());
    let CommittedEffect::Publication { event, .. } = &worker.effects[0] else {
        panic!("lost frozen suffix")
    };
    assert_eq!(event, &frozen);
    assert_eq!(event.payload.as_ptr(), allocation);
    assert_eq!(worker.state.next_event, 101);
}
