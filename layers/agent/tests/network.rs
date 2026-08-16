//! The network being slow, which is a condition and not a fault.
//!
//! Every other test here runs on loopback, where a hop costs nothing. That
//! hides the case a distributed inference actually lives in: machines a long
//! way apart, on links that are narrow, jittery, or that seize up under
//! somebody else's traffic. A layer that only holds together on a fast LAN has
//! not been tested against its own deployment.
//!
//! Each agent below sits behind a relay carrying its declared badness, and
//! nothing in P4 is told the relay is there — it addresses what it was given
//! and meets whatever that costs.
//!
//! The claims are the same four the fleet checks, because a slow link must
//! change how long the work takes and nothing else about it.

mod common;

use common::{Outer, Silent, chain_over, request, runtime, settle, start, start_behind, until};
use p4_link::Impairment;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A link with real distance on it: tens of milliseconds, and not a steady
/// tens.
fn wan() -> Impairment {
    Impairment::latency(Duration::from_millis(15), Duration::from_millis(10))
}

/// The guard on every other test in this file.
///
/// A relay that had quietly fallen out of the path would leave the scenarios
/// below passing while testing nothing, and they would pass *faster*, which is
/// the direction nobody investigates. So one test asserts the link is really
/// being crossed, by the only evidence that cannot be faked: it took the time.
#[test]
fn the_declared_link_is_actually_in_the_path() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let far = start_behind(
            Arc::new(Silent),
            Impairment::latency(Duration::from_millis(60), Duration::ZERO),
        )
        .await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 4)
            .await;
        far.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 4)
            .await;

        // One request, one token: exactly one crossing of the slow link.
        let chain = chain_over(&[(&a, "n0"), (&far, "n1")]);
        let started = Instant::now();
        a.enqueue(request("only", &chain, &outer, 1)).unwrap();
        until(|| !outer_duties.frames_for("only").is_empty()).await;
        let elapsed = started.elapsed();

        assert!(
            elapsed >= Duration::from_millis(55),
            "the frame crossed the 60ms link rather than going straight: {elapsed:?}"
        );
    });
}

#[test]
fn a_chain_across_slow_links_still_answers_every_request() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start_behind(Arc::new(Silent), wan()).await;
        let b = start_behind(Arc::new(Silent), wan()).await;
        let c = start_behind(Arc::new(Silent), wan()).await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::staged(1, Profile::default())), 8)
            .await;
        c.create_node("n2", Arc::new(Mock::terminal(2, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1"), (&c, "n2")]);
        for index in 0..24 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 4))
                .unwrap();
        }
        until(|| outer_duties.total_frames() >= 24 * 4).await;

        assert_eq!(outer_duties.routes(), 24, "every request answered");
        for index in 0..24 {
            let frames = outer_duties.frames_for(&format!("r{index}"));
            assert_eq!(frames.len(), 4, "route {index} got all of its tokens");
            assert_eq!(
                frames.last().unwrap().body,
                b"stop",
                "route {index} ended once"
            );
        }
    });
}

/// Jitter must not become reordering.
///
/// Each hop is held for a different length of time, so frames that left in
/// order arrive spread out. Per-route order is P4's promise, and this is the
/// condition most likely to break it.
#[test]
fn a_jittery_link_does_not_reorder_a_token_stream() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start_behind(
            Arc::new(Silent),
            Impairment::latency(Duration::from_millis(2), Duration::from_millis(25)),
        )
        .await;
        let b = start_behind(
            Arc::new(Silent),
            Impairment::latency(Duration::from_millis(2), Duration::from_millis(25)),
        )
        .await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..16 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 6))
                .unwrap();
        }
        until(|| outer_duties.total_frames() >= 16 * 6).await;

        for index in 0..16 {
            let route = format!("r{index}");
            let bodies: Vec<String> = outer_duties
                .frames_for(&route)
                .iter()
                .map(|frame| String::from_utf8_lossy(&frame.body).into_owned())
                .collect();
            let numbered: Vec<&String> = bodies.iter().filter(|body| *body != "stop").collect();
            let mut expected = numbered.clone();
            expected.sort();
            assert_eq!(numbered, expected, "{route} arrived in order: {bodies:?}");
            assert_eq!(
                bodies.last().map(String::as_str),
                Some("stop"),
                "{route} ended on its terminal"
            );
        }
    });
}

