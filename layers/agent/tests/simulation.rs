//! The whole system, over sockets, with mocks behind the adapter boundary.
//!
//! Every agent here is the real one and every hop crosses a real connection.
//! What is simulated is only what sits below the interface, so anything these
//! runs catch is P4's.

mod common;

use common::{Outer, Silent, chain_over, request, runtime, settle, start, until};
use p4_adapter::Adapter;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::QueueClass;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn a_three_stage_chain_generates_across_three_machines() {
    // Prefill traverses the chain once; every further token costs one lap of
    // the ring. Each stage lives on its own agent and every hop is a socket.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;
        let c = start(Arc::new(Silent)).await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::staged(1, Profile::default())), 8)
            .await;
        c.create_node("n2", Arc::new(Mock::terminal(2, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1"), (&c, "n2")]);
        a.enqueue(request("r1", &chain, &outer, 4)).unwrap();
        until(|| outer_duties.frames_for("r1").len() >= 5).await;

        let frames = outer_duties.frames_for("r1");
        assert!(!frames.is_empty(), "the caller heard back");
        assert!(
            frames
                .iter()
                .all(|f| f.envelope.lane == QueueClass::Response),
            "everything returned on the response lane"
        );
        // Four tokens wanted: four token frames, then one terminal.
        assert_eq!(frames.len(), 5, "tokens plus the terminal");
        assert_eq!(frames.last().unwrap().body, b"stop");
    });
}

#[test]
fn a_single_node_chain_generates_the_same_way() {
    // How vLLM and SGLang participate: the model is spread inside the backend,
    // so the chain is one link and a lap is a decode step on the same node.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let only = start(Arc::new(Silent)).await;
        only.create_node("n0", Arc::new(Mock::internal(Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&only, "n0")]);
        only.enqueue(request("r1", &chain, &outer, 3)).unwrap();
        until(|| outer_duties.frames_for("r1").len() >= 4).await;

        let frames = outer_duties.frames_for("r1");
        let bodies: Vec<String> = frames
            .iter()
            .map(|f| String::from_utf8_lossy(&f.body).into_owned())
            .collect();
        assert_eq!(frames.len(), 4, "saw {bodies:?}");
        assert_eq!(
            frames.last().unwrap().body,
            b"stop",
            "the stream ended on its terminal, saw {bodies:?}"
        );
    });
}

#[test]
fn every_request_of_a_crowd_reaches_a_terminal() {
    // Far more arrivals than the ceiling admits. None may be lost, and none
    // may produce two terminals.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        let first = Arc::new(Mock::staged(0, Profile::default()));
        a.create_node("n0", Arc::clone(&first) as Arc<dyn Adapter>, 4)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..40 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        until(|| outer_duties.total_frames() >= 40 * (1 + 1)).await;

        assert_eq!(outer_duties.routes(), 40, "every route answered");
        assert_eq!(
            outer_duties.total_frames(),
            40 * (1 + 1),
            "one token and terminal each"
        );
        assert!(
            first.widths().iter().all(|width| *width <= 4),
            "no hop exceeded the declared ceiling: {:?}",
            first.widths()
        );
        assert_eq!(
            first.peak_concurrent_hops(),
            1,
            "a node never ran two hops at once"
        );
    });
}

#[test]
fn arrivals_are_batched_rather_than_serialised() {
    // A window wider than one is the whole reason a hop carries a batch. If
    // every hop were width one, the node would be serialising.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;

        let node = Arc::new(Mock::terminal(
            0,
            Profile::measured_shape(Duration::from_millis(15)),
        ));
        a.create_node("n0", Arc::clone(&node) as Arc<dyn Adapter>, 16)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..32 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        until(|| outer_duties.routes() >= 32).await;

        let widest = node.widths().into_iter().max().unwrap_or(0);
        assert!(widest > 1, "hops carried batches, widest was {widest}");
        assert!(widest <= 16, "and never past the ceiling");
    });
}

#[test]
fn a_slow_node_shows_up_as_node_depth_not_agent_depth() {
    // The attribution that makes "is this P4's fault" answerable. With a slow
    // backend the work should pile up behind the adapter, not in front of it.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;

        let slow = Profile {
            leading_hop: Duration::from_millis(60),
            ..Profile::default()
        };
        a.create_node("n0", Arc::new(Mock::terminal(0, slow)), 2)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..24 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        settle(200).await;

        let node_depth = a.node_depth("n0").await.unwrap_or(0);
        assert!(
            node_depth > 0,
            "work waited on the node, where the slowness is"
        );
    });
}

