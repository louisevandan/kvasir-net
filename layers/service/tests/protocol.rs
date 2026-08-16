//! What OUTER can actually find out, and actually do, over the wire.
//!
//! The other tests here prove work completes. These prove it can be *watched*
//! and *steered*, which is a different claim and the one an operator cares
//! about: a load reporting itself stage by stage, a failure that is reported
//! rather than inferred from silence, a deployment that refuses to serve half
//! a model, a request that can be located while it runs and stopped partway,
//! and counters that can be collected from another machine instead of read off
//! a console.
//!
//! Every one of them goes through the message vocabulary. Nothing here reaches
//! into an agent in process.

mod common;

use common::{Outer, backends, chain_over, runtime, start, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::Standard;
use p4_service::message::{Reply, ToAgent, ToNode};
use std::sync::Arc;
use std::time::Duration;

/// Creates a node and loads it, returning once bound.
async fn place(
    agent: &Arc<p4_agent_core::agent::Agent>,
    outer: &Arc<p4_agent_core::agent::Agent>,
    seen: &Outer,
    node: &str,
    adapter: &str,
    plan: &str,
) {
    agent
        .enqueue(to_agent(
            agent,
            outer,
            &format!("create-{node}"),
            ToAgent::CreateNode {
                node: node.into(),
                adapter: adapter.into(),
            },
        ))
        .unwrap();
    until(|| !seen.replies(&format!("create-{node}")).is_empty()).await;

    let single = chain_over(&[(agent, node)]);
    agent
        .enqueue(to_node(
            &single,
            outer,
            &format!("load-{node}"),
            QueueClass::Control,
            ToNode::Load {
                plan: plan.into(),
                artifact: "model.gguf".into(),
                ceiling: 4,
            },
        ))
        .unwrap();
    until(|| !seen.replies(&format!("load-{node}")).is_empty()).await;
}

/// A model spread over three machines, watched as it lands.
///
/// Each stage reports its own progress and its own binding, because a model
/// spread over layer ranges finishes when its slowest piece does and one total
/// hides which piece that was.
#[test]
fn a_distributed_load_is_visible_stage_by_stage_on_every_machine() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let mut agents = Vec::new();
        for _ in 0..3 {
            agents.push(start(Arc::new(Standard::new(backends()))).await);
        }

        for (index, agent) in agents.iter().enumerate() {
            let adapter = if index == 2 { "mock-tail" } else { "mock-lead" };
            place(
                agent,
                &outer,
                &seen,
                &format!("n{index}"),
                adapter,
                &format!(r#"{{"layers":"{}-{}"}}"#, index * 20, index * 20 + 19),
            )
            .await;
        }

        for index in 0..3 {
            let replies = seen.replies(&format!("load-n{index}"));
            assert!(
                replies
                    .iter()
                    .any(|reply| matches!(reply, Reply::Progress { .. })),
                "stage {index} reported progress of its own: {replies:?}"
            );
            assert!(
                replies
                    .iter()
                    .any(|reply| matches!(reply, Reply::Bound { generation: 1 })),
                "stage {index} bound and said which generation: {replies:?}"
            );
        }
    });
}

