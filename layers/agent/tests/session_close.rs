//! The defect this pins: a chain's tail is the only node that ever observes
//! a native stop (EOS, well short of the request's own bound), and nothing
//! sent a hop back to the stages before it to say so. Every one of their
//! reservations leaked, one per request, forever -- measured as `Parallel 4,
//! Requests 8` leaving exactly four requests never admitted, stage 0 stuck
//! at four reservations while the tail sat at zero.
//!
//! The pass condition is deliberately not "every request completed". A
//! chain that finishes every request while still leaking every
//! intermediate stage's slot is exactly the sibling defect this also has to
//! catch: completion count alone would keep passing right up until the
//! ceiling ran out. So this asserts, in order: every request reaches its own
//! terminal, every stage's own admission gate reports zero reservations
//! afterwards -- not just the tail, which could always self-report zero on
//! its own -- and the deployment is still unloadable, which a close stuck
//! holding a node's one lifecycle slot would prevent.
mod common;

use common::{
    Lifecycle, Outer, Silent, chain_over, control, request, runtime, start, start_with, until,
};
use p4_mock::Mock;
use p4_mock::profile::Profile;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn a_sequence_that_stops_early_frees_every_stage_it_passed_through() {
    runtime().block_on(async {
        let ceiling = 2usize;
        let requests = ceiling * 2;
        // `eos_after_turns: Some(1)` is what makes this early rather than
        // length-terminal: `remaining: 200` on each request below is nowhere
        // close to reached, so nothing here exercises (or could be confused
        // with) the length-terminal path already fixed and pinned elsewhere.
        let profile = Profile {
            reserve_slots: true,
            eos_after_turns: Some(1),
            ..Profile::default()
        };

        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;

        let head = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        let tail = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        head.create_node(
            "s0",
            Arc::new(Mock::staged(0, profile.clone())) as Arc<_>,
            ceiling,
        )
        .await;
        tail.create_node(
            "s1",
            Arc::new(Mock::terminal(1, profile.clone())) as Arc<_>,
            ceiling,
        )
        .await;

        let chain = chain_over(&[(&head, "s0"), (&tail, "s1")]);
        for index in 0..requests {
            head.enqueue(request(&format!("r{index}"), &chain, &outer, 200))
                .unwrap();
        }

        // The completion condition is per-request, not an aggregate frame
        // count: a defect that stalls two of four requests must not be
        // masked by the other two producing enough traffic to look done.
        let route = |index: usize| format!("r{index}");
        let every_request_reached_its_own_eos = || {
            (0..requests).all(|index| {
                outer_duties
                    .frames_for(&route(index))
                    .iter()
                    .any(|frame| frame.body == b"eos")
            })
        };
        until(every_request_reached_its_own_eos).await;
        assert!(
            every_request_reached_its_own_eos(),
            "every one of {requests} requests must reach its own terminal at ceiling {ceiling}, \
             not just the first {ceiling} admitted"
        );

        // `until` cannot await an async condition, so this polls by hand.
        // The close for the last-finished request's outcome is emitted right
        // after its reply (see `events.rs`), so a short grace window on top
        // of the reply already having arrived is enough.
        let mut head_reserved = None;
        let mut tail_reserved = None;
        for _ in 0..100 {
            head_reserved = head.node_reserved("s0").await;
            tail_reserved = tail.node_reserved("s1").await;
            if head_reserved == Some(0) && tail_reserved == Some(0) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            head_reserved,
            Some(0),
            "the intermediate stage must not still be holding a slot for a \
             sequence whose tail already finished it -- this is the leak \
             the close broadcast exists to close"
        );
        assert_eq!(
            tail_reserved,
            Some(0),
            "the tail must have released its own reservation too, the same \
             way it always has"
        );

        // Unloadable afterwards: a close that failed to complete its own
        // lifecycle carrier would leave a node's single lifecycle slot
        // occupied, and this would hang rather than reply.
        for (agent, node) in [(&head, "s0"), (&tail, "s1")] {
            let unload_chain = chain_over(&[(agent, node)]);
            agent
                .enqueue(control(
                    &format!("unload-{node}"),
                    &unload_chain,
                    &outer,
                    "unload",
                ))
                .unwrap();
        }
        until(|| {
            outer_duties.frames_for("unload-s0").len() == 1
                && outer_duties.frames_for("unload-s1").len() == 1
        })
        .await;
        for node in ["s0", "s1"] {
            let frames = outer_duties.frames_for(&format!("unload-{node}"));
            assert_eq!(frames.len(), 1, "unload for {node} must reply exactly once");
            assert_eq!(
                frames[0].body, b"unloaded",
                "unload for {node} must succeed, not fail and not hang"
            );
        }
    });
}
