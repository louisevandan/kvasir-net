use super::RunConfig;
use super::config::{AcceptanceConfig, ArrivalWave, NodeConfig};
use super::inference_identity::{InferenceIdentity, insert_observation};
use p4_llamacpp_staged_adapter::v2::{
    BatchObservation, BatchRequestObservation, OutcomePayload, PhysicalBatchObservation,
    ReleasedPayload,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, OuterEndpoint};
use std::collections::{BTreeMap, BTreeSet};

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
fn output_requires_exact_tail_route_and_contiguous_positions() {
    let mut config = config();
    config.acceptance.expected_prefill_rows = Some(8);
    let identity = InferenceIdentity::new(&config, &outer()).unwrap();
    let (first, tail) = endpoints();
    let valid = event(tail.clone(), EventClass::Output, "request");
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
                &event(first, EventClass::Output, "request"),
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
        observation_id: "session:11".into(),
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
            requests: vec![
                BatchRequestObservation {
                    request_id: "request".into(),
                    prefill_rows: 1,
                    decode_rows: 0,
                    verify_rows: 0,
                    replay_rows: 0,
                },
                BatchRequestObservation {
                    request_id: "request-2".into(),
                    prefill_rows: 0,
                    decode_rows: 1,
                    verify_rows: 0,
                    replay_rows: 0,
                },
            ],
        }],
        mixed_physical_batches: 1,
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
                &ReleasedPayload {
                    load_generation: 9,
                    session_id: "session".into(),
                    released: 1,
                },
                &known,
            )
            .is_ok()
    );
}

#[test]
fn duplicate_observation_identity_must_have_identical_payload() {
    let mut observations = BTreeMap::new();
    let original = observation();
    assert!(insert_observation(&mut observations, original.clone()).is_ok());
    assert!(insert_observation(&mut observations, original).is_ok());
    let mut changed = observation();
    changed.logical_rows = 3;
    assert!(insert_observation(&mut observations, changed).is_err());
}