/// A link that seizes for a tenth of a second at a time.
///
/// This is the shape that breaks a design holding a worker per in-flight
/// request: the stall is far longer than the work, so anything waiting on the
/// socket is waiting instead of serving. Nothing here may be lost by it.
#[test]
fn a_stalling_link_delays_work_without_losing_it() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start_behind(
            Arc::new(Silent),
            Impairment::stalling(6, Duration::from_millis(120)),
        )
        .await;
        let b = start_behind(
            Arc::new(Silent),
            Impairment::stalling(5, Duration::from_millis(120)),
        )
        .await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..20 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 3))
                .unwrap();
        }
        until(|| outer_duties.total_frames() >= 20 * 3).await;

        assert_eq!(outer_duties.routes(), 20, "every route survived the stalls");
        assert_eq!(outer_duties.total_frames(), 20 * 3, "with all its tokens");
    });
}

/// A narrow link, where the cost is per byte rather than per hop.
///
/// The interesting part is that the agent must not answer the narrowness by
/// piling frames into a peer queue without bound: the wait belongs on the
/// link, and the backpressure has to reach back to the node producing them.
#[test]
fn a_narrow_link_slows_the_work_rather_than_burying_it() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start_behind(Arc::new(Silent), Impairment::bandwidth(96 * 1024)).await;
        let b = start_behind(Arc::new(Silent), Impairment::bandwidth(96 * 1024)).await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..32 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 4))
                .unwrap();
        }

        settle(120).await;
        let lanes = a.queue().depth();
        let in_front = lanes.control + lanes.prefill + lanes.decode + lanes.response;
        assert!(
            in_front < 4096,
            "the lanes did not become the buffer the link refused to be: {in_front}"
        );

        until(|| outer_duties.total_frames() >= 32 * 4).await;
        assert_eq!(outer_duties.routes(), 32, "and it all arrived");
    });
}

/// One stage far worse than the others, which is what a fleet actually looks
/// like once a machine is somewhere else.
///
/// The claim is that the chain runs at the speed of its worst link and not
/// slower — a design that serialised on the slow hop would multiply it by the
/// number of requests.
#[test]
fn one_bad_link_in_a_chain_does_not_serialise_the_rest() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start_behind(Arc::new(Silent), Impairment::default()).await;
        let slow = start_behind(
            Arc::new(Silent),
            Impairment::latency(Duration::from_millis(30), Duration::ZERO),
        )
        .await;
        let c = start_behind(Arc::new(Silent), Impairment::default()).await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 16)
            .await;
        slow.create_node("n1", Arc::new(Mock::staged(1, Profile::default())), 16)
            .await;
        c.create_node("n2", Arc::new(Mock::terminal(2, Profile::default())), 16)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&slow, "n1"), (&c, "n2")]);
        let started = Instant::now();
        for index in 0..24 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 2))
                .unwrap();
        }
        until(|| outer_duties.total_frames() >= 24 * 2).await;
        let elapsed = started.elapsed();

        assert_eq!(outer_duties.routes(), 24, "every request finished");
        // Serialised, twenty-four requests each paying four 30ms crossings
        // would be near three seconds. Overlapped, they share the wait.
        assert!(
            elapsed < Duration::from_millis(2_000),
            "requests overlapped on the slow hop rather than queueing behind \
             one another: {elapsed:?}"
        );
    });
}

/// A slow link and a slow backend at once, which is the real deployment.
///
/// Both waits exist; neither may be paid twice, and the attribution has to
/// survive having two causes.
#[test]
fn a_slow_link_and_a_slow_backend_together_still_attribute_correctly() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start_behind(Arc::new(Silent), wan()).await;
        let b = start_behind(Arc::new(Silent), wan()).await;

        let slow_backend = Profile {
            leading_hop: Duration::from_millis(25),
            trailing_hop: Duration::from_millis(25),
            ..Profile::default()
        };
        a.create_node("n0", Arc::new(Mock::staged(0, slow_backend)), 4)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..32 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 2))
                .unwrap();
        }

        settle(200).await;
        let node_depth = a.node_depth("n0").await.unwrap_or(0);
        let lanes = a.queue().depth();
        let in_front = lanes.control + lanes.prefill + lanes.decode + lanes.response;
        assert!(
            node_depth > 0,
            "the backend's share of the wait is on the node"
        );
        assert!(
            in_front <= node_depth,
            "and the link's share did not move it in front: lanes {in_front}, \
             node {node_depth}"
        );

        until(|| outer_duties.total_frames() >= 32 * 2).await;
        assert_eq!(outer_duties.routes(), 32, "and everything completed");
    });
}
