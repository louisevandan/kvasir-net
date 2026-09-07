//! ACK-consumer preconditions for the future yielding effect pump.
//! Pending controls and their declared progress below are constructed, not issued
//! by Worker::run. No native apply or forward is executed by this fixture. Actual
//! codec + handler consumption must distinguish incomplete from accepted progress.
//! These phase-precondition tests are not a live pump or phase-advancement proof;
//! loop_tests::effect_backpressure owns the independently observed Full case.
use super::super::state::{
    ControlDispatch, ControlDispatchPhase, PendingSettlement, ReadyRows, SettlementContinuation,
};
use super::*;
use crate::process::{ReadyInfo, ServerControl};
use p4_adapter::node_adapter::{CompletionMailbox, Poll};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct NativeHistory {
    starts: usize,
    readiness: usize,
    requests: Vec<Frame>,
    shutdowns: usize,
}

type History = Arc<Mutex<NativeHistory>>;

struct ObserveOnlyNative(History);

impl ServerControl for ObserveOnlyNative {
    fn start(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().starts += 1;
        Ok(())
    }

    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        self.0.lock().unwrap().readiness += 1;
        Ok(Some(ReadyInfo {
            protocol_revision: crate::PROTOCOL_REVISION,
            physical_identity_revision: 1,
            server_id: "ack-consumer-observer".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 1024,
            n_batch: 32,
            n_ubatch: 32,
            n_seq_max: 4,
            upstream_commit: "fixture-no-native-engine".into(),
            patch_set: "fixture-no-native-engine".into(),
            backend_inventory: "fixture-no-native-engine".into(),
        }))
    }

    fn request(&mut self, request: Frame) -> Result<Frame, String> {
        // Record the actual invocation including its body even if the ACK
        // consumer accidentally begins native work. There is no fake dedup.
        self.0.lock().unwrap().requests.push(request);
        Err("an ACK consumer must not execute a native control".into())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.0.lock().unwrap().shutdowns += 1;
        Ok(())
    }
}

fn attach_observer(worker: Worker) -> (Worker, History) {
    let history = Arc::new(Mutex::new(NativeHistory::default()));
    let worker = worker
        .with_stage_for_test(Box::new(ObserveOnlyNative(Arc::clone(&history))))
        .unwrap();
    assert_eq!(
        *history.lock().unwrap(),
        NativeHistory {
            starts: 1,
            readiness: 1,
            ..NativeHistory::default()
        }
    );
    (worker, history)
}

fn dispatch(phase: ControlDispatchPhase) -> ControlDispatch {
    ControlDispatch {
        load_generation: 1,
        session_id: "pipeline".into(),
        phase,
    }
}

fn business_snapshot(worker: &Worker) -> serde_json::Value {
    let mut value = super::release_tests::snapshot(worker);
    // A rejection publishes one diagnostic event; its identity is not a model,
    // slot, control or output-effect mutation. Everything else remains compared.
    value.as_object_mut().unwrap().remove("next_event");
    value["issue_witnesses"] = serde_json::json!(
        worker
            .state
            .requests
            .iter()
            .map(|(key, request)| (key, format!("{:?}", request.issued_work)))
            .collect::<Vec<_>>()
    );
    value
}

fn consume_and_require_rejection(
    worker: &mut Worker,
    mailbox: &CompletionMailbox,
    history: &History,
    event: Event,
) {
    let before = business_snapshot(worker);
    let native_before = history.lock().unwrap().clone();
    let next_event = worker.state.next_event;
    let bytes = p4_protocol::event::encode(&event).unwrap();
    let decoded = p4_protocol::event::decode(&bytes).unwrap();
    assert_eq!(p4_protocol::event::encode(&decoded).unwrap(), bytes);
    worker
        .handle(decoded)
        .expect("a rejected control must not kill the worker");
    let mut events = Vec::new();
    while let Poll::Event(event) = mailbox.try_take() {
        events.push(event);
    }
    eprintln!(
        "ack_consumer type={} replies={:?} preserved={} pending_release={} pending_settlement={} free={:?}",
        event.envelope.payload_content_type,
        events
            .iter()
            .map(|event| (
                &event.envelope.payload_content_type,
                String::from_utf8_lossy(&event.payload)
            ))
            .collect::<Vec<_>>(),
        business_snapshot(worker) == before,
        worker.state.pending_releases.len(),
        worker.state.pending_settlements.len(),
        worker.state.free_sequences
    );
    assert_eq!(
        business_snapshot(worker),
        before,
        "a registered but never dispatched ACK consumed pending authority or made work runnable"
    );
    assert_eq!(*history.lock().unwrap(), native_before);
    assert_eq!(worker.state.next_event, next_event + 1);
    assert_eq!(
        events.len(),
        1,
        "exactly one explicit rejection is required"
    );
    assert_eq!(events[0].envelope.payload_content_type, ERROR_CONTENT_TYPE);
    let payload: serde_json::Value = serde_json::from_slice(&events[0].payload).unwrap();
    assert_eq!(payload["code"], "LLAMA_ADAPTER_EVENT_REJECTED");
    assert_eq!(
        payload["detail"],
        if event.envelope.payload_content_type == RELEASED_CONTENT_TYPE {
            "release completion contains a non-owned sequence"
        } else {
            "settled sequence does not match its pending KV barrier"
        }
    );
    assert_eq!(
        events[0].envelope.correlation_id,
        event.envelope.correlation_id
    );
    assert_eq!(
        events[0].envelope.causation_id.as_deref(),
        Some(event.envelope.event_id.as_str())
    );
}

