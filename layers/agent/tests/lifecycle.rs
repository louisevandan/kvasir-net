//! Load, unload, deadlines and cancellation.
//!
//! The lifecycle half of the simulation: what happens to a node before and
//! after it is carrying work, and what happens to work that must not run.

mod common;

use common::{
    Lifecycle, Outer, Silent, chain_over, control, request, runtime, settle, start, start_with,
    until,
};
use p4_adapter::Adapter;
use p4_agent_core::agent::Agent;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::Chain;
use p4_protocol::frame::Frame;
use std::sync::Arc;

#[test]
fn a_distributed_load_reports_every_stage_then_binds() {
    // The load workload: a model spread over stages, each reporting on its own,
    // and the deployment executable only once the last one is in.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    stages: 4,
                    ..Profile::default()
                },
            )),
            1,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("load-1", &chain, &outer, "load|6"))
            .unwrap();
        until(|| outer_duties.frames_for("load-1").len() >= 5).await;

        let bodies: Vec<String> = outer_duties
            .frames_for("load-1")
            .iter()
            .map(|f| String::from_utf8_lossy(&f.body).into_owned())
            .collect();
        assert_eq!(bodies.len(), 5, "four stages then the binding: {bodies:?}");
        assert!(bodies[0].starts_with("stage 0"), "{bodies:?}");
        assert!(bodies[3].starts_with("stage 3"), "{bodies:?}");
        assert!(bodies[4].starts_with("loaded generation 1"), "{bodies:?}");
    });
}

#[test]
fn a_load_raises_the_ceiling_the_plan_declared() {
    // The ceiling is declared, never derived. Before the load the node admits
    // one at a time; after it, the declared width.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        let node = Arc::new(Mock::terminal(0, Profile::default()));
        a.create_node("n0", Arc::clone(&node) as Arc<dyn Adapter>, 1)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("load-1", &chain, &outer, "load|8"))
            .unwrap();
        until(|| !outer_duties.frames_for("load-1").is_empty()).await;

        for index in 0..24 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        until(|| outer_duties.routes() >= 25).await;

        let widest = node.widths().into_iter().max().unwrap_or(0);
        assert!(widest > 1, "the declared ceiling was used, widest {widest}");
        assert!(widest <= 8, "and not exceeded, widest {widest}");
    });
}

#[test]
fn an_unload_is_reported_and_the_node_keeps_serving_afterwards() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("unload-1", &chain, &outer, "unload"))
            .unwrap();
        until(|| !outer_duties.frames_for("unload-1").is_empty()).await;

        assert_eq!(outer_duties.frames_for("unload-1")[0].body, b"unloaded");

        // The node is idle again rather than stuck holding a finished
        // instruction, so ordinary work still runs.
        a.enqueue(request("after", &chain, &outer, 1)).unwrap();
        until(|| !outer_duties.frames_for("after").is_empty()).await;
        assert_eq!(outer_duties.frames_for("after").len(), 2);
    });
}

#[test]
fn a_load_that_fails_is_reported_and_does_not_wedge_the_node() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start_with(Arc::new(outer_duties.clone()), Arc::new(Lifecycle)).await;
        let a = start_with(Arc::new(Silent), Arc::new(Lifecycle)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Load,
                    ..Profile::default()
                },
            )),
            4,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(control("load-1", &chain, &outer, "load|4"))
            .unwrap();
        until(|| !outer_duties.frames_for("load-1").is_empty()).await;

        let bodies: Vec<String> = outer_duties
            .frames_for("load-1")
            .iter()
            .map(|f| String::from_utf8_lossy(&f.body).into_owned())
            .collect();
        assert!(
            bodies.iter().any(|body| body.contains("fail")),
            "the caller was told: {bodies:?}"
        );
        assert_eq!(a.node_depth("n0").await, Some(0), "nothing left stuck");
    });
}

