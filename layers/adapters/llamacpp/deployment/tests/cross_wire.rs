//! Proves the sealed contract's wire actually connects: the real llama v2
//! submission-stream server -- spawned as a child `node` process, running
//! `apps/llama`'s own `attachRingInferenceStream` against a hand-rolled
//! backend, see `cross-wire-fixture.ts`'s doc comment for why that backend
//! is faked and nothing about the wire is -- driven by this crate's real
//! [`DeploymentClient`] over a real TCP socket, HTTP Upgrade handshake
//! included.
//!
//! Not two fakes agreeing with each other: every byte on the wire in this
//! test is produced and consumed by the same code each side runs in
//! production. A drift between the two contracts this checkpoint unified --
//! protocol name, reason casing, the `protocol` field, `request` as an
//! object rather than a string -- shows up here as a hang or a decode
//! failure, not as two mocks quietly agreeing on the wrong thing.

mod support;

use p4_adapter::deployment::{Client as DeploymentClientTrait, DeploymentEvent};
use p4_llamacpp_deployment::DeploymentClient;
use p4_llamacpp_deployment::contract::Submit;
use p4_llamacpp_deployment::transport::TransportFactory;
use p4_llamacpp_deployment::transport::tcp::TcpTransportFactory;
use std::sync::Arc;
use std::time::Duration;
use support::{CollectingSink, Fixture, neutral_request};

#[test]
fn a_submission_runs_two_in_flight_and_absorbs_full_over_a_real_socket() {
    if !support::cross_wire_fixture_ready("a_submission_runs_two_in_flight_and_absorbs_full_over_a_real_socket") {
        return;
    }
    let fixture = Fixture::spawn();
    let factory: Arc<dyn TransportFactory> = Arc::new(TcpTransportFactory::new(fixture.addr));
    let sink = CollectingSink::new();
    let client = DeploymentClient::connect(
        factory,
        sink.clone(),
        fixture.deployment_id.clone(),
        fixture.deployment_generation,
        Duration::from_millis(50),
    )
    .expect("connect: TCP + the HTTP Upgrade handshake the server requires");

    let submit = |submission_id: &str| {
        client
            .try_submit(Submit {
                deployment_id: fixture.deployment_id.clone(),
                deployment_generation: fixture.deployment_generation,
                submission_id: submission_id.into(),
                deadline_unix_ms: 0,
                request: neutral_request(),
            })
            .expect("try_submit");
    };

    // Proof point 2: two submissions in flight at once. Both reach Accepted
    // on the real server while neither has settled -- the cross-wire
    // equivalent of coordinator.test.ts's own
    // "a second submission reaches Accepted...before the first settles".
    submit("a");
    submit("b");
    sink.wait_for(Duration::from_secs(10), |events| {
        accepted_count(events) >= 2
    });
    assert!(settled_ids(&sink.snapshot()).is_empty());

    // Proof point 3: real capacity pressure stays behind the deployment
    // client. The third submission is refused while a and b occupy both
    // leases, then retried by the adapter and completes after one frees.
    // P4's sink must never observe the intermediate Full.
    submit("c");
    sink.wait_for(Duration::from_secs(10), |events| {
        settled_ids(events).len() == 3
    });
    assert!(
        client.full_retry_count() > 0,
        "the real backend never reported Full"
    );
    assert!(
        !sink.snapshot().iter().any(|event| matches!(event, DeploymentEvent::Rejected(rejected) if rejected.submission_id == "c")),
        "adapter-local capacity pressure escaped to P4"
    );

    // Proof point 1: the full lifecycle, per submission -- Accepted ->
    // Produced* (contiguous ordinals) -> Settled exactly once.
    client.close();

    let events = sink.snapshot();
    for submission_id in ["a", "b", "c"] {
        assert_produced_then_settled(&events, submission_id);
    }
}

fn accepted_count(events: &[DeploymentEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, DeploymentEvent::Accepted(_)))
        .count()
}

fn settled_ids(events: &[DeploymentEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            DeploymentEvent::Settled(settled) => Some(settled.submission_id.clone()),
            _ => None,
        })
        .collect()
}

/// Walks every event recorded for `submission_id` and confirms the shape
/// `SEALED-CONTRACT.md` §1 requires: `Produced.event_ordinal` contiguous
/// from zero, at least one `Produced`, and exactly one terminal `Settled`
/// after all of them.
fn assert_produced_then_settled(events: &[DeploymentEvent], submission_id: &str) {
    let mut next_ordinal = 0u64;
    let mut settled = false;
    for event in events {
        match event {
            DeploymentEvent::Produced(produced) if produced.submission_id == submission_id => {
                assert!(!settled, "{submission_id}: Produced arrived after Settled");
                assert_eq!(
                    produced.event_ordinal, next_ordinal,
                    "{submission_id}: ordinal gap"
                );
                next_ordinal += 1;
            }
            DeploymentEvent::Settled(result) if result.submission_id == submission_id => {
                assert!(!settled, "{submission_id}: Settled arrived more than once");
                settled = true;
            }
            _ => {}
        }
    }
    assert!(settled, "{submission_id}: never settled");
    assert!(next_ordinal > 0, "{submission_id}: never produced anything");
}
