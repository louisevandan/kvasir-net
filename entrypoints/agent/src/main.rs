//! The agent process.
//!
//! One socket, one queue, workers, and whatever nodes get created on it. The
//! agent is the only process in this system — there is no controller and no
//! separate node process, because an agent reaching another agent is the same
//! path as an agent answering OUTER.
//!
//! Attaching a backend happens in `adapters()` below and nowhere else.

mod adapters;

use p4_agent_core::agent::{Agent, AgentDeploymentSink, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox::{self, Subscriptions};
use p4_protocol::Address;
use p4_service::{Bodies, CapabilityRegistry, Standard};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen = std::env::args().nth(1).ok_or(USAGE)?;
    let listener = TcpListener::bind(&listen).await?;
    let bound = listener.local_addr()?;

    // What this agent calls itself is what peers put in an envelope, so it has
    // to be the address they can reach rather than the interface it bound.
    let hint = std::env::args().nth(2);
    let own = Address::advertised(hint.as_deref(), &bound.ip().to_string(), bound.port())?;

    let registry = adapters::registry();
    let capabilities = CapabilityRegistry::default();
    let duties = Standard::with_capabilities(registry, capabilities.clone());
    let attached = duties.adapters().join(", ");

    let subscriptions = std::env::var_os("P4_AGENT_STATE_ROOT")
        .map(Subscriptions::with_journal)
        .unwrap_or_default();
    let (agent, receiver, in_flight) = Agent::new_with_subscriptions(
        own.clone(),
        Arc::new(duties),
        Arc::new(Bodies::with_capabilities(capabilities)),
        Lanes::default(),
        Budget::default().checked()?,
        subscriptions.clone(),
    );

    // The submission path, stood up beside the hop path rather than
    // replacing it -- `dispatch` falls back to it for every deployment id
    // this registry has no client for. Wired after `agent` exists, not
    // before: `AgentDeploymentSink` replies by calling back into this very
    // agent, and a `Weak` handle needs something already alive to point at
    // (see that type's own doc for why `Weak` rather than `Arc`).
    let relay_sink: Arc<dyn p4_adapter::deployment::Sink> =
        Arc::new(AgentDeploymentSink::new(Arc::downgrade(&agent)));
    let deployment_id =
        std::env::var("P4_LLAMACPP_DEPLOYMENT_ID").unwrap_or_else(|_| "llamacpp".to_string());
    let deployment_attached =
        adapters::deployment::attach_deployment(relay_sink, agent.deployments())?;
    println!("P4_AGENT_DEPLOYMENT id={deployment_id} attached={deployment_attached} relay=present");

    println!("P4_AGENT_READY address={own} adapters=[{attached}]");
    if own.is_local_only() {
        println!("P4_AGENT_UNREACHABLE address={own} peers=only-this-machine");
    }
    tokio::spawn(inbox::serve_with_subscriptions(
        listener,
        agent.queue(),
        Budget::default().connections,
        subscriptions,
    ));
    if std::env::var("P4_AGENT_STATS").is_ok() {
        watch(Arc::clone(&agent));
    }
    tokio::spawn(run(agent, receiver, in_flight));

    tokio::signal::ctrl_c().await?;
    println!("P4_AGENT_STOPPING address={own}");
    Ok(())
}

const USAGE: &str = "usage: p4-agent HOST:PORT [ADVERTISED_HOST[:PORT]]";

/// Prints what the agent is holding, once a second, when asked.
///
/// Two numbers decide where a slowdown lives: the agent's lanes and the depth
/// of its nodes. Shallow lanes beside deep nodes put the cause below the
/// adapter; the reverse puts it in P4. A fleet run that cannot see both is
/// guessing.
fn watch(agent: Arc<Agent>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let lanes = agent.queue().depth();
            println!(
                "P4_AGENT_DEPTH control={} prefill={} decode={} response={} nodes={}",
                lanes.control,
                lanes.prefill,
                lanes.decode,
                lanes.response,
                agent.node_depth_total().await
            );
            let traffic = agent.traffic();
            // `peers` and `waiting` are the two numbers that only a long run
            // moves. Both should settle; either climbing for hours is a leak
            // rather than load, and neither shows up in a depth reading.
            println!(
                "P4_AGENT_TRAFFIC forwarded={} consumed={} to_nodes={} unrouted={} refused={} emergency_lost={} peers={} waiting={}",
                traffic.forwarded,
                traffic.consumed,
                traffic.to_nodes,
                traffic.unrouted,
                traffic.refused,
                traffic.emergency_lost,
                agent.peers().connected().await,
                agent.continuations().outstanding(),
            );
            for line in agent.node_counts().await {
                println!("P4_AGENT_NODE {line}");
            }
        }
    });
}
