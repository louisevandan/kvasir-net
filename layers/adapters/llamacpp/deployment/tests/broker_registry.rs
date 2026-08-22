//! Proves item 4 of the integration task: a real request crossing the new
//! path, driven through code that is actually P4's own -- not this crate's
//! `DeploymentClient::try_submit` called directly (that is `cross_wire.rs`'s
//! job), but `p4_adapter::deployment::Registry::try_submit`, the
//! `DeploymentId`-keyed dispatcher a P4 broker holds per
//! `SEALED-CONTRACT.md` §9.1/§9.4. `apps/p4/entrypoints/agent`'s own wiring
//! of this registry lives at `entrypoints/agent/src/adapters/mod.rs` and
//! cannot be exercised from here (this crate is not yet that entrypoint's
//! cargo dependency -- a root-only registration, `CLAUDE.md`'s "Validation
//! and delivery"/`SEALED-CONTRACT.md` §7), but `p4_adapter::deployment`
//! itself already is this crate's dependency, so the registry -- P4's own
//! generic broker surface, not a backend-specific stand-in for it -- can be
//! driven for real, over a real socket, against the real llama-path server,
//! from right here.

mod support;

use p4_adapter::deployment::{Client as DeploymentClientTrait, DeploymentEvent, Registry};
use p4_llamacpp_deployment::DeploymentClient;
use p4_llamacpp_deployment::contract::Submit;
use p4_llamacpp_deployment::transport::TransportFactory;
use p4_llamacpp_deployment::transport::tcp::TcpTransportFactory;
use std::sync::Arc;
use std::time::Duration;
use support::{CollectingSink, Fixture, chat_request};

#[test]
fn a_p4_registry_dispatches_a_real_submission_through_the_real_client_to_a_real_backend() {
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

    // This is the whole of what `apps/p4/entrypoints/agent/src/adapters`
    // does once root's Cargo.toml registration lands: build a client, and
    // register it under its DeploymentId. Nothing else about wiring this in
    // is backend-specific.
    let registry = Registry::new();
    registry.register(
        fixture.deployment_id.clone(),
        client.clone() as Arc<dyn DeploymentClientTrait>,
    );

    // The call a P4 broker actually makes: not `client.try_submit`, but
    // `registry.try_submit` -- selecting the adapter instance by
    // `DeploymentId` first, exactly as SEALED-CONTRACT.md section 9.1
    // describes, then handing the Submit to its bounded queue.
    registry
        .try_submit(Submit {
            deployment_id: fixture.deployment_id.clone(),
            deployment_generation: fixture.deployment_generation,
            submission_id: "p4-broker-1".into(),
            request: chat_request(),
        })
        .expect("registry dispatches to the registered client");

    sink.wait_for(Duration::from_secs(10), |events| {
        events.iter().any(|event| {
            matches!(event, DeploymentEvent::Settled(settled) if settled.submission_id == "p4-broker-1")
        })
    });

    let events = sink.snapshot();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, DeploymentEvent::Accepted(a) if a.submission_id == "p4-broker-1")),
        "expected an Accepted for the registry-dispatched submission: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, DeploymentEvent::Produced(p) if p.submission_id == "p4-broker-1")),
        "expected the real backend to have produced at least one token: {events:?}"
    );

    // Cancel through the registry too -- the other half of what a P4
    // broker calls directly per SEALED-CONTRACT.md section 9.1.
    let found = registry.cancel(&fixture.deployment_id, "no-such-submission".into());
    assert!(found, "registry.cancel must reach the registered client");

    client.close();
}

#[test]
fn a_submission_against_an_unregistered_deployment_id_never_reaches_any_client() {
    let registry: Registry = Registry::new();
    let error = registry
        .try_submit(Submit {
            deployment_id: "nobody-registered-this".into(),
            deployment_generation: 1,
            submission_id: "s1".into(),
            request: chat_request(),
        })
        .expect_err("no client is registered for this deployment_id");
    assert_eq!(
        error,
        p4_adapter::deployment::DispatchError::UnknownDeployment
    );
}
