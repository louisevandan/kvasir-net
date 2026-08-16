//! The two-tier queue, watched while a backend deliberately takes its time.
//!
//! This is the claim the whole design rests on: the agent's queue drains at
//! agent speed and the node's queue holds the long work, so under load the
//! two depths say which side of the adapter boundary is slow. A test that
//! only checks the work finishes cannot see it — the interesting state is
//! mid-flight, while the mock is still holding hops.
//!
//! Every profile here is slow on purpose. Nothing is timed against wall clock
//! as a threshold; what is asserted is where the work is sitting.

mod common;

use common::{Outer, Silent, chain_over, request, runtime, settle, start, until};
use p4_mock::Mock;
use p4_mock::profile::Profile;
use std::sync::Arc;
use std::time::Duration;

/// A backend that holds each hop long enough for arrivals to stack up behind
/// it, with the ceiling low so the node cannot simply widen its way out.
fn slow(hop: u64) -> Profile {
    Profile {
        leading_hop: Duration::from_millis(hop),
        trailing_hop: Duration::from_millis(hop),
        prefill_extra: Duration::from_millis(hop / 2),
        ..Profile::default()
    }
}

/// Concurrent arrivals against one slow node: the work waits on the node and
/// the agent's lanes stay shallow.
///
/// Both halves matter. Deep node depth alone would be satisfied by an agent
/// that also backed up; it is the pairing that makes a slowdown attributable.
#[test]
fn concurrent_arrivals_wait_on_the_node_while_the_agent_stays_shallow() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, slow(40))), 2)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..48 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 2))
                .unwrap();
        }

        // Sampled while the backend is still working, not after.
        settle(150).await;
        let node_depth = a.node_depth("n0").await.unwrap_or(0);
        let lanes = a.queue().depth();
        let in_front = lanes.control + lanes.prefill + lanes.decode + lanes.response;

        assert!(
            node_depth > 0,
            "the wait is behind the adapter, where the slowness is"
        );
        assert!(
            in_front <= node_depth,
            "the agent's lanes did not back up with it: lanes {in_front}, node {node_depth}"
        );

        // And it still all finishes.
        until(|| outer_duties.routes() >= 48).await;
        assert_eq!(outer_duties.routes(), 48, "every arrival was answered");
    });
}

/// A chain of three slow nodes under concurrent arrivals.
///
/// Chained is the case the single-node test cannot cover: each hop hands work
/// to the next machine, so a stage that is slower than its neighbours must
/// hold its own queue rather than push the wait back up the chain into the
/// agent queues of the stages before it.
#[test]
fn a_chain_of_slow_nodes_keeps_each_wait_on_its_own_node() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;
        let c = start(Arc::new(Silent)).await;

        // The middle stage is the slow one, which is what a badly balanced
        // layer split looks like.
        a.create_node("n0", Arc::new(Mock::staged(0, slow(10))), 4)
            .await;
        b.create_node("n1", Arc::new(Mock::staged(1, slow(50))), 4)
            .await;
        c.create_node("n2", Arc::new(Mock::terminal(2, slow(10))), 4)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1"), (&c, "n2")]);
        for index in 0..32 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 3))
                .unwrap();
        }

        settle(200).await;
        let slow_stage = b.node_depth("n1").await.unwrap_or(0);
        let b_lanes = b.queue().depth();
        let a_lanes = a.queue().depth();
        let b_in_front = b_lanes.control + b_lanes.prefill + b_lanes.decode + b_lanes.response;
        let a_in_front = a_lanes.control + a_lanes.prefill + a_lanes.decode + a_lanes.response;

        assert!(
            slow_stage > 0,
            "the slow stage is holding the work it cannot yet run"
        );
        assert!(
            b_in_front <= slow_stage,
            "and holding it on its node, not in its lanes: lanes {b_in_front}, node {slow_stage}"
        );
        assert!(
            a_in_front <= slow_stage,
            "the stage before it did not absorb the wait: lanes {a_in_front}"
        );

        until(|| outer_duties.routes() >= 32).await;
        assert_eq!(outer_duties.routes(), 32, "every chained request finished");
    });
}

