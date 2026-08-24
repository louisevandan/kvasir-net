//! P4 v2 self-describing event agent.

mod event_runtime;

use p4_protocol::Address;
use std::str::FromStr;
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
    event_runtime::run(listener, own).await?;
    Ok(())
}

const USAGE: &str = "usage: p4-agent HOST:PORT [tcp://ADVERTISED_HOST:PORT]";
