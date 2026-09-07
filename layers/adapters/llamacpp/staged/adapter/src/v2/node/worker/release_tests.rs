//! Actual session/release/admission consumers. Source authority tests cross the
//! real protocol codec and generic broker before Worker::handle. Native RELEASE
//! receipts and the full async EventNode transport loop are not simulated here.
use super::*;
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};

pub(super) fn sequence(name: &str, id: u32) -> ReleaseSequence {
    ReleaseSequence {
        incarnation: 1,
        operation_id: 1,
        key: request_key("pipeline", name),
        id,
    }
}

fn stages() -> [NodeAddress; 3] {
    [("head", 1), ("middle", 1), ("tail", 3)].map(|(node, generation)| NodeAddress {
        agent: "tcp://127.0.0.1:42001".into(),
        node: node.into(),
        generation,
    })
}

fn graph() -> [Endpoint; 3] {
    [("head", 1), ("middle", 1), ("tail", 3)].map(|(node, generation)| {
        Endpoint::node(Address::tcp("127.0.0.1", 42001), node, generation)
    })
}

pub(super) fn fixture() -> (Worker, Arc<CompletionMailbox>) {
    let address = Address::tcp("127.0.0.1", 42001);
    let endpoint = Endpoint::node(address, "head", 1);
    let (_sender, receiver) = mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(8);
    let mut worker = Worker::new(
        endpoint.clone(),
        receiver,
        publisher,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    );
    worker.state.load_generation = 1;
    worker.state.sequence_capacity = 4;
    let mut install = crate::v2::tests::request_state(vec![7]).template;
    install.envelope.event_id = "release-test-install-session".into();
    install.envelope.source = Endpoint::outer(Address::tcp("127.0.0.1", 42001), "release-owner", 1);
    install.envelope.target = endpoint;
    install.envelope.class = EventClass::Control;
    install.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    install.payload = serde_json::to_vec(&SessionCommand {
        load_generation: 1,
        session_id: "pipeline".into(),
        stages: stages().to_vec(),
        stage_index: 0,
    })
    .unwrap();
    worker.handle(install).unwrap();
    let Poll::Event(ready) = mailbox.try_take() else {
        panic!("real SESSION installation must acknowledge readiness");
    };
    assert_eq!(
        ready.envelope.payload_content_type,
        SESSION_READY_CONTENT_TYPE
    );
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    let installed = &worker.state.sessions["pipeline"];
    // Authority is declared before any ACK, never learned from its sender.
    assert_eq!(installed.command.stages, stages());
    assert_eq!(installed.command.stage_index, 0);
    assert_eq!(installed.command.role(), NodeRole::First);
    assert_eq!(installed.first, graph()[0]);
    assert_eq!(installed.previous, None);
    assert_eq!(installed.next, Some(graph()[1].clone()));
    assert_eq!(installed.last, graph()[2]);
    assert_ne!(installed.next.as_ref(), Some(&installed.last));
    let owners = [sequence("a", 0), sequence("b", 1)];
    worker
        .state
        .begin_verify_fence(
            &owners
                .iter()
                .map(|owner| owner.key.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    for owner in owners {
        worker
            .state
            .pending_releases
            .insert(owner.key.clone(), pending(owner));
    }
    (worker, mailbox)
}

pub(super) fn pending(sequence: ReleaseSequence) -> super::super::state::PendingRelease {
    let request_id = sequence.key.split_once('\0').unwrap().1;
    let mut original = crate::v2::tests::request_state(vec![7]).template.envelope;
    original.event_id = format!("original-submission-{request_id}");
    original.correlation_id = "release-owned-pair".into();
    original.source = Endpoint::outer(Address::tcp("127.0.0.1", 42001), "release-owner", 1);
    original.target = graph()[0].clone();
    original.return_route = match &original.source {
        Endpoint::Outer(route) => Some(route.clone()),
        _ => unreachable!(),
    };
    original.class = EventClass::Data;
    original.payload_content_type = PREFILL_CONTENT_TYPE.into();
    let reply = ReplySpec {
        ingress_agent: "tcp://127.0.0.1:42001".into(),
        channel: "release-owner".into(),
        connection_generation: 1,
        correlation_id: original.correlation_id.clone(),
        deadline_unix_ms: original.deadline_unix_ms,
    };
    super::super::state::PendingRelease {
        sequence,
        original,
        reply,
        // Handcrafted post-forward consumer state. This helper is not native
        // or dispatch evidence; actual run-loop tests own that boundary.
        dispatch: super::super::state::ControlDispatch {
            load_generation: 1,
            session_id: "pipeline".into(),
            phase: super::super::state::ControlDispatchPhase::ForwardAccepted,
        },
    }
}

pub(super) fn event(sequences: Vec<ReleaseSequence>) -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template;
    event.envelope.source = graph()[2].clone();
    event.envelope.target = graph()[0].clone();
    event.envelope.class = EventClass::Telemetry;
    event.envelope.payload_content_type = RELEASED_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&ReleaseCommand {
        load_generation: 1,
        session_id: "pipeline".into(),
        sequences,
    })
    .unwrap();
    event
}