/// Decode laps keep being dispatched while prefill is still arriving.
///
/// Prefill is the expensive phase and there is far more of it queued here, so
/// a strict lane priority would let it hold the decode lane shut and no
/// request would ever reach its last token. Bounded preference is what stops
/// that, and a slow backend is what makes the two lanes overlap long enough
/// for it to matter.
#[test]
fn decode_keeps_moving_while_prefill_is_still_queued() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, slow(8))), 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        // Long generations, so each request spends most of its life lapping,
        // and enough of them that prefill keeps arriving throughout.
        for index in 0..40 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 8))
                .unwrap();
        }

        // Partway through, some routes should already be past their first
        // token — that is decode being served while prefill is still queued.
        settle(250).await;
        let lapping = (0..40)
            .filter(|index| outer_duties.frames_for(&format!("r{index}")).len() > 1)
            .count();
        assert!(
            lapping > 0,
            "decode was dispatched before every prefill had been taken"
        );

        // `routes` counts routes heard from, which a long generation reaches
        // early; the finish line is every token of every route.
        until(|| outer_duties.total_frames() >= 40 * 8).await;
        assert_eq!(outer_duties.routes(), 40, "and every route still finished");
        assert_eq!(
            outer_duties.total_frames(),
            40 * 8,
            "each request got all of its tokens"
        );
    });
}

/// The claim the design rests on, watched rather than assumed: however far the
/// node's own queue runs ahead, it never hands the adapter more than the
/// ceiling the load declared.
///
/// This is what "P4 keeps its own queue" means in practice. A backend has its
/// own optimal width and is configured for it; everything past that must wait
/// on this side, because the alternative is handing a backend a hundred
/// requests and relying on it to hold ninety — which makes the backend's
/// queue the real one, and makes cancellation, ordering and attribution its
/// business rather than ours.
///
/// Arrivals are spread over time on purpose. A burst measures a backlog
/// draining; work arriving while earlier work is still running is the case
/// that never ends, and it is the one the ceiling has to survive.
#[test]
fn a_node_never_hands_the_adapter_more_than_its_ceiling() {
    runtime().block_on(async {
        const CEILING: usize = 6;
        const ARRIVALS: usize = 90;

        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, slow(20))), CEILING)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        let mut widest = 0;
        let mut deepest = 0;
        let mut lanes_worst = 0;
        for index in 0..ARRIVALS {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 3))
                .unwrap();
            settle(4).await;
            let status = a.node_status().await;
            let node = status.first().expect("the node exists");
            widest = widest.max(node.running);
            deepest = deepest.max(node.depth);
            let lanes = a.queue().depth();
            lanes_worst =
                lanes_worst.max(lanes.control + lanes.prefill + lanes.decode + lanes.response);
            assert!(
                node.running <= CEILING,
                "hop {index} put {} sequences in the adapter against a ceiling of {CEILING}",
                node.running
            );
        }

        // Each half is worthless alone. A ceiling never exceeded is trivially
        // true if nothing ever queued, and a deep queue proves nothing about
        // where the excess was held.
        assert!(
            deepest > CEILING,
            "nothing was ever held back: deepest {deepest} against a ceiling of {CEILING}"
        );
        assert!(
            widest > 1,
            "windows of one prove nothing: the ceiling was never approached, only \n             never exceeded"
        );
        assert!(
            lanes_worst <= deepest,
            "the backlog lived in the agent's lanes, not the node's queue: \
             lanes {lanes_worst}, node {deepest}"
        );

        until(|| outer_duties.routes() >= ARRIVALS).await;
        assert_eq!(outer_duties.routes(), ARRIVALS, "every arrival was answered");
    });
}