#[test]
fn release_ack_consumer_refuses_registered_but_never_applied_or_forwarded_controls() {
    for reverse in [false, true] {
        let (mut worker, mailbox) = super::release_tests::fixture();
        for pending in worker.state.pending_releases.values_mut() {
            pending.dispatch = dispatch(ControlDispatchPhase::Queued);
        }
        let (mut worker, history) = attach_observer(worker);
        let mut sequences = vec![
            super::release_tests::sequence("a", 0),
            super::release_tests::sequence("b", 1),
        ];
        if reverse {
            sequences.reverse();
        }
        consume_and_require_rejection(
            &mut worker,
            &mailbox,
            &history,
            super::release_tests::event(sequences),
        );
    }
}

fn settlement_fixture() -> (Worker, Arc<CompletionMailbox>, Vec<SettlementSequence>) {
    let (mut worker, mailbox) = super::release_tests::fixture();
    worker.state.pending_releases.clear();
    worker.state.physical_capacity = 32;
    worker.state.next_speculative_id = 10;
    let mut acknowledgements = Vec::new();
    for (name, id, replay) in [("a", 0, false), ("b", 1, true)] {
        let mut request = crate::v2::tests::request_state(vec![7; 4]);
        request.input_mut_for_test().command.session_id = "pipeline".into();
        request.input_mut_for_test().command.request_id = name.into();
        request.input_mut_for_test().command.load_generation = 1;
        request.input_mut_for_test().command.max_tokens = 16;
        request.sequence_id = Some(id);
        request.prompt_cursor = 4;
        request.prompt_issued = 4;
        request.generated = 2;
        request.outstanding = 0;
        request.ready = None;
        request.after_settlement = Some(if replay {
            SettlementContinuation::Replay(ReadyRows {
                phase: Phase::Replay,
                tokens: vec![7, 8],
                position: 4,
                speculative_id: 5,
            })
        } else {
            SettlementContinuation::Proposal {
                position: 6,
                token: 9,
            }
        });
        let sequence = SettlementSequence {
            incarnation: 1,
            operation_id: 1,
            key: request_key("pipeline", name),
            id,
            retain_from: 6,
            replay_tokens: if replay { vec![7, 8] } else { Vec::new() },
            replay_position: if replay { 4 } else { 0 },
            proposal: if replay { Vec::new() } else { vec![9, 10] },
        };
        let mut expected = sequence.clone();
        // The tail appends a direct proposal; requiring byte-equality with the
        // outgoing SETTLE would wrongly reject that legitimate continuation.
        expected.proposal.clear();
        worker.state.pending_settlements.insert(
            sequence.key.clone(),
            PendingSettlement {
                sequence: expected,
                dispatch: dispatch(ControlDispatchPhase::Queued),
            },
        );
        worker.state.requests.insert(sequence.key.clone(), request);
        acknowledgements.push(sequence);
    }
    (worker, mailbox, acknowledgements)
}

fn settlement_event(worker: &Worker, sequences: Vec<SettlementSequence>) -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template.clone();
    event.envelope.source = worker.state.sessions["pipeline"].last.clone();
    event.envelope.target = worker.endpoint.clone();
    event.envelope.class = EventClass::Control;
    event.envelope.payload_content_type = SETTLED_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&SettlementCommand {
        load_generation: 1,
        session_id: "pipeline".into(),
        sequences,
    })
    .unwrap();
    event
}

