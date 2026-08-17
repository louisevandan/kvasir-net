//! The network as an abstraction, rather than the one the fleet happens to be
//! wired as.
//!
//! Everything the layer knows about the network is in the envelope: an address
//! that is absolute, a chain that travels whole, and a reply address. Nothing
//! else — no routing table, no discovery, no membership. If that is really all
//! it needs, then the shape of the network is free: agents that only relay,
//! several of them in a row, a star, a chain that comes back to a machine it
//! already visited, and links that stop existing partway through the work.
//!
//! These are the cases the four-machine chain cannot show, because that chain
//! is one shape. What is under test here is the model, not the wiring.

mod common;

use common::{
    Outer, Silent, chain_over, request, runtime, settle, start, start_behind, start_cuttable, until,
};
use p4_link::Impairment;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use std::sync::Arc;
use std::time::Duration;

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

/// Several agents in a row that own nothing.
///
/// The VPC case: the only machine reachable from outside owns no node and is
/// not on the chain, and neither is the one behind it. A relay decides "is
/// this mine" from the envelope alone, so a frame can pass through any number
/// of them and cost only the hops.
#[test]
fn a_frame_crosses_a_row_of_agents_that_own_nothing() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let door = start(Arc::new(Silent)).await;
        let middle = start(Arc::new(Silent)).await;
        let inner = start(Arc::new(Silent)).await;
        let worker = start(Arc::new(Silent)).await;
        worker
            .create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 8)
            .await;

        // The chain names only the worker. The three in front are not on it.
        let chain = chain_over(&[(&worker, "n0")]);
        for index in 0..24 {
            // Handed to the outermost, addressed at the innermost.
            door.enqueue(request(&format!("r{index}"), &chain, &outer, 3))
                .unwrap();
        }
        until(|| outer_duties.total_frames() >= 24 * (3 + 1)).await;

        assert_eq!(outer_duties.routes(), 24, "every request found its way");
        assert_eq!(
            outer_duties.total_frames(),
            24 * (3 + 1),
            "with all tokens and terminals"
        );
        // The two that were never addressed took no part, which is the claim:
        // relaying is not membership.
        assert_eq!(middle.traffic().consumed, 0, "the middle owned nothing");
        assert_eq!(inner.traffic().consumed, 0, "nor did the one behind it");
    });
}

/// One entry point in front of many workers.
///
/// Every request arrives at the same agent and leaves for a different machine.
/// Nothing about the entry is configured per worker — it reads an address.
#[test]
fn one_entry_point_serves_a_star_of_workers() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let door = start(Arc::new(Silent)).await;

        let mut workers = Vec::new();
        for index in 0..5 {
            let worker = start(Arc::new(Silent)).await;
            worker
                .create_node(
                    &format!("n{index}"),
                    Arc::new(Mock::terminal(0, Profile::default())),
                    8,
                )
                .await;
            workers.push(worker);
        }

        let mut sent = 0;
        for round in 0..8 {
            for (index, worker) in workers.iter().enumerate() {
                let chain = chain_over(&[(worker, &format!("n{index}"))]);
                door.enqueue(request(&format!("r{round}-{index}"), &chain, &outer, 2))
                    .unwrap();
                sent += 1;
            }
        }
        until(|| outer_duties.total_frames() >= sent * (2 + 1)).await;

        assert_eq!(outer_duties.routes(), sent, "every worker was reached");
        assert_eq!(
            door.traffic().forwarded as usize,
            sent,
            "the entry forwarded each without consuming any"
        );
    });
}

/// A chain that returns to a machine it has already used.
///
/// Position is what a link means, not identity, so a machine can appear more
/// than once — which is how a two-machine fleet runs a four-stage model.
#[test]
fn a_chain_may_return_to_a_machine_it_already_visited() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let one = start(Arc::new(Silent)).await;
        let two = start(Arc::new(Silent)).await;
        one.create_node("a0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        two.create_node("b0", Arc::new(Mock::staged(1, Profile::default())), 8)
            .await;
        one.create_node("a1", Arc::new(Mock::staged(2, Profile::default())), 8)
            .await;
        two.create_node("b1", Arc::new(Mock::terminal(3, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&one, "a0"), (&two, "b0"), (&one, "a1"), (&two, "b1")]);
        for index in 0..16 {
            one.enqueue(request(&format!("r{index}"), &chain, &outer, 4))
                .unwrap();
        }
        until(|| outer_duties.total_frames() >= 16 * (4 + 1)).await;

        for index in 0..16 {
            let frames = outer_duties.frames_for(&format!("r{index}"));
            assert_eq!(frames.len(), 5, "route {index} completed");
            assert_eq!(frames.last().unwrap().body, b"stop");
        }
    });
}

