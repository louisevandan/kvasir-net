//! SESSION is the independent topology authority, not a role inferred from ACK.
//! Post-LOAD worker fixture; this does not prove native load or fleet agreement.
use super::*;
use crate::v2::commands::ErrorPayload;
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};

fn command(index: usize) -> SessionCommand {
    SessionCommand {
        load_generation: 7,
        session_id: "declared-pipeline".into(),
        stages: ["head", "middle", "tail"]
            .into_iter()
            .map(|name| NodeAddress {
                agent: "tcp://127.0.0.1:43990".into(),
                node: name.into(),
                generation: 3,
            })
            .collect(),
        stage_index: index,
    }
}

fn endpoint(name: &str) -> Endpoint {
    Endpoint::node(Address::tcp("127.0.0.1", 43990), name, 3)
}

fn fixture(name: &str) -> (Worker, Arc<CompletionMailbox>) {
    let (_sender, receiver) = mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(8);
    let mut worker = Worker::new(
        endpoint(name),
        receiver,
        publisher,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    );
    worker.state.load_generation = 7;
    // This fixture injects post-LOAD state directly. Install the same
    // production request profile contract that `prefill` now requires.
    worker.state.resource_profile = Some(
        crate::v2::resource_profile::worker_fixture_resource_profile(1),
    );
    (worker, mailbox)
}

fn event(worker: &Worker, command: &SessionCommand) -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template.clone();
    event.envelope.target = worker.endpoint.clone();
    event.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(command).unwrap();
    p4_protocol::event::decode(&p4_protocol::event::encode(&event).unwrap()).unwrap()
}

fn response(mailbox: &CompletionMailbox) -> Event {
    match mailbox.try_take() {
        Poll::Event(event) => event,
        other => panic!("expected one SESSION response, got {other:?}"),
    }
}

type Installed = (
    String,
    SessionCommand,
    Option<Endpoint>,
    Endpoint,
    Option<Endpoint>,
    Endpoint,
);

fn installed(worker: &Worker) -> Vec<Installed> {
    worker
        .state
        .sessions
        .iter()
        .map(|(key, session)| {
            (
                key.clone(),
                session.command.clone(),
                session.next.clone(),
                session.first.clone(),
                session.previous.clone(),
                session.last.clone(),
            )
        })
        .collect()
}

// Only the loaded generation is injected. No ServerControl/native server is
// installed. These tests exercise SESSION/handle and the real completion Event
// codec, not LOAD, worker scheduling, a network hop, or future effect budgets.
#[derive(Debug, PartialEq, Eq)]
struct SessionEmissionSnapshot {
    next_event: u64,
    sessions: Vec<Installed>,
    effects: String,
    effects_fenced: bool,
    lifecycle: crate::lifecycle::LoadState,
    has_server: bool,
}

fn emission_snapshot(worker: &Worker) -> SessionEmissionSnapshot {
    SessionEmissionSnapshot {
        next_event: worker.state.next_event,
        sessions: installed(worker),
        effects: format!("{:?}", worker.effects),
        effects_fenced: worker.effects_fenced,
        lifecycle: worker.lifecycle.state(),
        has_server: worker.lifecycle.has_server(),
    }
}

fn assert_id_exhaustion_preserves_session(via_handle: bool) {
    let (mut worker, mailbox) = fixture("head");
    worker.state.next_event = u64::MAX;
    let before = emission_snapshot(&worker);
    assert_eq!(before.lifecycle, crate::lifecycle::LoadState::Empty);
    assert!(!before.has_server && before.sessions.is_empty());
    let input = event(&worker, &command(0));
    if via_handle {
        // The normal rejection ERROR also has no available ID. This tests
        // authority preservation, not successful delivery of that diagnostic.
        worker.handle(input).unwrap_err();
    } else {
        let error = worker.session(input).unwrap_err();
        assert!(error.contains("event ID is exhausted"), "{error}");
        assert!(!error.contains("queue is full"));
    }
    let after = emission_snapshot(&worker);
    assert_eq!(after.next_event, u64::MAX);
    assert_eq!(after.effects, before.effects);
    assert_eq!(after.effects_fenced, before.effects_fenced);
    assert_eq!(after.lifecycle, before.lifecycle);
    assert!(!after.has_server);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    assert_eq!(
        after.sessions, before.sessions,
        "a SESSION whose response ID cannot be prepared must not install routing authority"
    );
}