#[test]
fn settlement_ack_consumer_refuses_registered_but_never_applied_or_forwarded_controls() {
    for reverse in [false, true] {
        let (worker, mailbox, mut sequences) = settlement_fixture();
        let (mut worker, history) = attach_observer(worker);
        if reverse {
            sequences.reverse();
        }
        let event = settlement_event(&worker, sequences);
        consume_and_require_rejection(&mut worker, &mailbox, &history, event);
    }
}

fn release_accepted(
    worker: &mut Worker,
    mailbox: &CompletionMailbox,
    history: &History,
    mut event: Event,
) {
    let native_before = history.lock().unwrap().clone();
    let body = event.payload.clone();
    event.envelope.event_id = "accepted-release-retry".into();
    event.envelope.sequence += 1;
    let encoded = p4_protocol::event::encode(&event).unwrap();
    worker
        .handle(p4_protocol::event::decode(&encoded).unwrap())
        .unwrap();
    assert_eq!(event.payload, body);
    assert!(worker.state.pending_releases.is_empty());
    assert!(!worker.state.verify_fenced());
    let mut free = worker
        .state
        .free_sequences
        .iter()
        .copied()
        .collect::<Vec<_>>();
    free.sort_unstable();
    assert_eq!(free, vec![0, 1]);
    assert_eq!(*history.lock().unwrap(), native_before);
    let Poll::Event(receipt) = mailbox.try_take() else {
        panic!("accepted release must preserve owner notification");
    };
    assert_eq!(
        receipt.envelope.payload_content_type,
        RELEASE_RECEIPT_CONTENT_TYPE
    );
    let payload: ReleaseReceipt = serde_json::from_slice(&receipt.payload).unwrap();
    assert_eq!(payload.load_generation, 1);
    assert_eq!(payload.session_id, "pipeline");
    assert_eq!(payload.members.len(), 2);
    for (name, id) in [("a", 0), ("b", 1)] {
        let member = payload
            .members
            .iter()
            .find(|member| member.request_id == name)
            .unwrap();
        assert_eq!(
            member.submission_event_id,
            format!("original-submission-{name}")
        );
        assert_eq!(
            (member.sequence_id, member.incarnation, member.operation_id),
            (id, 1, 1)
        );
    }
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    assert!(worker.effects.is_empty());
}

fn settlement_accepted(
    worker: &mut Worker,
    mailbox: &CompletionMailbox,
    history: &History,
    mut event: Event,
) {
    let native_before = history.lock().unwrap().clone();
    let body = event.payload.clone();
    event.envelope.event_id = "accepted-settlement-retry".into();
    event.envelope.sequence += 1;
    let encoded = p4_protocol::event::encode(&event).unwrap();
    worker
        .handle(p4_protocol::event::decode(&encoded).unwrap())
        .unwrap();
    assert_eq!(event.payload, body);
    assert!(worker.state.pending_settlements.is_empty());
    assert!(!worker.state.verify_fenced());
    assert!(worker.state.free_sequences.is_empty());
    for (name, phase, tokens, position, speculative_id) in [
        ("a", Phase::Verify, vec![9, 10], 6, 10),
        ("b", Phase::Replay, vec![7, 8], 4, 5),
    ] {
        let request = &worker.state.requests[&request_key("pipeline", name)];
        assert_eq!(
            (
                request.generated,
                request.outstanding,
                request.prompt_cursor,
                request.prompt_issued
            ),
            (2, 0, 4, 4)
        );
        assert!(request.after_settlement.is_none());
        let ready = request.ready.as_ref().unwrap();
        assert_eq!(
            (ready.phase, ready.position, ready.speculative_id),
            (phase, position, speculative_id)
        );
        assert_eq!(ready.tokens, tokens);
    }
    assert_eq!(worker.state.next_speculative_id, 11);
    assert_eq!(*history.lock().unwrap(), native_before);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    assert!(worker.effects.is_empty());
}

