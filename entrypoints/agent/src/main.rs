//! The agent process.
//!
//! One socket, one queue, workers, and whatever nodes get created on it. The
//! agent is the only process in this system — there is no controller and no
//! separate node process, because an agent reaching another agent is the same
//! path as an agent answering OUTER.
//!
//! Attaching a backend happens in `adapters()` below and nowhere else.

mod adapters;

use p4_agent_core::agent::{Agent, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_protocol::Address;
use p4_service::{Bodies, Standard};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen = std::env::args()
        .nth(1)
        .ok_or("usage: p4-agent HOST:PORT [ADVERTISED_HOST]")?;
    let listener = TcpListener::bind(&listen).await?;
    let bound = listener.local_addr()?;

    // What this agent calls itself is what peers put in an envelope, so it has
    // to be the address they can reach rather than the interface it bound.
    let host = std::env::args()
        .nth(2)
        .unwrap_or_else(|| bound.ip().to_string());
    let own = Address::tcp(host, bound.port());

    let registry = adapters::registry();
    let duties = Standard::new(registry);
    let attached = duties.adapters().join(", ");

    let (agent, receiver, in_flight) = Agent::new(
        own.clone(),
        Arc::new(duties),
        Arc::new(Bodies),
        Lanes::default(),
        Budget::default().checked()?,
    );

    println!("P4_AGENT_READY address={own} adapters=[{attached}]");
    tokio::spawn(inbox::serve(
        listener,
        agent.queue(),
        Budget::default().connections,
    ));
    tokio::spawn(run(agent, receiver, in_flight));

    tokio::signal::ctrl_c().await?;
    println!("P4_AGENT_STOPPING address={own}");
    Ok(())
}