#[test]
fn session_id_exhaustion_preserves_routing_authority() {
    assert_id_exhaustion_preserves_session(false);
}

#[test]
fn handle_session_id_exhaustion_preserves_routing_authority() {
    assert_id_exhaustion_preserves_session(true);
}

#[test]
fn session_response_uses_the_prepared_id_and_exact_ready_wire() {
    let (mut worker, mailbox) = fixture("head");
    worker.state.next_event = 41;
    assert!(!worker.lifecycle.has_server());
    let expected_command = command(0);
    let input = event(&worker, &expected_command);
    let submission_id = input.envelope.event_id.clone();
    let correlation = input.envelope.correlation_id.clone();
    let target = Endpoint::Outer(input.envelope.return_route.clone().unwrap());
    worker.handle(input).unwrap();
    let actual = response(&mailbox);
    let wire = p4_protocol::event::encode(&actual).unwrap();
    let decoded = p4_protocol::event::decode(&wire).unwrap();
    assert_eq!(decoded, actual);
    assert_eq!(
        wire.len(),
        340,
        "the existing ready fixture wire is unchanged"
    );
    assert_eq!(
        actual.envelope.event_id,
        format!("{submission_id}:llamacpp:41")
    );
    assert_eq!(actual.envelope.sequence, 41);
    assert_eq!(
        actual.envelope.causation_id.as_deref(),
        Some(submission_id.as_str())
    );
    assert_eq!(actual.envelope.correlation_id, correlation);
    assert_eq!(actual.envelope.source, endpoint("head"));
    assert_eq!(actual.envelope.target, target);
    assert_eq!(actual.envelope.class, EventClass::Telemetry);
    assert_eq!(
        actual.envelope.payload_content_type,
        SESSION_READY_CONTENT_TYPE
    );
    assert_eq!(
        actual.payload,
        br#"{"load_generation":7,"session_id":"declared-pipeline","state":"ready"}"#
    );
    assert_eq!(worker.state.next_event, 42);
    assert_eq!(installed(&worker).len(), 1);
    assert_eq!(
        worker.state.sessions["declared-pipeline"].command,
        expected_command
    );
    assert!(worker.effects.is_empty() && !worker.effects_fenced);
    assert!(!worker.lifecycle.has_server());
    assert_eq!(worker.lifecycle.state(), crate::lifecycle::LoadState::Empty);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

fn input_whose_ready_envelope_cannot_round_trip(worker: &Worker) -> Event {
    let mut input = event(worker, &command(0));
    // Each response repeats this ID as both the derived event ID and the
    // causation ID. Individual fields fit; their aggregate response does not.
    input.envelope.event_id = "x".repeat(140_000);
    let wire = p4_protocol::event::encode(&input).unwrap();
    let decoded = p4_protocol::event::decode(&wire).unwrap();
    assert_eq!(decoded, input, "the submitted SESSION itself is wire-valid");
    decoded
}

#[test]
fn session_undecodable_ready_envelope_preserves_id_and_authority() {
    let (mut worker, mailbox) = fixture("head");
    worker.state.next_event = 41;
    let input = input_whose_ready_envelope_cannot_round_trip(&worker);
    let before = emission_snapshot(&worker);
    let error = worker.session(input).unwrap_err();
    assert!(
        error.contains("completion event cannot be decoded"),
        "{error}"
    );
    assert_eq!(emission_snapshot(&worker), before);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn handle_undecodable_ready_does_not_install_session_authority() {
    let (mut worker, mailbox) = fixture("head");
    worker.state.next_event = 41;
    let input = input_whose_ready_envelope_cannot_round_trip(&worker);
    let before = emission_snapshot(&worker);
    worker.handle(input.clone()).unwrap_err();
    let after = emission_snapshot(&worker);
    assert_eq!(after.sessions, before.sessions);
    assert!(after.effects_fenced);
    assert_eq!(after.lifecycle, before.lifecycle);
    assert!(!after.has_server);
    // This exact input also makes its ERROR envelope undecodable. Retain the
    // diagnostic and provenance instead of pretending a malformed response
    // was delivered; no response number has been assigned.
    assert_eq!(after.next_event, before.next_event);
    assert_eq!(worker.effects.len(), 1);
    let super::effects::CommittedEffect::UndeliverableDirect { intent, detail } =
        &worker.effects[0]
    else {
        panic!("the failed ERROR preflight must remain owned")
    };
    assert_eq!(intent.base, input.envelope);
    assert_eq!(intent.source, worker.endpoint);
    assert_eq!(intent.target, reply_target(&input).unwrap());
    assert_eq!(intent.class, EventClass::Output);
    assert_eq!(intent.content_type, ERROR_CONTENT_TYPE);
    assert!(detail.contains("completion event cannot be decoded"));
    let payload: ErrorPayload = serde_json::from_slice(&intent.body).unwrap();
    assert_eq!(payload.code, "LLAMA_ADAPTER_EVENT_REJECTED");
    assert!(
        payload
            .detail
            .contains("completion event cannot be decoded")
    );
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    assert_eq!(worker.ensure_event_id_obligations(0, 0), Ok(()));
}

#[test]
fn prepared_session_response_preserves_unicode_metadata_and_body() {
    let (mut worker, mailbox) = fixture("head");
    worker.state.next_event = 41;
    let mut expected = command(0);
    expected.session_id = "세션-🙂".into();
    let mut input = event(&worker, &expected);
    input.envelope.event_id = "원본-🙂".into();
    input.envelope.correlation_id = "요청-다국어".into();
    input.envelope.return_route.as_mut().unwrap().channel = "응답-🙂".into();
    input.envelope.source = Endpoint::Outer(input.envelope.return_route.clone().unwrap());
    let input_wire = p4_protocol::event::encode(&input).unwrap();
    let input = p4_protocol::event::decode(&input_wire).unwrap();
    worker.handle(input).unwrap();
    let actual = response(&mailbox);
    let wire = p4_protocol::event::encode(&actual).unwrap();
    assert_eq!(p4_protocol::event::decode(&wire).unwrap(), actual);
    assert_eq!(actual.envelope.event_id, "원본-🙂:llamacpp:41");
    assert_eq!(actual.envelope.causation_id.as_deref(), Some("원본-🙂"));
    assert_eq!(actual.envelope.correlation_id, "요청-다국어");
    let Endpoint::Outer(target) = actual.envelope.target else {
        panic!("reply must target OUTER")
    };
    assert_eq!(target.channel, "응답-🙂");
    assert_eq!(
        actual.payload,
        "{\"load_generation\":7,\"session_id\":\"세션-🙂\",\"state\":\"ready\"}".as_bytes()
    );
    assert_eq!(worker.state.sessions["세션-🙂"].command, expected);
    assert_eq!(worker.state.next_event, 42);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn session_installs_all_roles_from_the_same_ordered_pipeline() {
    for (index, name, role, previous, next) in [
        (0, "head", NodeRole::First, None, Some("middle")),
        (1, "middle", NodeRole::Middle, Some("head"), Some("tail")),
        (2, "tail", NodeRole::Last, Some("middle"), None),
    ] {
        let (mut worker, mailbox) = fixture(name);
        let command = command(index);
        worker.handle(event(&worker, &command)).unwrap();
        assert_eq!(
            response(&mailbox).envelope.payload_content_type,
            SESSION_READY_CONTENT_TYPE
        );
        let current = &worker.state.sessions["declared-pipeline"];
        assert_eq!(current.command, command);
        assert_eq!(current.command.role(), role);
        assert_eq!(current.first, endpoint("head"));
        assert_eq!(current.last, endpoint("tail"));
        assert_eq!(current.previous, previous.map(endpoint));
        assert_eq!(current.next, next.map(endpoint));
        let before = installed(&worker);
        worker.handle(event(&worker, &command)).unwrap();
        assert_eq!(
            response(&mailbox).envelope.payload_content_type,
            SESSION_READY_CONTENT_TYPE
        );
        assert_eq!(
            installed(&worker),
            before,
            "identical SESSION must not mutate authority"
        );
    }
}

#[test]
fn malformed_or_rebound_pipeline_cannot_replace_installed_authority() {
    let base = command(0);
    let mut candidates = Vec::new();
    let mut wrong = base.clone();
    wrong.stages.clear();
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stages.truncate(1);
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stage_index = 3;
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stage_index = 1;
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stages[0].generation += 1;
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stages[2] = wrong.stages[1].clone();
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stages[2] = wrong.stages[1].clone();
    wrong.stages[2].generation += 1;
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stages[2].agent = "not an address".into();
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stages[2].node.clear();
    candidates.push(wrong);
    let mut wrong = base.clone();
    wrong.stages[2].generation = 0;
    candidates.push(wrong);
    // Check invalid declarations before any route exists. Testing only after
    // installation would let immutability hide a missing validation check.
    for wrong in &candidates {
        let (mut worker, mailbox) = fixture("head");
        worker.handle(event(&worker, wrong)).unwrap();
        assert_eq!(
            response(&mailbox).envelope.payload_content_type,
            ERROR_CONTENT_TYPE,
            "{wrong:?}"
        );
        assert!(worker.state.sessions.is_empty());
        assert!(worker.effects.is_empty());
    }
    let mut wrong = base.clone();
    wrong.stages[2].node = "replacement-tail".into();
    candidates.push(wrong);
    for wrong in candidates {
        let (mut worker, mailbox) = fixture("head");
        worker.handle(event(&worker, &base)).unwrap();
        assert_eq!(
            response(&mailbox).envelope.payload_content_type,
            SESSION_READY_CONTENT_TYPE
        );
        let before = installed(&worker);
        worker.handle(event(&worker, &wrong)).unwrap();
        assert_eq!(
            response(&mailbox).envelope.payload_content_type,
            ERROR_CONTENT_TYPE,
            "{wrong:?}"
        );
        assert_eq!(installed(&worker), before);
        assert!(worker.effects.is_empty());
    }
}

#[test]
fn wrong_local_index_is_rejected_even_before_a_session_exists() {
    let (mut worker, mailbox) = fixture("head");
    worker.handle(event(&worker, &command(1))).unwrap();
    let error = response(&mailbox);
    assert_eq!(error.envelope.payload_content_type, ERROR_CONTENT_TYPE);
    assert!(
        String::from_utf8(error.payload)
            .unwrap()
            .contains("local index")
    );
    assert!(worker.state.sessions.is_empty());

    // Correct body and index do not authorize delivery to another worker.
    let mut wrong_target = event(&worker, &command(0));
    wrong_target.envelope.target = endpoint("middle");
    worker.handle(wrong_target).unwrap();
    let error = response(&mailbox);
    assert_eq!(error.envelope.payload_content_type, ERROR_CONTENT_TYPE);
    let error: ErrorPayload = serde_json::from_slice(&error.payload).unwrap();
    assert_eq!(
        error.detail,
        "session local index does not name this worker endpoint"
    );
    assert!(worker.state.sessions.is_empty());
}

#[test]
fn legacy_session_type_and_legacy_body_cannot_install_authority() {
    for legacy_type in [true, false] {
        let (mut worker, mailbox) = fixture("head");
        let mut input = event(&worker, &command(0));
        if legacy_type {
            input.envelope.payload_content_type =
                "application/vnd.p4.llamacpp.session-v3+json".into();
        } else {
            input.payload = serde_json::to_vec(&serde_json::json!({
                "load_generation": 7, "session_id": "declared-pipeline", "role": "first",
                "first": command(0).stages[0], "next": command(0).stages[1],
            }))
            .unwrap();
        }
        worker.handle(input).unwrap();
        assert_eq!(
            response(&mailbox).envelope.payload_content_type,
            ERROR_CONTENT_TYPE
        );
        assert!(worker.state.sessions.is_empty());
    }
}

fn capsule_body(terminal: bool) -> Vec<u8> {
    CapsuleSet(vec![PhysicalCapsule {
        execution_id: 1,
        terminal,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: 1,
            n_seqs: 1,
            n_seqs_unq: 1,
            n_pos: 1,
            positions: vec![0],
            sequence_counts: vec![1],
            sequence_ids: vec![0],
            output: vec![false],
        },
        owners: vec![RowOwner {
            load_generation: 7,
            incarnation: 1,
            request_id: "request".into(),
            sequence_key: request_key("declared-pipeline", "request"),
            session_id: "declared-pipeline".into(),
            reply: "{}".into(),
            sequence_id: 0,
            phase: Phase::Prefill,
            position: 0,
            max_tokens: 3,
            generated_tokens: 0,
            output: false,
            input_token: 7,
            speculative_id: 0,
            speculative_index: 0,
            speculative_count: 0,
            options: "{}".into(),
        }],
        tensors: if terminal {
            vec![]
        } else {
            vec![Tensor {
                descriptor: TensorDescriptor {
                    tensor_type: 0,
                    dimensions: vec![1],
                    strides: vec![4],
                    nbytes: 4,
                    view_offset: 0,
                    alias_of: None,
                    name: "cut-set".into(),
                },
                data: vec![7, 0, 0, 0],
            }]
        },
        outcomes: vec![],
    }])
    .encode()
    .unwrap()
}

#[test]
fn all_stage_message_families_check_declared_source_and_actual_recipient() {
    let release = serde_json::to_vec(&ReleaseCommand {
        load_generation: 7,
        session_id: "declared-pipeline".into(),
        sequences: vec![ReleaseSequence {
            key: request_key("declared-pipeline", "request"),
            id: 0,
            incarnation: 1,
            operation_id: 1,
        }],
    })
    .unwrap();
    let settle = serde_json::to_vec(&SettlementCommand {
        load_generation: 7,
        session_id: "declared-pipeline".into(),
        sequences: vec![SettlementSequence {
            key: request_key("declared-pipeline", "request"),
            id: 0,
            incarnation: 1,
            operation_id: 1,
            retain_from: 0,
            replay_tokens: vec![],
            replay_position: 0,
            proposal: vec![],
        }],
    })
    .unwrap();
    // This matrix checks the envelope gate, not native execution. The separate
    // post-LOAD loop tests retain successful execution/release/settlement proofs.
    for (index, recipient, source, content_type, body, operation) in [
        (
            1,
            "middle",
            "head",
            PHYSICAL_BATCH_CONTENT_TYPE,
            capsule_body(false),
            "physical batch",
        ),
        (
            1,
            "middle",
            "head",
            RELEASE_CONTENT_TYPE,
            release.clone(),
            "release",
        ),
        (
            1,
            "middle",
            "head",
            SETTLE_CONTENT_TYPE,
            settle.clone(),
            "settlement",
        ),
        (
            0,
            "head",
            "tail",
            TAIL_BATCH_CONTENT_TYPE,
            capsule_body(true),
            "tail batch",
        ),
        (
            0,
            "head",
            "tail",
            RELEASED_CONTENT_TYPE,
            release,
            "release completion",
        ),
        (
            0,
            "head",
            "tail",
            SETTLED_CONTENT_TYPE,
            settle,
            "settlement completion",
        ),
    ] {
        for wrong_target in [false, true] {
            let (mut worker, mailbox) = fixture(recipient);
            worker.handle(event(&worker, &command(index))).unwrap();
            assert_eq!(
                response(&mailbox).envelope.payload_content_type,
                SESSION_READY_CONTENT_TYPE
            );
            let before = installed(&worker);
            let mut input = event(&worker, &command(index));
            input.envelope.class = EventClass::Control;
            input.envelope.payload_content_type = content_type.into();
            input.envelope.source = endpoint(source);
            input.payload = body.clone();
            if wrong_target {
                input.envelope.target = endpoint("other-recipient");
            } else {
                input.envelope.source = endpoint("other-source");
            }
            let decoded =
                p4_protocol::event::decode(&p4_protocol::event::encode(&input).unwrap()).unwrap();
            worker.handle(decoded).unwrap();
            let error = response(&mailbox);
            assert_eq!(error.envelope.payload_content_type, ERROR_CONTENT_TYPE);
            let error: ErrorPayload = serde_json::from_slice(&error.payload).unwrap();
            assert_eq!(
                error.detail,
                format!("{operation} route does not match the declared pipeline")
            );
            assert_eq!(installed(&worker), before);
            assert!(worker.effects.is_empty() && !worker.effects_fenced);
            assert!(
                worker.state.pending_releases.is_empty()
                    && worker.state.pending_settlements.is_empty()
            );
            assert!(worker.state.requests.is_empty() && worker.state.free_sequences.is_empty());
            assert!(!worker.state.any_in_flight() && !worker.state.verify_fenced());
            assert!(matches!(mailbox.try_take(), Poll::Empty));
        }
    }
}

#[test]
fn prefill_owner_declarations_are_checked_before_remembering_or_admitting_the_attempt() {
    for wrong_target in [false, true] {
        let (mut worker, mailbox) = fixture("head");
        worker.state.context_size = 32;
        worker.state.sequence_capacity = 1;
        worker.state.free_sequences.push_back(0);
        worker.handle(event(&worker, &command(0))).unwrap();
        assert_eq!(
            response(&mailbox).envelope.payload_content_type,
            SESSION_READY_CONTENT_TYPE
        );
        let mut input = event(&worker, &command(0));
        input.envelope.event_id = "original-submission".into();
        input.envelope.source = Endpoint::outer(Address::tcp("127.0.0.1", 43991), "owner", 5);
        let Endpoint::Outer(route) = &input.envelope.source else {
            unreachable!()
        };
        input.envelope.return_route = Some(route.clone());
        input.envelope.class = EventClass::Data;
        input.envelope.payload_content_type = PREFILL_CONTENT_TYPE.into();
        input.payload = serde_json::to_vec(&InferenceCommand {
            load_generation: 7,
            session_id: "declared-pipeline".into(),
            request_id: "binding".into(),
            tokens: vec![7],
            prompt: None,
            options: "{}".into(),
            session_key: Some("sk1:owner/binding".into()),
            max_tokens: 1,
        })
        .unwrap();
        let mut bad = input.clone();
        if wrong_target {
            bad.envelope.target = endpoint("middle");
        } else {
            bad.envelope.source = Endpoint::outer(Address::tcp("127.0.0.1", 43992), "other", 5);
        }
        let incarnation = worker.state.next_incarnation;
        let decoded = if wrong_target {
            p4_protocol::event::decode(&p4_protocol::event::encode(&bad).unwrap()).unwrap()
        } else {
            // P4 now refuses the contradictory OUTER identity at the wire
            // boundary. Still exercise the adapter's independent admission
            // check with the same counterexample through its direct handler.
            assert!(p4_protocol::event::encode(&bad).is_err());
            bad
        };
        worker.handle(decoded).unwrap();
        let error = response(&mailbox);
        let error: ErrorPayload = serde_json::from_slice(&error.payload).unwrap();
        assert_eq!(
            error.detail,
            if wrong_target {
                "inference target does not name this worker endpoint"
            } else {
                "inference source does not match its OUTER return route"
            }
        );
        assert!(worker.state.requests.is_empty() && worker.state.pending.is_empty());
        assert!(worker.state.session_keys.is_empty() && worker.effects.is_empty());
        assert_eq!(worker.state.next_incarnation, incarnation);
        assert_eq!(worker.state.free_sequences, [0]);
        // Correct declarations must still admit through the actual handler.
        worker.handle(input).unwrap();
        assert!(matches!(mailbox.try_take(), Poll::Empty));
        let request = &worker.state.requests[&request_key("declared-pipeline", "binding")];
        assert_eq!(request.sequence_id, Some(0));
        assert_eq!(request.template.envelope.event_id, "original-submission");
    }
}

#[path = "prefill_admission_tests.rs"]
mod prefill_admission_tests;