#[test]
fn release_ack_consumer_validates_every_dispatch_phase_before_any_slot_return() {
    for phase in [
        ControlDispatchPhase::Queued,
        ControlDispatchPhase::LocalApplied,
    ] {
        for incomplete in ["a", "b"] {
            for reverse in [false, true] {
                let (worker, mailbox) = super::release_tests::fixture();
                let (mut worker, history) = attach_observer(worker);
                for pending in worker.state.pending_releases.values_mut() {
                    pending.dispatch = dispatch(ControlDispatchPhase::ForwardAccepted);
                }
                worker
                    .state
                    .pending_releases
                    .get_mut(&request_key("pipeline", incomplete))
                    .unwrap()
                    .dispatch = dispatch(phase);
                let mut sequences = vec![
                    super::release_tests::sequence("a", 0),
                    super::release_tests::sequence("b", 1),
                ];
                if reverse {
                    sequences.reverse();
                }
                let event = super::release_tests::event(sequences);
                consume_and_require_rejection(&mut worker, &mailbox, &history, event.clone());
                // Only the declared phase changes; the exact ACK/body and native
                // history stay fixed. This tests consumption, not phase progress.
                worker
                    .state
                    .pending_releases
                    .get_mut(&request_key("pipeline", incomplete))
                    .unwrap()
                    .dispatch
                    .phase = ControlDispatchPhase::ForwardAccepted;
                release_accepted(&mut worker, &mailbox, &history, event);
            }
        }
    }
}

#[test]
fn settlement_ack_consumer_validates_every_phase_without_losing_appended_proposal_or_replay() {
    for phase in [
        ControlDispatchPhase::Queued,
        ControlDispatchPhase::LocalApplied,
    ] {
        for incomplete in ["a", "b"] {
            for reverse in [false, true] {
                let (worker, mailbox, mut sequences) = settlement_fixture();
                let (mut worker, history) = attach_observer(worker);
                for pending in worker.state.pending_settlements.values_mut() {
                    pending.dispatch = dispatch(ControlDispatchPhase::ForwardAccepted);
                }
                worker
                    .state
                    .pending_settlements
                    .get_mut(&request_key("pipeline", incomplete))
                    .unwrap()
                    .dispatch = dispatch(phase);
                if reverse {
                    sequences.reverse();
                }
                let event = settlement_event(&worker, sequences);
                consume_and_require_rejection(&mut worker, &mailbox, &history, event.clone());
                worker
                    .state
                    .pending_settlements
                    .get_mut(&request_key("pipeline", incomplete))
                    .unwrap()
                    .dispatch
                    .phase = ControlDispatchPhase::ForwardAccepted;
                settlement_accepted(&mut worker, &mailbox, &history, event);
            }
        }
    }
}

#[test]
fn accepted_phase_does_not_authorize_a_different_release_load_or_session() {
    for bad_session in [false, true] {
        for reverse in [false, true] {
            let (worker, mailbox) = super::release_tests::fixture();
            let (mut worker, history) = attach_observer(worker);
            for pending in worker.state.pending_releases.values_mut() {
                pending.dispatch = dispatch(ControlDispatchPhase::ForwardAccepted);
            }
            let pending = worker
                .state
                .pending_releases
                .get_mut(&request_key("pipeline", "b"))
                .unwrap();
            if bad_session {
                pending.dispatch.session_id = "other-session".into();
            } else {
                pending.dispatch.load_generation = 2;
            }
            let mut sequences = vec![
                super::release_tests::sequence("a", 0),
                super::release_tests::sequence("b", 1),
            ];
            if reverse {
                sequences.reverse();
            }
            let event = super::release_tests::event(sequences);
            consume_and_require_rejection(&mut worker, &mailbox, &history, event.clone());
            worker
                .state
                .pending_releases
                .get_mut(&request_key("pipeline", "b"))
                .unwrap()
                .dispatch = dispatch(ControlDispatchPhase::ForwardAccepted);
            release_accepted(&mut worker, &mailbox, &history, event);
        }
    }
}

#[test]
fn accepted_phase_does_not_authorize_a_different_settlement_load_or_session() {
    for bad_session in [false, true] {
        for reverse in [false, true] {
            let (worker, mailbox, mut sequences) = settlement_fixture();
            let (mut worker, history) = attach_observer(worker);
            for pending in worker.state.pending_settlements.values_mut() {
                pending.dispatch = dispatch(ControlDispatchPhase::ForwardAccepted);
            }
            let pending = worker
                .state
                .pending_settlements
                .get_mut(&request_key("pipeline", "b"))
                .unwrap();
            if bad_session {
                pending.dispatch.session_id = "other-session".into();
            } else {
                pending.dispatch.load_generation = 2;
            }
            if reverse {
                sequences.reverse();
            }
            let event = settlement_event(&worker, sequences);
            consume_and_require_rejection(&mut worker, &mailbox, &history, event.clone());
            worker
                .state
                .pending_settlements
                .get_mut(&request_key("pipeline", "b"))
                .unwrap()
                .dispatch = dispatch(ControlDispatchPhase::ForwardAccepted);
            settlement_accepted(&mut worker, &mailbox, &history, event);
        }
    }
}
