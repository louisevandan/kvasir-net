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

use p4_adapter::Phase;
use p4_agent_core::agent::Agent;
use p4_protocol::QueueClass;
use std::sync::Arc;

/// Status snapshots currently have one legacy layout and one current layout.
/// A peer must not silently interpret a future layout as schema 2.
pub const MIN_SUPPORTED_SCHEMA: u16 = 1;
pub const MAX_SUPPORTED_SCHEMA: u16 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub schema: u16,
    pub snapshot_seq: u64,
    pub generated_at_unix_ms: u64,
    pub address: String,
    pub traffic: TrafficSnapshot,
    pub lanes: LaneSnapshot,
    pub peers: usize,
    pub continuations: usize,
    pub subscription_pending: usize,
    pub subscription_unacked: usize,
    pub subscription_dropped: usize,
    /// Aggregate ACK rejections in this process, including stale generation
    /// and body/envelope channel mismatches. This is not request-level trace.
    pub subscription_ack_rejected: usize,
    pub nodes: Vec<NodeSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrafficSnapshot {
    pub forwarded: usize,
    pub consumed: usize,
    pub to_nodes: usize,
    pub unrouted: usize,
    pub refused: usize,
    pub emergency_lost: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneSnapshot {
    pub control: usize,
    pub prefill: usize,
    pub decode: usize,
    pub response: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeSnapshot {
    pub node: String,
    pub depth: usize,
    pub running: usize,
    pub outbox_lost: usize,
    pub waiting: Vec<String>,
    pub backend: String,
    pub waiting_requests: Vec<RequestSnapshot>,
    pub active_hop: Option<ActiveHopSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestSnapshot {
    pub route: String,
    pub request_id: String,
    pub stream_id: String,
    pub lane: QueueClass,
    pub deadline_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveHopSnapshot {
    pub id: u64,
    pub phase: Phase,
    pub timed_out: bool,
    pub requests: Vec<RequestSnapshot>,
}

impl StatusSnapshot {
    /// Compatibility view for older assertions and human-facing callers.
    /// New protocol consumers should inspect the typed fields directly.
    pub fn contains(&self, needle: &str) -> bool {
        self.legacy_text().contains(needle)
    }

    fn legacy_text(&self) -> String {
        let mut out = format!(
            "address={} forwarded={} consumed={} to_nodes={} unrouted={} refused={} emergency_lost={} control={} prefill={} decode={} response={} peers={} waiting={} ack_rejected={}",
            self.address,
            self.traffic.forwarded,
            self.traffic.consumed,
            self.traffic.to_nodes,
            self.traffic.unrouted,
            self.traffic.refused,
            self.traffic.emergency_lost,
            self.lanes.control,
            self.lanes.prefill,
            self.lanes.decode,
            self.lanes.response,
            self.peers,
            self.continuations,
            self.subscription_ack_rejected,
        );
        for node in &self.nodes {
            out.push_str(&format!(
                "\nnode={} depth={} running={} routes=[{}] backend=[{}]",
                escape(&node.node),
                node.depth,
                node.running,
                node.waiting
                    .iter()
                    .map(|route| escape(route))
                    .collect::<Vec<_>>()
                    .join(","),
                escape(&node.backend),
            ));
        }
        out
    }
}

impl std::fmt::Display for StatusSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.legacy_text())
    }
}

/// The machine-readable status contract. The legacy text formatter below is
/// retained for old callers; new callers must use this correlated snapshot.
pub async fn typed_snapshot(agent: &Arc<Agent>) -> StatusSnapshot {
    let traffic = agent.traffic();
    let lanes = agent.queue().depth();
    let subscriptions = agent.subscription_metrics().await;
    let nodes = agent
        .node_status()
        .await
        .into_iter()
        .map(|node| NodeSnapshot {
            node: node.node,
            depth: node.depth,
            running: node.running,
            outbox_lost: node.outbox_lost,
            waiting: node.waiting,
            backend: node.backend,
            waiting_requests: node
                .waiting_requests
                .into_iter()
                .map(|request| RequestSnapshot {
                    route: request.route,
                    request_id: request.request_id,
                    stream_id: request.stream_id,
                    lane: request.lane,
                    deadline_unix_ms: request.deadline_unix_ms,
                })
                .collect(),
            active_hop: node.active_hop.map(|hop| ActiveHopSnapshot {
                id: hop.id,
                phase: hop.phase,
                timed_out: hop.timed_out,
                requests: hop
                    .requests
                    .into_iter()
                    .map(|request| RequestSnapshot {
                        route: request.route,
                        request_id: request.request_id,
                        stream_id: request.stream_id,
                        lane: request.lane,
                        deadline_unix_ms: request.deadline_unix_ms,
                    })
                    .collect(),
            }),
        })
        .collect();
    StatusSnapshot {
        schema: 6,
        snapshot_seq: agent.next_status_sequence(),
        generated_at_unix_ms: now_unix_ms(),
        address: agent.address().to_string(),
        traffic: TrafficSnapshot {
            forwarded: traffic.forwarded,
            consumed: traffic.consumed,
            to_nodes: traffic.to_nodes,
            unrouted: traffic.unrouted,
            refused: traffic.refused,
            emergency_lost: traffic.emergency_lost,
        },
        lanes: LaneSnapshot {
            control: lanes.control,
            prefill: lanes.prefill,
            decode: lanes.decode,
            response: lanes.response,
        },
        peers: agent.peers().connected().await,
        continuations: agent.continuations().outstanding(),
        subscription_pending: subscriptions.pending,
        subscription_unacked: subscriptions.unacked,
        subscription_dropped: subscriptions.dropped,
        subscription_ack_rejected: agent.ack_rejected(),
        nodes,
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Reads the agent and formats it. Takes the node lock, so callers run it off
/// the worker path.
pub async fn snapshot(agent: &Arc<Agent>) -> String {
    let traffic = agent.traffic();
    let lanes = agent.queue().depth();
    let mut out = format!(
        "address={} forwarded={} consumed={} to_nodes={} unrouted={} refused={} emergency_lost={} \
         control={} prefill={} decode={} response={} peers={} waiting={}",
        agent.address(),
        traffic.forwarded,
        traffic.consumed,
        traffic.to_nodes,
        traffic.unrouted,
        traffic.refused,
        traffic.emergency_lost,
        lanes.control,
        lanes.prefill,
        lanes.decode,
        lanes.response,
        agent.peers().connected().await,
        agent.continuations().outstanding(),
    );
    for node in agent.node_status().await {
        out.push_str(&format!(
            "\nnode={} depth={} running={} routes=[{}] backend=[{}]",
            escape(&node.node),
            node.depth,
            node.running,
            node.waiting
                .iter()
                .map(|route| escape(route))
                .collect::<Vec<_>>()
                .join(","),
            // The backend in its own words, escaped like anything else that
            // came from outside. Read by whoever asked and by nothing here.
            escape(&node.backend),
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
