//! Stands a bad link between two addresses.
//!
//! Usage:
//!   p4-link LISTEN TARGET [--delay MS] [--jitter MS] [--rate BYTES_PER_SEC]
//!                         [--stall-every N] [--stall MS]
//!
//! Point an agent at the relay instead of its peer and every frame between
//! them crosses the declared link. Run one per direction that needs impairing;
//! a chain whose middle hop is a satellite is one relay, not four.

use p4_link::Impairment;
use p4_link::relay;
use std::time::Duration;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let listen = args.next().ok_or(USAGE)?;
    let target = args.next().ok_or(USAGE)?;
    let mut link = Impairment::default();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(USAGE)?;
        match flag.as_str() {
            "--delay" => link.delay = Duration::from_millis(value.parse()?),
            "--jitter" => link.jitter = Duration::from_millis(value.parse()?),
            "--rate" => link.rate = Some(value.parse()?),
            "--stall" => link.stall = Duration::from_millis(value.parse()?),
            "--stall-every" => link.stall_every = value.parse()?,
            other => return Err(format!("unknown flag {other}\n{USAGE}").into()),
        }
    }

    let listener = TcpListener::bind(&listen).await?;
    println!(
        "P4_LINK_READY listen={} target={target} delay_ms={} jitter_ms={} rate={} stall_ms={} every={}",
        listener.local_addr()?,
        link.delay.as_millis(),
        link.jitter.as_millis(),
        link.rate
            .map(|rate| rate.to_string())
            .unwrap_or_else(|| "none".into()),
        link.stall.as_millis(),
        link.stall_every,
    );
    relay::serve(listener, target, link).await;
    Ok(())
}

const USAGE: &str = "usage: p4-link LISTEN TARGET [--delay MS] [--jitter MS] [--rate BYTES_PER_SEC] [--stall-every N] [--stall MS]";
