//! Standing up a loaded node and holding a conversation on it.
//!
//! Shared by the cache tests, which are split by what they claim rather than
//! by what they need: the verbs on one node are one file, what a cache verb
//! does to a deployment and to a chain is another, and both start from here.

use super::{Outer, chain_over, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::message::{Reply, ToAgent, ToNode};
use std::sync::Arc;

pub type Agent = Arc<p4_agent_core::agent::Agent>;

pub async fn place(agent: &Agent, outer: &Agent, seen: &Outer, node: &str) {
    agent
        .enqueue(to_agent(
            agent,
            outer,
            "create",
            ToAgent::CreateNode {
                node: node.into(),
                adapter: "mock-tail".into(),
            },
        ))
        .unwrap();
    until(|| !seen.replies("create").is_empty()).await;

    let single = chain_over(&[(agent, node)]);
    agent
        .enqueue(to_node(
            &single,
            outer,
            "load",
            QueueClass::Control,
            ToNode::Load {
                plan: r#"{"layers":"0-19"}"#.into(),
                artifact: "model.gguf".into(),
                ceiling: 4,
                capability_snapshot_id: String::new(),
                capability_expires_at: 0,
            },
        ))
        .unwrap();
    until(|| {
        seen.replies("load")
            .iter()
            .any(|reply| matches!(reply, Reply::Bound { .. }))
    })
    .await;
}

/// Runs one inference to completion under `route`.
pub async fn infer(
    agent: &Agent,
    outer: &Agent,
    seen: &Outer,
    node: &str,
    route: &str,
    tokens: u32,
) {
    let chain = chain_over(&[(agent, node)]);
    agent
        .enqueue(to_node(
            &chain,
            outer,
            route,
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "대화".into(),
                max_tokens: tokens,
                options: "{}".into(),
            },
        ))
        .unwrap();
    until(|| {
        seen.replies(route)
            .iter()
            .any(|reply| matches!(reply, Reply::Done { .. }))
    })
    .await;
}

/// Sends one cache instruction and waits for its answer.
pub async fn cache_op(
    agent: &Agent,
    outer: &Agent,
    seen: &Outer,
    node: &str,
    route: &str,
    op: ToNode,
) {
    let chain = chain_over(&[(agent, node)]);
    agent
        .enqueue(to_node(&chain, outer, route, QueueClass::Control, op))
        .unwrap();
    until(|| !seen.replies(route).is_empty()).await;
}
