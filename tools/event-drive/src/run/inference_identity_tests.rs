use super::RunConfig;
use super::config::{AcceptanceConfig, ArrivalWave, NodeConfig};
use super::inference_identity::InferenceIdentity;
use p4_llamacpp_staged_adapter::v2::{
    BatchObservation, BatchRequestObservation, OutcomePayload, PhysicalBatchObservation,
    ReleaseMember, ReleaseReceipt,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, OuterEndpoint};
use std::collections::BTreeSet;

#[path = "inference_output_contract_tests.rs"]
mod output_contract;

fn config() -> RunConfig {
    RunConfig {
        ingress_agent: "tcp://127.0.0.1:52000".into(),
        channel: "outer".into(),
        connection_generation: 7,
        load_generation: 9,
        session_id: "session".into(),
        request_id: "request".into(),
        nodes: vec![node("first"), node("tail")],
        prompt: "prompt".into(),
        prompts: Vec::new(),
        session_key_template: String::new(),
        max_tokens: 8,
        waves: vec![ArrivalWave {
            after_ms: 0,
            count: 1,
        }],
        options: String::new(),
        pre_inference_hold_ms: 0,
        acceptance: AcceptanceConfig::default(),
        timeout_ms: 1000,
        pipeline_compatibility: Default::default(),
    }
}

fn node(name: &str) -> NodeConfig {
    NodeConfig {
        agent: "tcp://127.0.0.1:52000".into(),
        node: name.into(),
        generation: 3,
        binary: "server".into(),
        endpoint: "tcp://127.0.0.1:53000".into(),
        plan: "plan".into(),
        args: Vec::new(),
        environment: Vec::new(),
        n_batch: 8,
        n_ubatch: 8,
        context_size: 8,
        total_context_size: 8,
        sequence_capacity: 1,
        resource_profile: super::config::test_resource_profile(),
    }
}

fn outer() -> OuterEndpoint {
    OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 52000),
        channel: "outer".into(),
        connection_generation: 7,
    }
}

fn event(source: Endpoint, class: EventClass, correlation: &str) -> Event {
    let outer = outer();
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: format!("event-{correlation}"),
            correlation_id: correlation.into(),
            causation_id: Some(format!("cause-{correlation}")),
            source,
            target: Endpoint::Outer(outer.clone()),
            return_route: Some(outer),
            class,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: "test".into(),
        },
        payload: Vec::new(),
    }
}

fn endpoints() -> (Endpoint, Endpoint) {
    let agent = Address::tcp("127.0.0.1", 52000);
    (
        Endpoint::node(agent.clone(), "first", 3),
        Endpoint::node(agent, "tail", 3),
    )
}

fn outcome(position: u32) -> OutcomePayload {
    OutcomePayload {
        load_generation: 9,
        session_id: "session".into(),
        request_id: "request".into(),
        sequence_id: 2,
        token: 42,
        text: "x".into(),
        position,
        stop: None,
    }
}

#[test]
fn output_requires_exact_approving_head_route_and_contiguous_positions() {
    let mut config = config();
    config.acceptance.expected_prefill_rows = Some(8);
    let identity = InferenceIdentity::new(&config, &outer()).unwrap();
    let (first, tail) = endpoints();
    // The tail sends physical decisions to the first worker. Only that head
    // approves request state and publishes the OUTER output stream.
    let valid = event(first, EventClass::Output, "request");
    assert!(identity.output(&valid, &outcome(8), None).is_ok());
    assert!(
        identity
            .output(&valid, &outcome(9), Some(&outcome(8)))
            .is_ok()
    );
    assert!(
        identity
            .output(&valid, &outcome(10), Some(&outcome(8)))
            .is_err()
    );
    assert!(
        identity
            .output(
                &event(tail, EventClass::Output, "request"),
                &outcome(8),
                None
            )
            .is_err()
    );
    let mut stale = outcome(8);
    stale.session_id = "stale".into();
    assert!(identity.output(&valid, &stale, None).is_err());
    assert!(identity.output(&valid, &outcome(7), None).is_err());
}