#[test]
fn a_chain_through_an_agent_that_owns_no_node_still_works() {
    // The entry agent is a pure entry point: it holds no node and only relays.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let entry = start(Arc::new(Silent)).await;
        let worker = start(Arc::new(Silent)).await;
        worker
            .create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&worker, "n0")]);
        // Sent to the entry agent, addressed at the worker's node.
        entry.enqueue(request("r1", &chain, &outer, 2)).unwrap();
        until(|| outer_duties.frames_for("r1").len() >= 3).await;

        assert_eq!(outer_duties.frames_for("r1").len(), 3);
    });
}

#[test]
fn sustained_arrivals_keep_completing_while_more_come_in() {
    // Cohort replacement: work continues to arrive while earlier work is still
    // in flight, and the node keeps admitting as it frees up.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        let first = Arc::new(Mock::staged(
            0,
            Profile {
                leading_hop: Duration::from_millis(4),
                ..Profile::default()
            },
        ));
        a.create_node("n0", Arc::clone(&first) as Arc<dyn Adapter>, 6)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 6)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for wave in 0..6 {
            for index in 0..8 {
                a.enqueue(request(&format!("w{wave}-r{index}"), &chain, &outer, 1))
                    .unwrap();
            }
            settle(60).await;
        }
        until(|| outer_duties.routes() >= 48).await;

        assert_eq!(outer_duties.routes(), 48, "every wave completed");
        assert_eq!(first.peak_concurrent_hops(), 1);
        assert!(first.widths().iter().all(|width| *width <= 6));
    });
}

#[test]
fn a_failing_backend_answers_the_route_instead_of_stranding_it() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Hop,
                    ..Profile::default()
                },
            )),
            4,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(request("r1", &chain, &outer, 2)).unwrap();
        until(|| !outer_duties.frames_for("r1").is_empty()).await;

        let frames = outer_duties.frames_for("r1");
        assert_eq!(frames.len(), 1, "the caller was told once");
        assert!(
            String::from_utf8_lossy(&frames[0].body).contains("fail"),
            "and told what happened"
        );
    });
}

#[test]
fn concurrent_chains_on_shared_agents_do_not_mix_their_routes() {
    // Two chains crossing the same two agents. A token belonging to one route
    // must never surface under the other.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        a.create_node("left", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        a.create_node("right", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("tail", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let left = chain_over(&[(&a, "left"), (&b, "tail")]);
        let right = chain_over(&[(&a, "right"), (&b, "tail")]);
        for index in 0..10 {
            a.enqueue(request(&format!("L{index}"), &left, &outer, 2))
                .unwrap();
            a.enqueue(request(&format!("R{index}"), &right, &outer, 3))
                .unwrap();
        }
        until(|| {
            (0..10).all(|index| {
                outer_duties.frames_for(&format!("L{index}")).len() >= 3
                    && outer_duties.frames_for(&format!("R{index}")).len() >= 4
            })
        })
        .await;

        for index in 0..10 {
            assert_eq!(
                outer_duties.frames_for(&format!("L{index}")).len(),
                3,
                "left route {index} got its own token count"
            );
            assert_eq!(
                outer_duties.frames_for(&format!("R{index}")).len(),
                4,
                "right route {index} got its own token count"
            );
        }
    });
}

#[test]
fn a_token_stream_arrives_in_the_order_it_was_produced() {
    // Registration order is the only ordering guarantee there is, so it has to
    // survive the queue, the worker pool and the wire. Handing consecutive
    // frames of one route to different workers is what broke this before.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;

        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        for index in 0..6 {
            a.enqueue(request(&format!("s{index}"), &chain, &outer, 5))
                .unwrap();
        }
        until(|| (0..6).all(|i| outer_duties.frames_for(&format!("s{i}")).len() >= 6)).await;

        for index in 0..6 {
            let route = format!("s{index}");
            let bodies: Vec<String> = outer_duties
                .frames_for(&route)
                .iter()
                .map(|f| String::from_utf8_lossy(&f.body).into_owned())
                .collect();
            assert_eq!(
                bodies,
                vec![
                    format!("{route}#1 "),
                    format!("{route}#2 "),
                    format!("{route}#3 "),
                    format!("{route}#4 "),
                    format!("{route}#5 "),
                    "stop".to_string(),
                ],
                "route {route} arrived out of order"
            );
        }
    });
}
