//! More than one node on the same agent.
//!
//! An agent is a machine, and a machine holds as many nodes as it has room
//! for. Every other test here gives an agent exactly one, which hides the
//! question this file exists to answer: that the nodes are separate queues
//! and separate lifecycles behind one address, and that a route passing
//! through the same machine twice is two visits rather than one.
//!
//! This is the shape a real placement takes. A chain of eight over four
//! machines is two nodes on each, and nothing above the node knows or cares.

mod common;

use common::{Outer, backends, chain_over, runtime, start, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::Standard;
use p4_service::message::{Reply, ToAgent, ToNode};
use std::sync::Arc;

/// Four nodes on two agents, created, loaded and run entirely by message.
///
/// The chain visits each agent twice, so a token's lap crosses every machine
/// twice per token. If the two nodes on one agent shared anything — a queue,
/// a lifecycle flag, a claim — the stream would interleave or stall here.
#[test]
fn two_nodes_on_each_agent_are_created_loaded_and_driven() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let one = start(Arc::new(Standard::new(backends()))).await;
        let two = start(Arc::new(Standard::new(backends()))).await;

        // Two nodes per agent. Only the last is terminal; the rest hand work
        // on, which is what makes the chain four hops rather than four
        // independent deployments.
        let placement = [
            (&one, "n0", "mock-lead"),
            (&two, "n1", "mock-lead"),
            (&one, "n2", "mock-lead"),
            (&two, "n3", "mock-tail"),
        ];
        for (index, (agent, node, adapter)) in placement.iter().enumerate() {
            agent
                .enqueue(to_agent(
                    agent,
                    &outer,
                    &format!("create-{index}"),
                    ToAgent::CreateNode {
                        node: (*node).into(),
                        adapter: (*adapter).into(),
                    },
                ))
                .unwrap();
        }
        until(|| {
            (0..placement.len()).all(|index| !seen.replies(&format!("create-{index}")).is_empty())
        })
        .await;
        for index in 0..placement.len() {
            assert!(
                matches!(
                    seen.replies(&format!("create-{index}")).first(),
                    Some(Reply::Accepted { .. })
                ),
                "node {index} was accepted: {:?}",
                seen.replies(&format!("create-{index}"))
            );
        }

        // Each node is loaded on its own, including the two sharing a machine.
        // A load addressed to one must not bind the other.
        for (index, (agent, node, _)) in placement.iter().enumerate() {
            let single = chain_over(&[(*agent, node)]);
            agent
                .enqueue(to_node(
                    &single,
                    &outer,
                    &format!("load-{index}"),
                    QueueClass::Control,
                    ToNode::Load {
                        plan: format!(r#"{{"layers":"{}-{}"}}"#, index * 10, index * 10 + 9),
                        artifact: "model.gguf".into(),
                        ceiling: 8,
                        capability_snapshot_id: String::new(),
                        capability_expires_at: 0,
                    },
                ))
                .unwrap();
        }
        until(|| {
            (0..placement.len()).all(|index| {
                seen.replies(&format!("load-{index}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Bound { .. }))
            })
        })
        .await;
        for index in 0..placement.len() {
            assert!(
                seen.replies(&format!("load-{index}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Bound { .. })),
                "node {index} bound its own share"
            );
        }

        // One inference over all four, then a second one, so the nodes are
        // reused rather than only exercised once.
        let chain = chain_over(&[(&one, "n0"), (&two, "n1"), (&one, "n2"), (&two, "n3")]);
        for round in 0..2 {
            let route = format!("infer-{round}");
            one.enqueue(to_node(
                &chain,
                &outer,
                &route,
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "여러 노드".into(),
                    max_tokens: 5,
                    options: "{}".into(),
                },
            ))
            .unwrap();
            until(|| {
                seen.replies(&route)
                    .iter()
                    .any(|reply| matches!(reply, Reply::Done { .. }))
            })
            .await;

            let stream = seen.replies(&route);
            let tokens: Vec<u32> = stream
                .iter()
                .filter_map(|reply| match reply {
                    Reply::Token { index, .. } => Some(*index),
                    _ => None,
                })
                .collect();
            assert_eq!(
                tokens,
                vec![0, 1, 2, 3, 4],
                "round {round} arrived in order: {stream:?}"
            );
            assert!(
                matches!(stream.last(), Some(Reply::Done { generated: 5, .. })),
                "round {round} ended once, counting every token: {stream:?}"
            );
        }
    });
}

/// Two chains over the same four nodes at once.
///
/// Each agent is running two nodes and each node is carrying two routes. The
/// claim is only that nothing crosses: a route's tokens are its own, and each
/// ends exactly once.
#[test]
fn nodes_shared_by_two_chains_keep_the_routes_apart() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let one = start(Arc::new(Standard::new(backends()))).await;
        let two = start(Arc::new(Standard::new(backends()))).await;

        let placement = [
            (&one, "n0", "mock-lead"),
            (&two, "n1", "mock-lead"),
            (&one, "n2", "mock-lead"),
            (&two, "n3", "mock-tail"),
        ];
        for (index, (agent, node, adapter)) in placement.iter().enumerate() {
            agent
                .enqueue(to_agent(
                    agent,
                    &outer,
                    &format!("c{index}"),
                    ToAgent::CreateNode {
                        node: (*node).into(),
                        adapter: (*adapter).into(),
                    },
                ))
                .unwrap();
        }
        until(|| (0..placement.len()).all(|index| !seen.replies(&format!("c{index}")).is_empty()))
            .await;

        for (index, (agent, node, _)) in placement.iter().enumerate() {
            let single = chain_over(&[(*agent, node)]);
            agent
                .enqueue(to_node(
                    &single,
                    &outer,
                    &format!("l{index}"),
                    QueueClass::Control,
                    ToNode::Load {
                        plan: r#"{"layers":"0-9"}"#.into(),
                        artifact: "model.gguf".into(),
                        ceiling: 8,
                        capability_snapshot_id: String::new(),
                        capability_expires_at: 0,
                    },
                ))
                .unwrap();
        }
        until(|| {
            (0..placement.len()).all(|index| {
                seen.replies(&format!("l{index}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Bound { .. }))
            })
        })
        .await;

        // The short chain stops at the first node's machine partner; the long
        // one runs all four. They share n0 and n1.
        let short = chain_over(&[(&one, "n0"), (&two, "n3")]);
        let long = chain_over(&[(&one, "n0"), (&two, "n1"), (&one, "n2"), (&two, "n3")]);
        for index in 0..8 {
            one.enqueue(to_node(
                &short,
                &outer,
                &format!("S{index}"),
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "짧은".into(),
                    max_tokens: 3,
                    options: "{}".into(),
                },
            ))
            .unwrap();
            one.enqueue(to_node(
                &long,
                &outer,
                &format!("L{index}"),
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "긴".into(),
                    max_tokens: 6,
                    options: "{}".into(),
                },
            ))
            .unwrap();
        }

        until(|| {
            (0..8).all(|index| {
                [format!("S{index}"), format!("L{index}")]
                    .iter()
                    .all(|route| {
                        seen.replies(route)
                            .iter()
                            .any(|reply| matches!(reply, Reply::Done { .. }))
                    })
            })
        })
        .await;

        for index in 0..8 {
            for (route, expected) in [(format!("S{index}"), 3u32), (format!("L{index}"), 6)] {
                let stream = seen.replies(&route);
                let tokens: Vec<u32> = stream
                    .iter()
                    .filter_map(|reply| match reply {
                        Reply::Token { index, .. } => Some(*index),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    tokens,
                    (0..expected).collect::<Vec<_>>(),
                    "{route} kept its own tokens: {stream:?}"
                );
                let terminals = stream
                    .iter()
                    .filter(|reply| matches!(reply, Reply::Done { .. }))
                    .count();
                assert_eq!(terminals, 1, "{route} ended exactly once");
            }
        }
    });
}
