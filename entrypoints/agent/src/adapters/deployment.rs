//! Wiring for `p4-llamacpp-deployment`'s submission-stream client into the
//! `p4_adapter::deployment::Registry` the running `Agent` itself holds --
//! the P4-side broker surface `SEALED-CONTRACT.md` §9.1/§9.4 describes.
//! Unlike [`super::registry`], which hands out `Arc<dyn p4_adapter::Adapter>`
//! for the legacy `Work`/`Hop` path, nothing here goes through `Adapter` at
//! all: `p4_agent_core::agent::Agent::dispatch` calls `try_submit`/`cancel`
//! on `Agent::deployments()` directly (proved end-to-end, over a real
//! socket, by `p4-llamacpp-deployment`'s own `tests/broker_registry.rs`, and
//! now also by this crate reaching it from a live inbound frame).
//!
//! `attach_deployment` registers into a `Registry` the caller already owns,
//! rather than building and returning its own the way an earlier version of
//! this file did: the registry an inbound frame is actually dispatched
//! against lives on `Agent` (see `Agent::deployments`), and a second,
//! disconnected `Registry` built here and never consulted by `dispatch`
//! would be exactly the kind of duplicated ownership `SEALED-CONTRACT.md`
//! §9.3 names as the mistake to not repeat.
//!
//! Depends on `P4_LLAMACPP_DEPLOYMENT_*` environment variables, the same
//! env-gated-capability shape [`super::register_staged`] already uses for
//! the staged adapter: a deployment this build cannot reach is a
//! deployment this function does not register, not a runtime fallback.

use p4_adapter::deployment::{DeploymentId, Registry, Sink};
use p4_llamacpp_deployment::DeploymentClient;
use p4_llamacpp_deployment::transport::TransportFactory;
use p4_llamacpp_deployment::transport::tcp::TcpTransportFactory;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_RECONNECT_BACKOFF: Duration = Duration::from_millis(200);

/// Connects `p4-llamacpp-deployment`'s client and registers it into
/// `registry` under `P4_LLAMACPP_DEPLOYMENT_ID`, returning whether it did.
/// A no-op returning `Ok(false)` when `P4_LLAMACPP_DEPLOYMENT_ADDR` is
/// unset, unless `P4_LLAMACPP_DEPLOYMENT_REQUIRED=1`. Required mode is used
/// by relay deployments so a missing address fails startup instead of
/// silently sending the same inference down the legacy hop/window path.
/// `sink` is the caller's single long-lived
/// `Arc<dyn Sink>`: every event for every deployment this client ever
/// raises is delivered there (`p4_adapter::deployment::Client`'s own doc
/// explains why this cannot be per-submission).
pub fn attach_deployment(sink: Arc<dyn Sink>, registry: &Registry) -> std::io::Result<bool> {
    let Some(addr) = std::env::var_os("P4_LLAMACPP_DEPLOYMENT_ADDR") else {
        if std::env::var_os("P4_LLAMACPP_DEPLOYMENT_REQUIRED").is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "P4_LLAMACPP_DEPLOYMENT_REQUIRED is set but P4_LLAMACPP_DEPLOYMENT_ADDR is missing",
            ));
        }
        return Ok(false);
    };
    let addr: SocketAddr = addr
        .to_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "P4_LLAMACPP_DEPLOYMENT_ADDR is not a valid socket address",
            )
        })?;
    let deployment_id: DeploymentId =
        std::env::var("P4_LLAMACPP_DEPLOYMENT_ID").unwrap_or_else(|_| "llamacpp".to_string());

    // The generation is the backend's to issue. It used to default to 1
    // here, which is a number this process made up: when it disagreed with
    // the deployment actually loaded, every submission reached the wire and
    // came back `deployment_closed`, which reads from the outside like a
    // backend that answers nothing. The handshake now carries it, and a
    // server that reports none is a server this client cannot correctly
    // submit to -- so that is a failure to attach, not a default.
    let factory = Arc::new(TcpTransportFactory::for_deployment(
        addr,
        deployment_id.clone(),
    ));
    let probe = Arc::clone(&factory);
    let factory: Arc<dyn TransportFactory> = factory;
    let client = DeploymentClient::connect(
        factory,
        sink,
        deployment_id.clone(),
        0,
        DEFAULT_RECONNECT_BACKOFF,
    )?;
    let Some(generation) = probe.reported_generation() else {
        client.close();
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "the deployment server reported no generation for {deployment_id}; \
                 it does not know this deployment, or it predates the handshake \
                 that carries one"
            ),
        ));
    };
    client.advance_generation(generation);
    if std::env::var("P4_AGENT_STATS").is_ok_and(|value| value != "0") {
        let observed = Arc::downgrade(&client);
        let observed_id = deployment_id.clone();
        std::thread::Builder::new()
            .name("p4-deployment-stats".into())
            .spawn(move || {
                while let Some(client) = observed.upgrade() {
                    let stats = client.stats();
                    println!(
                        "P4_AGENT_DEPLOYMENT_STATS id={} queued={} peak_queued={} outstanding={} peak_outstanding={} inbound={} reconnects={} full_retries={}",
                        observed_id,
                        stats.queued_submissions,
                        stats.peak_queued_submissions,
                        stats.outstanding_submissions,
                        stats.peak_outstanding_submissions,
                        stats.inbound_events,
                        stats.reconnects,
                        stats.full_retries
                    );
                    std::thread::sleep(Duration::from_secs(1));
                }
            })?;
    }
    registry.register(deployment_id, client);
    Ok(true)
}
