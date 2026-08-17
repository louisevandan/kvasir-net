//! Watching and steering an inference that is already running.
//!
//! The load's own visibility is next door in `protocol.rs`. These are the
//! questions an operator asks about work in flight: which node is holding
//! which request, stopping one of them without touching the rest, being told
//! plainly when there was nothing left to stop, and collecting an agent's
//! counters from another machine instead of reading them off its console.
//!
//! Every one goes through the message vocabulary. Nothing here reaches into an
//! agent in process.

mod common;

use common::deployment::place;
use common::{Outer, backends, chain_over, runtime, start, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::Standard;
use p4_service::message::{Reply, ToAgent, ToNode};
use std::sync::Arc;
use std::time::Duration;

/// Where a request is, while it is still there.
///
/// The chain says which machines an inference will visit; only the agent knows
/// which one it is on now and what that node is doing. Asking is a message
/// like any other.
#[test]
fn outer_can_see_which_node_is_holding_which_request() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "slow",
            "mock-slow",
            r#"{"l":"0-9"}"#,
            4,
        )
        .await;

        let single = chain_over(&[(&agent, "slow")]);
        for index in 0..6 {
            agent
                .enqueue(to_node(
                    &single,
                    &outer,
                    &format!("req{index}"),
                    QueueClass::Prefill,
                    ToNode::Execute {
                        prompt: "긴 작업".into(),
                        max_tokens: 6,
                        options: "{}".into(),
                    },
                ))
                .unwrap();
        }

        // Asked while the backend is still working.
        tokio::time::sleep(Duration::from_millis(80)).await;
        agent
            .enqueue(to_agent(&agent, &outer, "status", ToAgent::Status))
            .unwrap();
        until(|| !seen.replies("status").is_empty()).await;

        let Some(Reply::Status { snapshot }) = seen.replies("status").into_iter().next() else {
            panic!("a status reply came back");
        };
        assert!(
            snapshot.contains("node=slow"),
            "the node is named: {snapshot}"
        );
        assert!(
            snapshot.contains("running=true") || snapshot.contains("depth="),
            "with what it is doing: {snapshot}"
        );
        assert!(
            (0..6).any(|index| snapshot.contains(&format!("req{index}"))),
            "and the requests it is holding, by route: {snapshot}"
        );
    });
}

/// Stopping one request without touching the others.
///
/// The backend never answers, on purpose. That is the only way to know a route
/// is still queued rather than racing to observe it: the first hop never ends,
/// so nothing behind it can be claimed, and what is queued stays queued. An
/// earlier version of this test watched the node's status and then cancelled,
/// which lost the race often enough to fail one run in three.
#[test]
fn one_inference_can_be_cancelled_while_the_rest_carry_on() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "held",
            "mock-silent",
            r#"{"l":"0-9"}"#,
            1,
        )
        .await;

        let single = chain_over(&[(&agent, "held")]);
        for route in ["keep0", "keep1", "keep2", "drop-me", "keep3"] {
            agent
                .enqueue(to_node(
                    &single,
                    &outer,
                    route,
                    QueueClass::Prefill,
                    ToNode::Execute {
                        prompt: "대기".into(),
                        max_tokens: 3,
                        options: "{}".into(),
                    },
                ))
                .unwrap();
        }

        // Wait until the node is holding it, using the same status message an
        // operator would. Every reply so far rather than the first: reading
        // only the first latched onto a snapshot taken before the work arrived.
        // Both halves, in one snapshot. That the route is waiting is not enough
        // on its own — the node might not have claimed anything yet, and the
        // hop it claims next could be this one. `running=1 ` says it is already
        // holding a hop, and with a backend that never answers, a hop it is
        // holding is one it never lets go of, so nothing more will be claimed.
        // Watching only for the route lost this race about one run in five.
        until(|| {
            if seen.replies("where").iter().any(|reply| {
                matches!(reply, Reply::Status { snapshot }
                    if snapshot.contains("drop-me") && snapshot.contains("running=1 "))
            }) {
                return true;
            }
            agent
                .enqueue(to_agent(&agent, &outer, "where", ToAgent::Status))
                .unwrap();
            false
        })
        .await;

        agent
            .enqueue(to_agent(
                &agent,
                &outer,
                "cancel",
                ToAgent::Cancel {
                    route: "drop-me".into(),
                },
            ))
            .unwrap();
        until(|| !seen.replies("cancel").is_empty()).await;
        assert!(
            matches!(seen.replies("cancel").first(), Some(Reply::Accepted { .. })),
            "the cancellation found something to stop: {:?}",
            seen.replies("cancel")
        );

        // What it did not touch: the others are still there. That is the claim
        // — cancelling one route leaves the rest alone — and it is checkable
        // without waiting for work this backend will never finish.
        agent
            .enqueue(to_agent(&agent, &outer, "after", ToAgent::Status))
            .unwrap();
        until(|| !seen.replies("after").is_empty()).await;
        let Some(Reply::Status { snapshot }) = seen.replies("after").into_iter().next() else {
            panic!("a status reply came back");
        };
        assert!(
            !snapshot.contains("drop-me"),
            "the cancelled route is gone: {snapshot}"
        );
        // Which of the others is still queued rather than in flight is not
        // fixed: routes are spread across workers by hash, so the order they
        // reach the node is not the order they were sent. What is fixed is
        // that cancelling one route left the rest alone.
        let kept = ["keep0", "keep1", "keep2", "keep3"]
            .iter()
            .filter(|route| snapshot.contains(*route))
            .count();
        assert!(
            kept >= 3,
            "only drop-me was taken; {kept} of four keeps remain: {snapshot}"
        );
    });
}