pub(super) fn snapshot(worker: &Worker) -> serde_json::Value {
    serde_json::json!({
        "sessions": worker.state.sessions.iter().map(|(key, session)| serde_json::json!({
            "key": key, "command": session.command, "first": format!("{:?}",session.first),
            "previous": format!("{:?}",session.previous), "next": format!("{:?}",session.next), "last": format!("{:?}",session.last),
        })).collect::<Vec<_>>(),
        "pending_releases": format!("{:?}", worker.state.pending_releases),
        "pending_settlements": worker.state.pending_settlements,
        "free": worker.state.free_sequences,
        "pending": worker.state.pending,
        "fences": [worker.state.verify_fence_matches(&sequence("a",0).key), worker.state.verify_fence_matches(&sequence("b",1).key)],
        "requests": worker.state.requests.iter().map(|(key,request)| serde_json::json!({
            "key": key, "command": request.command, "incarnation": request.incarnation,
            "sequence": request.sequence_id, "template": p4_protocol::event::encode(&request.template).unwrap(),
            "reply": request.reply, "prompt_cursor": request.prompt_cursor,
            "prompt_issued": request.prompt_issued, "generated": request.generated,
            "outstanding": request.outstanding, "ready": format!("{:?}", request.ready),
            "after_settlement": match &request.after_settlement {
                None => serde_json::Value::Null,
                Some(super::super::state::SettlementContinuation::Proposal {position, token}) => serde_json::json!({"position":position,"token":token}),
                Some(super::super::state::SettlementContinuation::Replay(rows)) => serde_json::json!({"replay":format!("{rows:?}")}),
            },
        })).collect::<Vec<_>>(),
        "next_event": worker.state.next_event,
        "effects": format!("{:?}", worker.effects), "fenced": worker.effects_fenced,
        "flights": format!("{:?}", worker.state.flights),
        "open_batches": worker.state.open_batches,
        "prepared_issue": worker.state.prepared_issue.as_ref().map(|issue| (issue.ordinal, format!("{:?}",issue.progress), format!("{:?}", issue.logical))),
        "owners": format!("{:?}", worker.state.stage_owners),
        "frontiers": format!("{:?}", worker.state.stage_frontiers),
        "receive": format!("{:?}", worker.state.physical_receives),
        "session_keys": format!("{:?}", worker.state.session_keys),
        "ordinals": [worker.state.load_generation, worker.state.last_load_generation, worker.state.next_incarnation, worker.state.next_control_operation, worker.state.next_speculative_id, worker.state.next_open_batch],
        "capacities": [worker.state.batch_capacity, worker.state.physical_capacity, worker.state.max_atomic_sequences, worker.state.context_size, worker.state.sequence_capacity as usize],
    })
}

#[test]
fn unowned_release_and_late_bad_member_leave_every_slot_and_fence_unchanged() {
    for reverse in [false, true] {
        for bad in [sequence("unknown", 1), sequence("b", 3), sequence("a", 0)] {
            let (mut worker, mailbox) = fixture();
            let before = snapshot(&worker);
            let mut members = vec![sequence("a", 0), bad];
            if reverse {
                members.reverse();
            }
            assert!(worker.released(event(members)).is_err());
            assert_eq!(snapshot(&worker), before);
            assert!(matches!(mailbox.try_take(), Poll::Empty));
        }
    }
}