fn observation() -> BatchObservation {
    BatchObservation {
        scheduling: None,
        observation_id: "session:11".into(),
        logical_ordinal: 1,
        load_generation: 9,
        session_id: "session".into(),
        logical_rows: 2,
        physical_batches: vec![PhysicalBatchObservation {
            execution_id: 11,
            rows: 2,
            prefill_rows: 1,
            decode_rows: 1,
            verify_rows: 0,
            replay_rows: 0,
            request_count: 2,
            sequence_count: 2,
            owned_requests: vec![
                BatchRequestObservation {
                    request_id: "request".into(),
                    submission_event_id: "sent-request".into(),
                    sequence_id: 0,
                    incarnation: 1,
                    request_issue_index: 1,
                    rows: vec![p4_llamacpp_staged_adapter::v2::IssuedRow {
                        phase: p4_llamacpp_staged_adapter::v2::Phase::Prefill,
                        position: 0,
                    }],
                    prefill_rows: 1,
                    decode_rows: 0,
                    verify_rows: 0,
                    replay_rows: 0,
                },
                BatchRequestObservation {
                    request_id: "request-2".into(),
                    submission_event_id: "sent-request-2".into(),
                    sequence_id: 1,
                    incarnation: 1,
                    request_issue_index: 1,
                    rows: vec![p4_llamacpp_staged_adapter::v2::IssuedRow {
                        phase: p4_llamacpp_staged_adapter::v2::Phase::Decode,
                        position: 4,
                    }],
                    prefill_rows: 0,
                    decode_rows: 1,
                    verify_rows: 0,
                    replay_rows: 0,
                },
            ],
        }],
        mixed_physical_batches: 1,
        // The first node's pacing, which this fixture does not exercise: the
        // identity check reads dimensions and routing, not timings.
        stage_ms: 0,
        idle_ms: 0,
        idle_gated: 0,
        ready_rows: 0,
        ready_sequences: 0,
    }
}

#[test]
fn telemetry_requires_first_node_and_consistent_physical_counts() {
    let config = config();
    let identity = InferenceIdentity::new(&config, &outer()).unwrap();
    let (first, tail) = endpoints();
    let known = BTreeSet::from(["request".into(), "request-2".into()]);
    let batch_event = event(first.clone(), EventClass::Telemetry, "request");
    assert!(
        identity
            .observation(&batch_event, &observation(), &known)
            .is_ok()
    );
    let mut wrong = observation();
    wrong.physical_batches[0].decode_rows = 2;
    assert!(identity.observation(&batch_event, &wrong, &known).is_err());
    assert!(
        identity
            .observation(
                &event(tail, EventClass::Telemetry, "request"),
                &observation(),
                &known
            )
            .is_err()
    );
    assert!(
        identity
            .released(
                &event(first, EventClass::Telemetry, "request"),
                &ReleaseReceipt {
                    load_generation: 9,
                    session_id: "session".into(),
                    members: vec![ReleaseMember {
                        request_id: "request".into(),
                        submission_event_id: "sent-request".into(),
                        sequence_id: 2,
                        incarnation: 1,
                        operation_id: 1,
                    }],
                },
                &known,
            )
            .is_ok()
    );
}

#[test]
fn owned_projection_keeps_global_counts_but_cannot_invent_impossible_request_or_slot_counts() {
    let config = config();
    let identity = InferenceIdentity::new(&config, &outer()).unwrap();
    let first = endpoints().0;
    let known = BTreeSet::from(["request".into(), "request-2".into()]);
    let envelope = event(first, EventClass::Telemetry, "request");
    let mut projected = observation();
    projected.physical_batches[0].owned_requests.pop();
    identity.observation(&envelope, &projected, &known).unwrap();
    assert_eq!(projected.physical_batches[0].rows, 2);
    for mutation in 0..4 {
        let mut invalid = observation();
        let batch = &mut invalid.physical_batches[0];
        match mutation {
            0 => batch.request_count = 0,
            1 => batch.request_count = 3,
            2 => batch.sequence_count = 1,
            3 => batch.owned_requests[1].sequence_id = batch.owned_requests[0].sequence_id,
            _ => unreachable!(),
        }
        assert!(
            identity.observation(&envelope, &invalid, &known).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn duplicate_observation_identity_must_have_identical_payload() {
    use super::evidence_ledger::{EvidenceLedger, SubmittedAuthority};
    use p4_llamacpp_staged_adapter::v2::InferenceCommand;
    let mut ledger = EvidenceLedger::new(2);
    for request_id in ["request", "request-2"] {
        let mut input = event(endpoints().0, EventClass::Data, request_id);
        input.envelope.source = Endpoint::Outer(outer());
        input.envelope.target = endpoints().0;
        input.envelope.event_id = format!("sent-{request_id}");
        let command = InferenceCommand {
            load_generation: 9,
            session_id: "session".into(),
            request_id: request_id.into(),
            tokens: vec![42],
            prompt: None,
            options: String::new(),
            session_key: None,
            max_tokens: 1,
        };
        ledger.register(SubmittedAuthority::from_event(&input, &command).unwrap());
    }
    let envelope = event(endpoints().0, EventClass::Telemetry, "request");
    let original = observation();
    ledger.observation(&envelope, original.clone()).unwrap();
    let before = format!("{ledger:?}");
    ledger.observation(&envelope, original).unwrap();
    assert_eq!(format!("{ledger:?}"), before);
    let mut changed = observation();
    changed.logical_rows = 3;
    assert_eq!(
        ledger.observation(&envelope, changed).unwrap_err(),
        "duplicate observation identity changed its payload"
    );
    assert_eq!(format!("{ledger:?}"), before);
}