/// Cancelling something that already finished says so rather than pretending.
#[test]
fn cancelling_a_finished_request_reports_that_there_was_nothing_to_stop() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "n0",
            "mock-tail",
            r#"{"l":"0-9"}"#,
            4,
        )
        .await;

        agent
            .enqueue(to_agent(
                &agent,
                &outer,
                "cancel",
                ToAgent::Cancel {
                    route: "never-existed".into(),
                },
            ))
            .unwrap();
        until(|| !seen.replies("cancel").is_empty()).await;
        assert!(
            matches!(seen.replies("cancel").first(), Some(Reply::Failed { .. })),
            "a caller can tell a cancellation from a race it lost: {:?}",
            seen.replies("cancel")
        );
    });
}

/// Traffic and queue depths, collected from another machine.
#[test]
fn outer_can_collect_traffic_and_queue_statistics() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "n0",
            "mock-tail",
            r#"{"l":"0-9"}"#,
            4,
        )
        .await;

        let single = chain_over(&[(&agent, "n0")]);
        for index in 0..12 {
            agent
                .enqueue(to_node(
                    &single,
                    &outer,
                    &format!("r{index}"),
                    QueueClass::Prefill,
                    ToNode::Execute {
                        prompt: "일".into(),
                        max_tokens: 2,
                        options: "{}".into(),
                    },
                ))
                .unwrap();
        }
        until(|| {
            (0..12).all(|index| {
                seen.replies(&format!("r{index}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Done { .. }))
            })
        })
        .await;

        agent
            .enqueue(to_agent(&agent, &outer, "stats", ToAgent::Status))
            .unwrap();
        until(|| !seen.replies("stats").is_empty()).await;

        let Some(Reply::Status { snapshot }) = seen.replies("stats").into_iter().next() else {
            panic!("a status reply came back");
        };
        for field in [
            "address=",
            "forwarded=",
            "consumed=",
            "to_nodes=",
            "unrouted=",
            "prefill=",
            "peers=",
            "waiting=",
            "node=n0",
        ] {
            assert!(snapshot.contains(field), "{field} is reported: {snapshot}");
        }
        assert!(
            !snapshot.contains("to_nodes=0 "),
            "and the numbers are the real ones: {snapshot}"
        );
    });
}

/// What a backend says about itself reaches OUTER, and the layer between never
/// reads it.
///
/// The mirror of a plan. A plan goes down opaque and this comes up the same
/// way, which is what lets a backend be observable without the core learning
/// what a backend is.
///
/// It exists because every defect found under load in this layer was diagnosed
/// from a backend's own log and the machine's socket table — how many streams
/// were open, how many had to be reached twice, how many were refused — and
/// none of that could be seen from anywhere else. A caller on another machine
/// had no way to tell a busy deployment from a broken one.
#[test]
fn a_backend_describes_itself_through_the_status_message() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(
            &agent,
            &outer,
            &seen,
            "n0",
            "mock-solo",
            r#"{"l":"0-9"}"#,
            4,
        )
        .await;

        agent
            .enqueue(to_agent(&agent, &outer, "ask", ToAgent::Status))
            .unwrap();
        until(|| !seen.replies("ask").is_empty()).await;

        let snapshot = seen
            .replies("ask")
            .iter()
            .find_map(|reply| match reply {
                Reply::Status { snapshot } => Some(snapshot.clone()),
                _ => None,
            })
            .expect("a status snapshot");

        assert!(
            snapshot.contains("backend=["),
            "the node's line carries what the backend said: {snapshot}"
        );
        // The mock has nothing to say, and says nothing — an adapter is not
        // obliged to report, and an empty report must not look like a missing
        // field.
        assert!(
            snapshot.contains("backend=[]"),
            "an adapter with nothing to say leaves it empty: {snapshot}"
        );
    });
}
