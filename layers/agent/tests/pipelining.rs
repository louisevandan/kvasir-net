//! How much of the wall clock a chain's stages spend computing.
//!
//! One inference is serial by construction — its stages run in order — so a
//! chain only pays for itself when the stages are working on *different*
//! requests at the same time. That is the whole of pipelining, and it is the
//! difference between a chain and a queue with extra hops.
//!
//! The measure is the one this machine used on the runtime that came before:
//! stage compute over wall clock. One stage can never exceed the wall. A chain
//! of three that overlaps properly approaches three times it. A chain that
//! takes turns stays at one however much work is queued behind it — the old
//! runtime measured exactly that, 97.5% with a cohort taken as one window,
//! against 166% with the same cohort split in two, and the difference was
//! worth 27% of throughput.
//!
//! Nothing here is timed against a threshold in seconds. What is asserted is a
//! ratio, which is scale-free: a slower machine moves both halves.

mod common;

use common::{Outer, Silent, chain_over, request, runtime, start, until};
use p4_mock::Mock;
use p4_mock::profile::Profile;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Long enough that the ratio is about pipelining rather than about how the
/// operating system happened to schedule three threads. Short hops make the
/// measurement a contest with whatever else the machine is running, and this
/// suite runs its files in parallel.
fn costly(hop: u64) -> Profile {
    Profile {
        leading_hop: Duration::from_millis(hop),
        trailing_hop: Duration::from_millis(hop),
        prefill_extra: Duration::from_millis(hop),
        ..Profile::default()
    }
}

/// Runs a chain of `stages` and returns stage-compute over wall clock.
async fn overlap(stages: usize, arrivals: usize, ceiling: usize, tokens: u32) -> f64 {
    let outer_duties = Outer::default();
    let outer = start(Arc::new(outer_duties.clone())).await;

    let mut agents = Vec::new();
    let mut mocks = Vec::new();
    for position in 0..stages {
        let agent = start(Arc::new(Silent)).await;
        let mock = Arc::new(match position + 1 == stages {
            true => Mock::terminal(position, costly(30)),
            false => Mock::staged(position, costly(30)),
        });
        agent
            .create_node(format!("s{position}"), Arc::clone(&mock) as Arc<_>, ceiling)
            .await;
        agents.push(agent);
        mocks.push(mock);
    }

    let named: Vec<(&Arc<p4_agent_core::agent::Agent>, &str)> = agents
        .iter()
        .enumerate()
        .map(|(position, agent)| (agent, ["s0", "s1", "s2", "s3"][position]))
        .collect();
    let chain = chain_over(&named);

    let began = Instant::now();
    for index in 0..arrivals {
        agents[0]
            .enqueue(request(&format!("r{index}"), &chain, &outer, tokens))
            .unwrap();
    }
    until(|| outer_duties.routes() >= arrivals).await;
    let wall = began.elapsed();

    let computed: Duration = mocks.iter().map(|mock| mock.busy()).sum();
    computed.as_secs_f64() / wall.as_secs_f64().max(f64::MIN_POSITIVE)
}

/// Two stages must compute at the same time, not take turns.
#[test]
fn two_stages_overlap() {
    runtime().block_on(async {
        let ratio = overlap(2, 32, 8, 3).await;
        println!("two stages: stage compute / wall = {ratio:.2}");
        assert!(
            ratio > 1.2,
            "two stages managed {ratio:.2} of the wall clock between them, \
             which is one stage's worth: they took turns"
        );
    });
}

/// And three, where there is more to lose.
///
/// The ceiling is deliberately below the arrivals. A node given its whole
/// queue in one window hands the next stage everything at once and then has
/// nothing left to start, which is the shape that measures 1.0 however long
/// the queue is.
#[test]
fn three_stages_overlap() {
    runtime().block_on(async {
        let ratio = overlap(3, 48, 8, 3).await;
        println!("three stages: stage compute / wall = {ratio:.2}");
        assert!(
            ratio > 1.4,
            "three stages managed {ratio:.2} of the wall clock between them, \
             so at least one was idle while the others worked"
        );
    });
}

/// The failure this exists to catch: a ceiling wide enough to swallow every
/// arrival leaves the stage behind nothing to begin.
///
/// Not a defect to fix in the layer — it is what a caller asks for when it
/// declares a ceiling that large — but it must be visible, because the same
/// deployment looks identical from outside except for being slower.
#[test]
fn a_ceiling_wider_than_the_work_costs_the_overlap() {
    runtime().block_on(async {
        let split = overlap(3, 24, 6, 3).await;
        let whole = overlap(3, 24, 64, 3).await;
        println!("three stages: split={split:.2} whole={whole:.2}");
        assert!(
            split > whole,
            "a window bounded below the arrivals ({split:.2}) should overlap \
             more than one that swallows them ({whole:.2})"
        );
    });
}
