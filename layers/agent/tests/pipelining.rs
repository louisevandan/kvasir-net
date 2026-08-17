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
        // Both, because a stage reads whichever matches its position and the
        // caller here is naming one stage at a time.
        leading_hop: Duration::from_millis(hop),
        trailing_hop: Duration::from_millis(hop),
        prefill_extra: Duration::from_millis(hop),
        ..Profile::default()
    }
}

/// Runs a chain whose stages all cost the same.
async fn overlap(stages: usize, arrivals: usize, ceiling: usize, tokens: u32) -> f64 {
    let costs: Vec<u64> = (0..stages).map(|_| 30).collect();
    uneven(&costs, arrivals, ceiling, tokens).await.overlap
}

/// Runs a chain and returns stage-compute over wall clock, with each stage
/// costing what it is told.
///
/// Stages are not alike in a real deployment. The first carries the prefill,
/// and this machine measured it at 5.05 seconds per layer against 3.0 for the
/// second — which is why the layer split that won was 16/24 rather than 20/20:
/// the expensive stage gets fewer layers so the two take the same time.
async fn uneven(costs: &[u64], arrivals: usize, ceiling: usize, tokens: u32) -> Run {
    let stages = costs.len();
    let outer_duties = Outer::default();
    let outer = start(Arc::new(outer_duties.clone())).await;

    let mut agents = Vec::new();
    let mut mocks = Vec::new();
    for position in 0..stages {
        let agent = start(Arc::new(Silent)).await;
        let profile = costly(costs[position]);
        let mock = Arc::new(match position + 1 == stages {
            true => Mock::terminal(position, profile),
            false => Mock::staged(position, profile),
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
    // Per stage, what a card would show: busy over busy-plus-the-gaps between
    // its own hops. The ramp before the first hop and the drain after the last
    // are not in it, because the question is whether a stage rests while work
    // is queued behind it.
    let mut starved = 1.0f64;
    for (position, mock) in mocks.iter().enumerate() {
        let busy = mock.busy().as_secs_f64();
        let idle = mock.idle().as_secs_f64();
        let occupied = busy / (busy + idle).max(f64::MIN_POSITIVE);
        starved = starved.min(occupied);
        println!(
            "    stage {position}: busy {busy:.2}s idle {idle:.2}s -> {:.0}% occupied",
            occupied * 100.0
        );
    }
    Run {
        overlap: computed.as_secs_f64() / wall.as_secs_f64().max(f64::MIN_POSITIVE),
        starved,
    }
}

/// What a run of a chain says about itself.
struct Run {
    /// Stage compute over wall clock. Bounded above by the number of stages,
    /// and dragged down by the fill and the drain however busy the stages are.
    overlap: f64,
    /// The least occupied stage: busy over busy-plus-its-own-gaps. This is the
    /// one that says whether a stage rests while work is queued behind it, and
    /// it is not affected by the ramp.
    starved: f64,
}

/// Two stages must compute at the same time, not take turns.
#[test]
fn two_stages_overlap() {
    runtime().block_on(async {
        let ratio = overlap(2, 128, 8, 3).await;
        println!("two stages: stage compute / wall = {ratio:.2}");
        assert!(
            ratio > 1.8,
            "two stages managed {ratio:.2} of the wall clock between them, \
             which is one stage's worth: they took turns"
        );
    });
}

/// And three, under arrival that keeps coming.
///
/// The arrivals are many because the measure counts the whole run, including
/// the start where the later stages have nothing yet and the end where the
/// earlier ones are done. That is real time, and on a short run it is most of
/// it — which is what makes a chain that never rests read as 76% efficient.
///
/// The ceiling is deliberately below the arrivals. A node given its whole
/// queue in one window hands the next stage everything at once and then has
/// nothing left to start, which is the shape that measures 1.0 however long
/// the queue is.
#[test]
fn three_stages_overlap() {
    runtime().block_on(async {
        let ratio = overlap(3, 192, 8, 3).await;
        println!("three stages: stage compute / wall = {ratio:.2}");
        assert!(
            ratio > 2.7,
            "three stages managed {ratio:.2} of the wall clock between them,              so one was idle while the others worked"
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

/// The shortfall is the fill and the empty, not a stage resting.
///
/// Stage compute over wall clock counts the whole run, including the start
/// where the later stages have nothing yet and the end where the earlier ones
/// are done. That is real time and a short run is mostly made of it — which is
/// why the ratio alone reads as an efficiency problem when the stages are in
/// fact almost never idle.
///
/// So the same chain is run twice, differing only in how much work arrives. If the shortfall were stages resting it would not move; it is the
/// ramp, so a longer run buries it.
#[test]
fn a_longer_run_approaches_the_number_of_stages() {
    runtime().block_on(async {
        let brief = overlap(3, 24, 8, 3).await;
        let sustained = overlap(3, 192, 8, 3).await;
        println!("three stages: brief={brief:.2} sustained={sustained:.2} of 3.00");
        assert!(
            sustained > brief,
            "a longer run should bury the ramp: brief {brief:.2}, \
             sustained {sustained:.2}"
        );
        assert!(
            sustained > 2.8,
            "with the ramp amortised the stages should be nearly always \
             computing, and this measured {sustained:.2} of 3.00"
        );
    });
}

/// A chain runs at the speed of its slowest stage, so the layers have to be
/// dealt to make them equal.
///
/// The first stage is not like the others: it carries the prefill. This
/// machine measured it at 5.05 seconds per layer against 3.0 for the second,
/// and the split that won was 16/24 rather than 20/20 — fewer layers on the
/// expensive stage so the two take the same time. Equal layers on unequal
/// stages is the mistake this pins.
///
/// Both runs do the same total work. Only the deal changes.
#[test]
fn dealing_layers_by_cost_beats_dealing_them_evenly() {
    runtime().block_on(async {
        // Equal layers: the first stage is 1.7x per layer, so it takes 1.7x
        // the time and the stage behind it waits.
        let evenly = uneven(&[51, 30, 30], 192, 8, 3).await;
        // The same three stages' work, dealt so each takes the same time: the
        // expensive one gets fewer layers, the cheap ones absorb them.
        let by_cost = uneven(&[37, 37, 37], 192, 8, 3).await;

        println!(
            "evenly: overlap {:.2} worst stage {:.0}% | by cost: overlap {:.2} worst stage {:.0}%",
            evenly.overlap,
            evenly.starved * 100.0,
            by_cost.overlap,
            by_cost.starved * 100.0
        );
        // The claim is about the stage that waits, not about the aggregate.
        // An overloaded first stage starves the one behind it, and that shows
        // as its idle rather than in a ratio the ramp also moves.
        assert!(
            evenly.starved < 0.92,
            "equal layers on unequal stages should leave one waiting, and the              least occupied managed {:.0}%",
            evenly.starved * 100.0
        );
        assert!(
            by_cost.starved > 0.95,
            "dealt by cost, no stage should rest: the least occupied managed              {:.0}%",
            by_cost.starved * 100.0
        );
        assert!(
            by_cost.overlap > evenly.overlap,
            "dealing by cost ({:.2}) should beat dealing evenly ({:.2})",
            by_cost.overlap,
            evenly.overlap
        );
    });
}