#[test]
fn release_acknowledgement_reuses_only_owned_slots_once() {
    let (mut worker, mailbox) = fixture();
    let mut waiting = crate::v2::tests::request_state(vec![7]);
    waiting.command.session_id = "pipeline".into();
    waiting.command.request_id = "waiting".into();
    waiting.sequence_id = None;
    let key = request_key("pipeline", "waiting");
    worker.state.requests.insert(key.clone(), waiting);
    worker.state.pending.push_back(key.clone());
    worker
        .released(event(vec![sequence("a", 0), sequence("b", 1)]))
        .unwrap();
    assert_eq!(worker.state.requests[&key].sequence_id, Some(0));
    assert_eq!(
        worker
            .state
            .free_sequences
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert!(worker.state.pending_releases.is_empty());
    assert!(!worker.state.verify_fenced());
    assert!(matches!(mailbox.try_take(), Poll::Event(_)));
    let before = snapshot(&worker);
    assert!(
        worker
            .released(event(vec![sequence("a", 0), sequence("b", 1)]))
            .is_err()
    );
    assert_eq!(snapshot(&worker), before);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn invalid_pending_admission_cannot_partially_consume_release_or_free_slots() {
    for via_release in [false, true] {
        let (mut worker, mailbox) = fixture();
        let mut waiting = crate::v2::tests::request_state(vec![7]);
        waiting.sequence_id = None;
        worker.state.requests.insert("valid".into(), waiting);
        worker
            .state
            .pending
            .extend(["valid".into(), "missing".into()]);
        if !via_release {
            worker.state.free_sequences.extend([2, 3]);
        }
        let before = snapshot(&worker);
        let result = if via_release {
            worker.released(event(vec![sequence("a", 0), sequence("b", 1)]))
        } else {
            worker.admit_pending()
        };
        assert!(result.is_err());
        assert_eq!(snapshot(&worker), before);
        assert!(matches!(mailbox.try_take(), Poll::Empty));
    }
}

fn deliver_through_broker(worker: &mut Worker, incoming: Event) {
    use p4_agent_core::event_broker::{Delivery, DispatchOutcome, EventBroker, bounded_queue};
    let (agent, _agent_rx) = bounded_queue(8);
    let (outer, _outer_rx) = bounded_queue(8);
    let (outbound, _outbound_rx) = bounded_queue(8);
    let broker = EventBroker::new(Address::tcp("127.0.0.1", 42001), agent, outer, outbound, 16);
    let (node, mut node_rx) = bounded_queue(8);
    broker.register_node("head", 1, node).unwrap();
    let source = incoming.envelope.source.clone();
    let bytes = p4_protocol::event::encode(&incoming).unwrap();
    let decoded = p4_protocol::event::decode(&bytes).unwrap();
    assert_eq!(
        broker.dispatch(decoded).unwrap(),
        DispatchOutcome::Enqueued(Delivery::Node {
            node: "head".into(),
            generation: 1,
        }),
        "a valid target must reach the adapter's topology authority check"
    );
    let delivered = node_rx.try_recv().unwrap();
    assert_eq!(delivered.envelope.source, source);
    worker.handle(delivered).unwrap();
}

fn outputs(mailbox: &CompletionMailbox) -> Vec<Event> {
    let mut events = Vec::new();
    while let Poll::Event(event) = mailbox.try_take() {
        events.push(event);
    }
    events
}

fn business_snapshot(worker: &Worker) -> serde_json::Value {
    let mut value = snapshot(worker);
    // A controlled ERROR legitimately consumes one event ID and updates the
    // diagnostic status string. It must not consume business authority.
    value.as_object_mut().unwrap().remove("next_event");
    value
}

fn release_source_probe(source: Endpoint, tag: &str, should_reject: bool) {
    let (mut worker, mailbox) = fixture();
    let mut waiting = crate::v2::tests::request_state(vec![7]);
    waiting.command.session_id = "pipeline".into();
    waiting.command.request_id = "waiting".into();
    waiting.sequence_id = None;
    let waiting_key = request_key("pipeline", "waiting");
    worker.state.requests.insert(waiting_key.clone(), waiting);
    worker.state.pending.push_back(waiting_key.clone());
    let before = business_snapshot(&worker);
    let next_event = worker.state.next_event;
    let mut incoming = event(vec![sequence("a", 0), sequence("b", 1)]);
    incoming.envelope.event_id = format!("release-source-{tag}");
    incoming.envelope.correlation_id = "release-owned-pair".into();
    incoming.envelope.sequence = 1;
    incoming.envelope.source = source.clone();
    let same_body = incoming.payload.clone();
    deliver_through_broker(&mut worker, incoming.clone());
    let mut completed = outputs(&mailbox);
    if should_reject {
        assert_eq!(completed.len(), 1, "exactly one controlled rejection");
        assert_eq!(
            completed[0].envelope.payload_content_type, ERROR_CONTENT_TYPE,
            "wrong source must not free a slot or produce RELEASED"
        );
        let error: serde_json::Value = serde_json::from_slice(&completed[0].payload).unwrap();
        assert_eq!(error["code"], "LLAMA_ADAPTER_EVENT_REJECTED");
        assert_eq!(
            error["detail"],
            "release completion route does not match the declared pipeline"
        );
        assert_eq!(worker.state.next_event, next_event + 1);
        assert_eq!(
            business_snapshot(&worker),
            before,
            "rejection preserves requests, route caches, owners, flights, pending controls, slots, fences and effects"
        );
        // Rejecting everything is not a repair. The exact pending identity and
        // body must still succeed from the predeclared terminal stage.
        incoming.envelope.event_id = format!("release-source-{tag}-valid-retry");
        incoming.envelope.source = graph()[2].clone();
        incoming.envelope.sequence = 2;
        assert_eq!(incoming.payload, same_body);
        deliver_through_broker(&mut worker, incoming);
        completed = outputs(&mailbox);
    } else {
        assert_eq!(
            source,
            graph()[2],
            "the positive sender is the declared tail"
        );
    }
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0].envelope.payload_content_type,
        RELEASE_RECEIPT_CONTENT_TYPE
    );
    assert!(worker.state.pending_releases.is_empty());
    assert_eq!(worker.state.requests[&waiting_key].sequence_id, Some(0));
    assert_eq!(
        worker
            .state
            .free_sequences
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [1]
    );
    assert!(!worker.state.verify_fenced());
    assert!(!worker.effects_fenced);
    let released: ReleaseReceipt = serde_json::from_slice(&completed[0].payload).unwrap();
    assert_eq!(
        released,
        ReleaseReceipt {
            load_generation: 1,
            session_id: "pipeline".into(),
            members: vec![
                ReleaseMember {
                    request_id: "a".into(),
                    submission_event_id: "original-submission-a".into(),
                    sequence_id: 0,
                    incarnation: 1,
                    operation_id: 1
                },
                ReleaseMember {
                    request_id: "b".into(),
                    submission_event_id: "original-submission-b".into(),
                    sequence_id: 1,
                    incarnation: 1,
                    operation_id: 1
                },
            ],
        }
    );
}

#[test]
fn release_source_terminal_positive_survives_real_broker_and_worker_dispatch() {
    release_source_probe(graph()[2].clone(), "terminal", false);
}

#[test]
fn release_source_middle_is_not_the_tail_even_when_it_is_the_next_hop() {
    release_source_probe(graph()[1].clone(), "middle", true);
}

#[test]
fn release_source_outside_pipeline_cannot_return_slots_or_approve_release() {
    // A different Node envelope, not a claim about remote peer authentication.
    release_source_probe(
        Endpoint::node(Address::tcp("127.0.0.2", 42111), "external", 1),
        "external",
        true,
    );
}

#[test]
fn release_source_tail_with_an_old_node_generation_is_not_the_declared_tail() {
    // Load generation and the target's node generation remain valid. Only the
    // sender's node incarnation is old, so the broker cannot hide this case.
    release_source_probe(
        Endpoint::node(Address::tcp("127.0.0.1", 42001), "tail", 2),
        "old-tail-generation",
        true,
    );
}

#[test]
fn release_source_same_tail_name_on_a_different_agent_is_not_authority() {
    release_source_probe(
        Endpoint::node(Address::tcp("127.0.0.2", 42111), "tail", 3),
        "other-agent-tail",
        true,
    );
}