fn expiring(route: &str, chain: &Chain, outer: &Arc<Agent>, deadline_unix_ms: u64) -> Frame {
    let mut frame = request(route, chain, outer, 2);
    frame.envelope.deadline_unix_ms = deadline_unix_ms;
    frame
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

#[test]
fn work_whose_deadline_already_passed_is_answered_rather_than_run() {
    // Expired work must not reach a backend, and must not vanish either: a
    // caller left waiting for a terminal that never comes is what a leaked
    // route looks like from outside.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let node = Arc::new(Mock::terminal(0, Profile::default()));
        a.create_node("n0", Arc::clone(&node) as Arc<dyn Adapter>, 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(expiring("late", &chain, &outer, now_unix_ms() - 1_000))
            .unwrap();
        until(|| !outer_duties.frames_for("late").is_empty()).await;

        let body = String::from_utf8_lossy(&outer_duties.frames_for("late")[0].body).into_owned();
        assert!(body.contains("deadline"), "the caller was told why: {body}");
        assert!(
            node.widths().is_empty(),
            "expired work never reached the backend"
        );
    });
}

#[test]
fn a_live_deadline_does_not_stop_ordinary_work() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(expiring("soon", &chain, &outer, now_unix_ms() + 60_000))
            .unwrap();
        until(|| outer_duties.frames_for("soon").len() >= 3).await;

        assert_eq!(outer_duties.frames_for("soon").len(), 3);
    });
}

#[test]
fn cancelling_stops_the_work_that_has_not_started() {
    // A hop already inside a backend runs to its boundary; cancelling means
    // the next one never starts. Here the backend never answers, so everything
    // behind the first hop is still waiting and can be withdrawn.
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Silence,
                    ..Profile::default()
                },
            )),
            1,
        )
        .await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..6 {
            a.enqueue(request(&format!("c{index}"), &chain, &outer, 1))
                .unwrap();
        }
        // The first hop is inside the silent backend; the rest are queued.
        settle(300).await;

        let mut cancelled = 0;
        for index in 0..6 {
            if a.cancel(&format!("c{index}")).await {
                cancelled += 1;
            }
        }
        assert!(cancelled >= 5, "the queued work was withdrawn, {cancelled}");
        assert_eq!(a.node_depth("n0").await, Some(0), "nothing left queued");
    });
}

#[test]
fn an_in_flight_hop_is_fenced_and_cancelled_at_its_deadline() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Silence,
                    ..Profile::default()
                },
            )),
            1,
        )
        .await;
        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(expiring(
            "in-flight-timeout",
            &chain,
            &outer,
            now_unix_ms() + 100,
        ))
        .unwrap();

        until(|| !outer_duties.frames_for("in-flight-timeout").is_empty()).await;
        let frames = outer_duties.frames_for("in-flight-timeout");
        assert_eq!(frames.len(), 1, "deadline produces one terminal response");
        assert!(String::from_utf8_lossy(&frames[0].body).contains("deadline"));
        settle(300).await;
        assert_eq!(a.node_depth("n0").await, Some(0));
        assert_eq!(a.node_status().await[0].running, 0);
    });
}

#[test]
fn a_non_cooperative_adapter_remains_visible_as_timed_out_until_terminal_event() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    fault: p4_mock::profile::Fault::Stubborn,
                    ..Profile::default()
                },
            )),
            1,
        )
        .await;
        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(expiring(
            "stubborn-timeout",
            &chain,
            &outer,
            now_unix_ms() + 40,
        ))
        .unwrap();

        until(|| !outer_duties.frames_for("stubborn-timeout").is_empty()).await;
        let status = a.node_status().await;
        let active = status[0]
            .active_hop
            .as_ref()
            .expect("timed-out adapter remains visible while still running");
        assert!(active.timed_out);
        settle(250).await;
        assert!(a.node_status().await[0].active_hop.is_none());
    });
}

#[test]
fn cancelling_a_route_that_already_finished_says_so() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(request("done", &chain, &outer, 1)).unwrap();
        until(|| !outer_duties.frames_for("done").is_empty()).await;

        assert!(
            !a.cancel("done").await,
            "there was nothing left to cancel, which is not the same as failing"
        );
    });
}