/// A machine that stops being reachable partway through.
///
/// Not a slow link — no link. The work behind it cannot arrive, and the claim
/// is that it is *answered* rather than left hanging: a caller waiting on a
/// terminal that never comes is what a leaked route looks like from outside.
#[test]
fn work_cut_off_mid_flight_is_answered_by_its_deadline() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let near = start(Arc::new(Silent)).await;
        let (far, cut) = start_cuttable(Arc::new(Silent), Impairment::default()).await;
        near.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        far.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&near, "n0"), (&far, "n1")]);
        cut.cut();
        for index in 0..12 {
            let mut frame = request(&format!("r{index}"), &chain, &outer, 3);
            frame.envelope.deadline_unix_ms = now_unix_ms() + 250;
            near.enqueue(frame).unwrap();
        }

        // Past the deadline, everything must have been accounted for. Some
        // routes are answered by the near node's own expiry check; none may be
        // silently gone.
        settle(900).await;
        // The cut has to be real, or this test asserts nothing: with the link
        // up these would have finished long ago.
        assert_eq!(
            outer_duties
                .frames_for("r0")
                .iter()
                .filter(|frame| frame.body == b"stop")
                .count(),
            0,
            "nothing reached the far side while the link was gone"
        );
        assert_eq!(
            near.node_depth("n0").await,
            Some(0),
            "nothing is still sitting on the near node"
        );
        for node in near.node_counts().await {
            assert!(node.contains("lost=0"), "{node}");
        }
    });
}

/// The link comes back.
///
/// A fleet reconnects constantly — a machine reboots, a switch flaps — and the
/// layer has to resume without anything being restarted or reconfigured,
/// because there is nothing to reconfigure.
#[test]
fn a_partition_that_heals_resumes_without_restarting_anything() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let near = start(Arc::new(Silent)).await;
        let (far, cut) = start_cuttable(Arc::new(Silent), Impairment::default()).await;
        near.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        far.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&near, "n0"), (&far, "n1")]);

        // Works, then the far machine goes away, then it comes back.
        near.enqueue(request("before", &chain, &outer, 2)).unwrap();
        until(|| outer_duties.frames_for("before").len() >= 3).await;

        cut.cut();
        settle(150).await;
        for index in 0..8 {
            near.enqueue(request(&format!("during{index}"), &chain, &outer, 2))
                .unwrap();
        }
        settle(200).await;
        // Proof the partition was one: these had ample time on a live link.
        assert!(
            (0..8).all(|index| outer_duties
                .frames_for(&format!("during{index}"))
                .is_empty()),
            "nothing crossed while the machine was unreachable"
        );
        cut.heal();

        // No restart, no reconnect command, no membership update.
        for index in 0..8 {
            near.enqueue(request(&format!("after{index}"), &chain, &outer, 2))
                .unwrap();
        }
        until(|| (0..8).all(|index| outer_duties.frames_for(&format!("after{index}")).len() >= 3))
            .await;

        for index in 0..8 {
            assert_eq!(
                outer_duties.frames_for(&format!("after{index}")).len(),
                3,
                "work after the heal completed"
            );
        }
    });
}

/// A partition on a link that is also slow, healing under continuous arrivals.
///
/// The combination is the one a fleet actually meets, and the one where a
/// design that treats reconnection as an event rather than a condition starts
/// dropping the first frame after every heal.
#[test]
fn arrivals_continue_across_a_cut_and_heal_on_a_slow_link() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let near = start_behind(
            Arc::new(Silent),
            Impairment::latency(Duration::from_millis(4), Duration::from_millis(6)),
        )
        .await;
        let (far, cut) = start_cuttable(
            Arc::new(Silent),
            Impairment::latency(Duration::from_millis(4), Duration::from_millis(6)),
        )
        .await;
        near.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        far.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&near, "n0"), (&far, "n1")]);
        for cycle in 0..3 {
            cut.heal();
            for index in 0..6 {
                near.enqueue(request(&format!("ok{cycle}-{index}"), &chain, &outer, 2))
                    .unwrap();
            }
            until(|| {
                (0..6)
                    .all(|index| outer_duties.frames_for(&format!("ok{cycle}-{index}")).len() >= 3)
            })
            .await;
            cut.cut();
            settle(80).await;
        }
        cut.heal();

        near.enqueue(request("last", &chain, &outer, 2)).unwrap();
        until(|| outer_duties.frames_for("last").len() >= 3).await;
        assert_eq!(
            outer_duties.frames_for("last").len(),
            3,
            "the first frame after the third heal was not swallowed"
        );
    });
}
