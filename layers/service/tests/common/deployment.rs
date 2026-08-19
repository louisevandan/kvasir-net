//! Putting a node on an agent and loading it, in one call.
//!
//! Shared because the protocol tests are split by what they claim — what a
//! load makes visible on one side, what can be watched and steered on the
//! other — and both have to get a deployment standing first.

use super::{Outer, chain_over, to_agent, to_node, until};
use p4_protocol::QueueClass;
use p4_service::message::{ToAgent, ToNode};
use std::sync::Arc;

/// Creates a node and loads it, returning once bound.
///
/// `ceiling` is the declared concurrency. One makes a node take work strictly
/// in turn, which is what lets a test know something is still queued rather
/// than racing to observe it.
pub async fn place(
    agent: &Arc<p4_agent_core::agent::Agent>,
    outer: &Arc<p4_agent_core::agent::Agent>,
    seen: &Outer,
    node: &str,
    adapter: &str,
    plan: &str,
    ceiling: u32,
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
                ceiling,
                capability_snapshot_id: "test-snapshot".into(),
                capability_expires_at: u64::MAX,
            },
        ))
        .unwrap();
    until(|| !seen.replies(&format!("load-{node}")).is_empty()).await;
}
