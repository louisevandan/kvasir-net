//! P4 agent entrypoint.
//!
//! Two runtimes live here on purpose. `event_runtime` is the self-describing
//! event path of [docs/event-protocol-v2.md](../../../docs/event-protocol-v2.md)
//! and is what the four-node acceptance harness drives. The service runtime
//! below it is the earlier Chain/Hop path, kept as the comparison arm an A/B
//! measurement needs before anything is deleted — the same reason
//! `adapters::deployment` is still a sibling of the registry.
//!
//! The event path is the default because it is the one under test; set
//! `P4_AGENT_SERVICE_RUNTIME` to run the service path instead.

mod adapters;
mod event_runtime;

use p4_agent_core::agent::{Agent, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_protocol::Address;
use p4_service::{Bodies, Standard};
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let listen = std::env::args().nth(1).ok_or(USAGE)?;
    let listener = TcpListener::bind(&listen).await?;
    let bound = listener.local_addr()?;
    let own = match std::env::args().nth(2) {
        Some(value) => Address::from_str(&value)?,
        None => Address::tcp(bound.ip().to_string(), bound.port()),
    };
    println!("P4_EVENT_AGENT_READY address={own}");

    if std::env::var_os("P4_AGENT_SERVICE_RUNTIME").is_some() {
        let registry = crate::adapters::registry();
        let (agent, receiver, in_flight) = Agent::new(
            own,
            Arc::new(Standard::new(registry)),
            Arc::new(Bodies::default()),
            Lanes::default(),
            Budget::default(),
        );
        tokio::spawn(inbox::serve(listener, agent.queue(), 256));
        run(agent, receiver, in_flight).await;
        return Ok(());
    }

    event_runtime::run(listener, own).await?;
    Ok(())
}

const USAGE: &str = "usage: p4-agent HOST:PORT [tcp://ADVERTISED_HOST:PORT]";