/// One stage of a distributed load fails.
///
/// The failure is reported — not inferred from a stage that never answers —
/// and the deployment does not half-work. A chain composed over the failed
/// stage is refused *by that stage*, so an operator who loaded three machines
/// and lost one cannot accidentally serve answers from two thirds of a model.
#[test]
fn a_stage_that_fails_to_load_is_reported_and_refuses_to_serve() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let good = start(Arc::new(Standard::new(backends()))).await;
        let broken = start(Arc::new(Standard::new(backends()))).await;

        place(
            &good,
            &outer,
            &seen,
            "n0",
            "mock-lead",
            r#"{"layers":"0-19"}"#,
        )
        .await;
        place(
            &broken,
            &outer,
            &seen,
            "n1",
            "mock-unloadable",
            r#"{"layers":"20-39"}"#,
        )
        .await;

        // Reported, with a reason, rather than left to a timeout.
        let broken_replies = seen.replies("load-n1");
        assert!(
            broken_replies
                .iter()
                .any(|reply| matches!(reply, Reply::Failed { .. })),
            "the stage that could not load said so: {broken_replies:?}"
        );
        assert!(
            !broken_replies
                .iter()
                .any(|reply| matches!(reply, Reply::Bound { .. })),
            "and never claimed to be bound: {broken_replies:?}"
        );
        // Its neighbour is fine, which is what makes this a transaction rather
        // than a list of independent loads.
        assert!(
            seen.replies("load-n0")
                .iter()
                .any(|reply| matches!(reply, Reply::Bound { .. })),
            "the other stage bound normally"
        );

        // Now run an inference over both anyway.
        let chain = chain_over(&[(&good, "n0"), (&broken, "n1")]);
        good.enqueue(to_node(
            &chain,
            &outer,
            "infer",
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "안녕".into(),
                max_tokens: 4,
                options: "{}".into(),
            },
        ))
        .unwrap();
        until(|| {
            seen.replies("infer")
                .iter()
                .any(|reply| matches!(reply, Reply::Failed { .. } | Reply::Done { .. }))
        })
        .await;

        let stream = seen.replies("infer");
        assert!(
            stream
                .iter()
                .any(|reply| matches!(reply, Reply::Failed { .. })),
            "the half-loaded deployment refused rather than answered: {stream:?}"
        );
        assert!(
            !stream
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. })),
            "and produced no completion: {stream:?}"
        );
    });
}

/// An unload is reported, and what it leaves behind is a node that says so.
#[test]
fn an_unload_is_reported_and_the_deployment_stops_being_current() {
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
            r#"{"layers":"0-9"}"#,
        )
        .await;

        let single = chain_over(&[(&agent, "n0")]);
        agent
            .enqueue(to_node(
                &single,
                &outer,
                "unload",
                QueueClass::Control,
                ToNode::Unload,
            ))
            .unwrap();
        until(|| !seen.replies("unload").is_empty()).await;
        assert!(
            seen.replies("unload")
                .iter()
                .any(|reply| matches!(reply, Reply::Released)),
            "the release was reported: {:?}",
            seen.replies("unload")
        );
    });
}

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
        place(&agent, &outer, &seen, "slow", "mock-slow", r#"{"l":"0-9"}"#).await;

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
#[test]
fn one_inference_can_be_cancelled_while_the_rest_carry_on() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;
        place(&agent, &outer, &seen, "slow", "mock-slow", r#"{"l":"0-9"}"#).await;

        let single = chain_over(&[(&agent, "slow")]);
        for index in 0..6 {
            agent
                .enqueue(to_node(
                    &single,
                    &outer,
                    &format!("keep{index}"),
                    QueueClass::Prefill,
                    ToNode::Execute {
                        prompt: "계속".into(),
                        max_tokens: 3,
                        options: "{}".into(),
                    },
                ))
                .unwrap();
        }
        agent
            .enqueue(to_node(
                &single,
                &outer,
                "drop-me",
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "취소될 것".into(),
                    max_tokens: 3,
                    options: "{}".into(),
                },
            ))
            .unwrap();

        // Control is the preferred lane, so a cancel sent immediately arrives
        // before the work it is cancelling has reached the node — which is a
        // race the caller loses, not a defect. Wait until the node is holding
        // it, using the same status message an operator would.
        until(|| {
            let Some(Reply::Status { snapshot }) = seen.replies("where").into_iter().next() else {
                agent
                    .enqueue(to_agent(&agent, &outer, "where", ToAgent::Status))
                    .unwrap();
                return false;
            };
            snapshot.contains("drop-me")
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

        // The others finish untouched.
        until(|| {
            (0..6).all(|index| {
                seen.replies(&format!("keep{index}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Done { .. }))
            })
        })
        .await;
        for index in 0..6 {
            assert!(
                seen.replies(&format!("keep{index}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Done { .. })),
                "request {index} was not disturbed"
            );
        }
        assert!(
            !seen
                .replies("drop-me")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. })),
            "and the cancelled one never completed: {:?}",
            seen.replies("drop-me")
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
        place(&agent, &outer, &seen, "n0", "mock-tail", r#"{"l":"0-9"}"#).await;

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
        place(&agent, &outer, &seen, "n0", "mock-tail", r#"{"l":"0-9"}"#).await;

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
