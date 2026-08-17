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
//!   CHAIN       comma-separated agent addresses, in stage order. `;` separates
//!               replica deployments, which requests are spread across in turn.
//!   REQUESTS    how many inferences to send
//!   TOKENS      how many tokens each should generate
//!   ADAPTER     which registered backend to create nodes on (default `mock`)
//!   ADVERTISED  what the agents should reply to; required across machines
//!
//! `P4_DRIVE_PLAN` is the plan each load carries, for a concrete backend that
//! needs to be told where it is. Defaults to a simulated one.
//! `P4_DRIVE_PLAN_<n>` overrides it for stage `n`, because the shares of a
//! distributed deployment differ from each other, and `P4_DRIVE_PLAN_<d>_<n>`
//! overrides it for stage `n` of replica `d` — replicas are copies in shape
//! and not in placement, since two on one machine sit on different cards.
//!
//! `P4_DRIVE_SERVE` names the stages an inference visits, as indices into the
//! chain — by default all of them. A backend that spreads a model internally
//! has shares that must be loaded and serve nothing, and stating which stages
//! serve keeps the plan opaque here: the alternative is this tool reading a
//! plan to work out what a stage is for, which is the one thing it must not do.
//!
//! `P4_DRIVE_PROMPT_FILE` (or `P4_DRIVE_PROMPT`) is what every request asks,
//! and `P4_DRIVE_OPTIONS` is the generation settings merged into it. A real
//! profile is a long prompt against a long answer, and a prompt sized in
//! thousands of tokens comes from a file so that what was measured is exactly
//! what was sent.
//!
//! `P4_DRIVE_QUIET_MS` is how long nothing may arrive before the driver stops
//! waiting; 30s by default. It is not a budget for the run — an answer takes as
//! long as it takes, and what says something is wrong is silence, not duration.

mod fleet;
mod report;
mod session;
#[cfg(test)]
mod tests;

use fleet::Fleet;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let listen = args.next().ok_or(USAGE)?;
    let fleet = Fleet::parse(&args.next().ok_or(USAGE)?)?;
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
    let plans = fleet.plans(&plan);
    let serving = serving(
        std::env::var("P4_DRIVE_SERVE").ok().as_deref(),
        fleet.stages(),
    )?;
    // What every request asks, and how it should be generated. A real profile
    // is a long prompt against a long answer, and neither fits on a command
    // line — the prompt comes from a file so its size is exactly what was
    // measured rather than whatever survived a shell.
    let prompt = match std::env::var("P4_DRIVE_PROMPT_FILE") {
        Ok(path) => std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {path}: {error}"))?,
        Err(_) => {
            std::env::var("P4_DRIVE_PROMPT").unwrap_or_else(|_| "simulated prompt".to_owned())
        }
    };
    let options = std::env::var("P4_DRIVE_OPTIONS").unwrap_or_else(|_| "{}".to_owned());
    // How long nothing may arrive before the driver stops waiting. Not a
    // budget for the run: a five-thousand-token answer takes as long as it
    // takes, and what says something is wrong is silence, not duration.
    let quiet = Duration::from_millis(
        std::env::var("P4_DRIVE_QUIET_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(30_000),
    );
    // Gap between arrivals. Zero — the default — sends the whole run at once,
    // which measures a backlog draining. Real work arrives while earlier work
    // is still running, and the queues behave differently under the two.
    let arrive = Duration::from_millis(
        std::env::var("P4_DRIVE_ARRIVE_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
    );
    // Whether each request gets a prompt of its own. One prompt sent many
    // times measures a cache as much as a model, and a warm server showed
    // exactly that: sixty-four identical prompts matched its prompt cache at
    // similarity 1.000 and each admission evicted a 143 MiB entry.
    let vary = std::env::var("P4_DRIVE_VARY").is_ok_and(|value| value != "0");
    // What a deployment declares it admits at once. A real one states this
    // from what it measured; a driver that passed its own request count would
    // be declaring a ceiling nobody sized.
    let ceiling: u32 = std::env::var("P4_DRIVE_CEILING")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(32);

    let session = session::Session::start(
        &listen,
        advertise.as_deref(),
        plans,
        prompt,
        options,
        quiet,
        arrive,
        vary,
    )
    .await?;
    println!(
        "P4_DRIVE_READY address={} deployments={} stages={} serving={} prompt_bytes={}",
        session.address(),
        fleet.deployments().len(),
        fleet.stages(),
        serving.len(),
        session.prompt_bytes()
    );
    if session.address().is_local_only()
        && fleet.addresses().iter().any(|stage| !stage.is_local_only())
    {
        println!(
            "P4_DRIVE_UNREACHABLE address={} note=remote-stages-cannot-reply",
            session.address()
        );
    }

    let nodes = fleet.deployments().len() * fleet.stages();
    session.create_nodes(&fleet, &adapter).await?;
    println!("P4_DRIVE_NODES created={nodes}");

    session.load(&fleet, ceiling).await?;
    println!("P4_DRIVE_LOADED nodes={nodes}");

    let started = Instant::now();
    let outcome = session.infer(&fleet, &serving, requests, tokens).await;
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

/// Which stages an inference visits, in order.
///
/// Must end at the last stage: node names carry their position in the whole
/// deployment, so the stage a chain ends on is the one created as its tail, and
/// a served chain ending anywhere else would decode against a node that was
/// never told it was last.
fn serving(
    declared: Option<&str>,
    stages: usize,
) -> Result<Vec<usize>, Box<dyn std::error::Error>> {
    let Some(declared) = declared else {
        return Ok((0..stages).collect());
    };
    let serving: Vec<usize> = declared
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<usize>())
        .collect::<Result<_, _>>()?;
    if serving.is_empty() {
        return Err("P4_DRIVE_SERVE names no stage: nothing would serve".into());
    }
    if let Some(&out) = serving.iter().find(|&&stage| stage >= stages) {
        return Err(format!("P4_DRIVE_SERVE names stage {out} of {stages}").into());
    }
    if serving.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("P4_DRIVE_SERVE must ascend: a chain runs in stage order".into());
    }
    if serving[serving.len() - 1] != stages - 1 {
        return Err(format!(
            "P4_DRIVE_SERVE must end at stage {}, the deployment's tail",
            stages - 1
        )
        .into());
    }
    Ok(serving)
}

const USAGE: &str = "usage: p4-drive LISTEN CHAIN REQUESTS TOKENS [ADAPTER] [ADVERTISED]";
