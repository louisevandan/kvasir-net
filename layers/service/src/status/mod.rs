//! What the agent is doing, written for a caller rather than a log.
//!
//! The counters were only ever printed to stdout, which makes them a thing you
//! read while sitting at the machine. OUTER is not sitting at the machine, and
//! "where has my request got to" is a question it has to be able to ask over
//! the same socket as everything else.
//!
//! Two halves. The traffic and lane numbers say what the agent is carrying;
//! the node lines say which requests are on which node right now. A count
//! answers how many, never which, and the route that has gone missing is
//! exactly the one a count cannot show.

use p4_agent_core::agent::Agent;
use std::sync::Arc;

/// Reads the agent and formats it. Takes the node lock, so callers run it off
/// the worker path.
pub async fn snapshot(agent: &Arc<Agent>) -> String {
    let traffic = agent.traffic();
    let lanes = agent.queue().depth();
    let mut out = format!(
        "address={} forwarded={} consumed={} to_nodes={} unrouted={} \
         control={} prefill={} decode={} response={} peers={} waiting={}",
        agent.address(),
        traffic.forwarded,
        traffic.consumed,
        traffic.to_nodes,
        traffic.unrouted,
        lanes.control,
        lanes.prefill,
        lanes.decode,
        lanes.response,
        agent.peers().connected().await,
        agent.continuations().outstanding(),
    );
    for node in agent.node_status().await {
        out.push_str(&format!(
            "\nnode={} depth={} running={} routes=[{}]",
            escape(&node.node),
            node.depth,
            node.running,
            node.waiting
                .iter()
                .map(|route| escape(route))
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    out
}

/// Node ids and routes come from a caller, so they cannot be trusted to keep
/// the snapshot readable. A newline in one would make a node look like two.
fn escape(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' | '\r' | ',' | '[' | ']' => '_',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests;
