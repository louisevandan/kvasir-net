//! Drives a fleet of agents and reports what came back.
//!
//! This is OUTER, and it is an agent with different duties — the same core,
//! the same envelope, the same socket. That it can be built this way is the
//! point: nothing in the communication layer distinguishes the thing that asks
//! from the things that answer.
//!
//! Usage:
//!   p4-drive LISTEN CHAIN REQUESTS TOKENS [ADAPTER] [ADVERTISED]
//!
//!   LISTEN      where replies come back, e.g. 0.0.0.0:52000
//!   CHAIN       comma-separated agent addresses, in stage order
//!   REQUESTS    how many inferences to send
//!   TOKENS      how many tokens each should generate
//!   ADAPTER     which registered backend to create nodes on (default `mock`)
//!   ADVERTISED  what the agents should reply to; required across machines
//!
//! `P4_DRIVE_PLAN` is the plan each load carries, for a concrete backend that
//! needs to be told where it is. Defaults to a simulated one.

mod report;
mod session;

use p4_protocol::Address;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let listen = args.next().ok_or(USAGE)?;
    let chain: Vec<Address> = args
        .next()
        .ok_or(USAGE)?
        .split(',')
        .map(|part| format!("tcp://{}", part.trim()).parse())
        .collect::<Result<_, _>>()?;
    let requests: usize = args.next().ok_or(USAGE)?.parse()?;
    let tokens: u32 = args.next().ok_or(USAGE)?.parse()?;
    let adapter = args.next().unwrap_or_else(|| "mock".to_owned());
    // Agents reply to what the driver called itself, so across machines this
    // has to be an address they can reach.
    let advertise = args.next();
    // What a load carries to the backend. A mock ignores it; a concrete
    // adapter reads it and is the only thing that knows what it means.
    let plan =
        std::env::var("P4_DRIVE_PLAN").unwrap_or_else(|_| r#"{"simulated":true}"#.to_owned());
    // What a deployment declares it admits at once. A real one states this
    // from what it measured; a driver that passed its own request count would
    // be declaring a ceiling nobody sized.
    let ceiling: u32 = std::env::var("P4_DRIVE_CEILING")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(32);

    let session = session::Session::start(&listen, advertise.as_deref(), plan).await?;
    println!(
        "P4_DRIVE_READY address={} stages={}",
        session.address(),
        chain.len()
    );
    if session.address().is_local_only() && chain.iter().any(|stage| !stage.is_local_only()) {
        println!(
            "P4_DRIVE_UNREACHABLE address={} note=remote-stages-cannot-reply",
            session.address()
        );
    }

    session.create_nodes(&chain, &adapter).await?;
    println!("P4_DRIVE_NODES created={}", chain.len());

    session.load(&chain, ceiling).await?;
    println!("P4_DRIVE_LOADED stages={}", chain.len());

    let started = Instant::now();
    let outcome = session.infer(&chain, requests, tokens).await;
    let elapsed = started.elapsed();

    report::print(&outcome, elapsed, requests, tokens);
    if outcome.completed == requests {
        Ok(())
    } else {
        Err(format!(
            "{} of {requests} requests did not finish",
            requests - outcome.completed
        )
        .into())
    }
}

const USAGE: &str = "usage: p4-drive LISTEN CHAIN REQUESTS TOKENS [ADAPTER] [ADVERTISED]";
