//! A refused teardown must not erase the run it was tearing down.
//!
//! The real consumption path is driven to a node-reported inference failure,
//! then the artifact is assembled with a teardown failure beside it. Before
//! this was fixed, `execute` propagated the teardown error with `?`, which
//! discarded the whole `InferenceResult` and made the caller write no
//! artifact - so the pressure scenario reported only "unload is busy" and
//! the failure that caused it was unrecoverable.
//!
//! This is not a native, model or transport fixture. The peer replies with a
//! scripted ERROR envelope; the teardown failure is supplied directly,
//! because whether UNLOAD refuses is the node's decision and not this test's
//! subject. The subject is what survives when it does.
use super::*;
use p4_llamacpp_staged_adapter::v2::{ERROR_CONTENT_TYPE, InferenceCommand, PREFILL_CONTENT_TYPE};
use std::time::Instant;
use tokio::io::duplex;

const NODE_ERROR: &str = "stage refused decode: sequence 41 has no resident slot";
const UNLOAD_REFUSAL: &str = "unload is busy; active_owners=224/256";

fn config() -> RunConfig {
    let node = |name: &str| config::NodeConfig {
        agent: "tcp://127.0.0.1:52501".into(),
        node: name.into(),
        generation: 3,
        binary: "not-started".into(),
        endpoint: "tcp://127.0.0.1:52502".into(),
        plan: "not-loaded".into(),
        args: vec![],
        environment: vec![],
        n_batch: 32,
        n_ubatch: 32,
        context_size: 128,
        total_context_size: 256,
        sequence_capacity: 2,
    };
    RunConfig {
        ingress_agent: "tcp://127.0.0.1:52501".into(),
        channel: "teardown-preserves-failure-test".into(),
        connection_generation: 7,
        load_generation: 9,
        session_id: "session".into(),
        request_id: "request".into(),
        nodes: vec![node("head"), node("tail")],
        prompt: String::new(),
        prompts: vec!["Explain Rust.".into()],
        session_key_template: String::new(),
        max_tokens: 4,
        waves: vec![ArrivalWave {
            after_ms: 0,
            count: 1,
        }],
        options: String::new(),
        pre_inference_hold_ms: 0,
        timeout_ms: 2_000,
        acceptance: AcceptanceConfig {
            minimum_generated_tokens: 1,
            expected_prefill_rows: None,
            allowed_stop_reasons: Vec::new(),
            responses: vec![ResponseExpectation {
                exact_response: Some("Normal response.".into()),
                ..Default::default()
            }],
        },
    }
}

/// Drive the production consumption path until a node reports an inference
/// error, and return what it produced.
async fn failed_run(config: &RunConfig) -> inference::InferenceResult {
    let head = node_endpoint(&config.nodes[0]).unwrap();
    let (client, peer) = duplex(128 * 1024);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = wire::EventWire::new(reader, writer);
    let mut sender = Sender::new(OuterEndpoint {
        ingress_agent: Address::from_str(&config.ingress_agent).unwrap(),
        channel: config.channel.clone(),
        connection_generation: config.connection_generation,
    });
    let peer_task = tokio::spawn(async move {
        let (reader, writer) = tokio::io::split(peer);
        let mut wire = wire::EventWire::new(reader, writer);
        let request = wire
            .receive(Instant::now() + Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(
            request.envelope.payload_content_type,
            PREFILL_CONTENT_TYPE,
            "the run must submit before it can fail"
        );
        let command: InferenceCommand = serde_json::from_slice(&request.payload).unwrap();
        command.validate().unwrap();
        let outer = request.envelope.return_route.clone().unwrap();
        wire.send(Event {
            envelope: Envelope {
                protocol_version: Envelope::VERSION,
                event_id: "stage-error-1".into(),
                correlation_id: request.envelope.correlation_id.clone(),
                causation_id: Some(request.envelope.event_id.clone()),
                source: head.clone(),
                target: Endpoint::Outer(outer.clone()),
                return_route: Some(outer),
                class: EventClass::Output,
                sequence: 1,
                deadline_unix_ms: None,
                adapter_kind: Some("llamacpp".into()),
                payload_content_type: ERROR_CONTENT_TYPE.into(),
            },
            payload: NODE_ERROR.as_bytes().to_vec(),
        })
        .await
        .unwrap();
    });
    let run = inference::drive(config, &mut wire, &mut sender)
        .await
        .expect("a node-reported error is a run outcome, not a transport failure");
    peer_task.await.unwrap();
    run
}

#[tokio::test]
async fn a_refused_teardown_does_not_replace_the_runs_own_failure() {
    let config = config();
    let run = failed_run(&config).await;
    assert_eq!(
        run.error.as_deref(),
        Some(NODE_ERROR),
        "the consumption path must surface the node's error as the run's error"
    );
    let submitted = run.request_count;
    assert_eq!(submitted, 1);

    let artifact = assemble(
        config,
        Default::default(),
        run,
        Some(UNLOAD_REFUSAL.to_string()),
    );

    assert_eq!(
        artifact.error.as_deref(),
        Some(NODE_ERROR),
        "the first failure is what the run is judged on and must survive teardown"
    );
    assert_eq!(
        artifact.cleanup_error.as_deref(),
        Some(UNLOAD_REFUSAL),
        "the teardown refusal is recorded, in its own field"
    );
    assert_ne!(
        artifact.error, artifact.cleanup_error,
        "one message must never stand in for the other"
    );
    assert_eq!(
        artifact.request_count, submitted,
        "the requests the run did submit are still counted"
    );
    assert!(
        !artifact.passed,
        "a refused teardown still fails the run; the guard is not relaxed"
    );
}

#[tokio::test]
async fn a_clean_teardown_after_a_failed_run_still_fails_and_names_only_the_run() {
    let config = config();
    let run = failed_run(&config).await;
    let artifact = assemble(config, Default::default(), run, None);
    assert_eq!(artifact.error.as_deref(), Some(NODE_ERROR));
    assert_eq!(
        artifact.cleanup_error, None,
        "no teardown failure must be invented when teardown succeeded"
    );
    assert!(!artifact.passed);
}

/// The other half of the contract. Keeping the teardown failure out of
/// `error` must not also keep it out of the verdict: a run whose own result
/// is clean still fails when its teardown was refused. Without this, moving
/// the error to its own field would quietly relax the UNLOAD guard.
#[tokio::test]
async fn a_refused_teardown_fails_an_otherwise_clean_run() {
    let mut config = config();
    config.prompts.clear();
    config.waves.clear();
    config.acceptance.responses.clear();
    let clean = || inference::InferenceResult {
        request_count: 0,
        completed_count: 0,
        released_count: 0,
        requests: vec![],
        batch_observations: vec![],
        stage_spans: vec![],
        elapsed_ms: 1,
        telemetry_complete_elapsed_ms: Some(1),
        error: None,
    };

    let control = assemble(config.clone(), Default::default(), clean(), None);
    assert!(
        control.passed,
        "positive control: this run must pass when teardown succeeds, or the \
         negative case below proves nothing: {:?}",
        control.acceptance
    );

    let refused = assemble(
        config,
        Default::default(),
        clean(),
        Some(UNLOAD_REFUSAL.to_string()),
    );
    assert!(
        !refused.passed,
        "a refused teardown must still fail the run"
    );
    assert_eq!(refused.error, None, "the run itself did not fail");
    assert_eq!(refused.cleanup_error.as_deref(), Some(UNLOAD_REFUSAL));
}
