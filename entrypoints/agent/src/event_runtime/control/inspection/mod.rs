//! Read-only machine and node-registry projection for an OUTER.
//!
//! See `docs/event-protocol-v2.md#agent-inspection`.

mod hardware;

use super::NodeOwner;
use p4_protocol::event::Envelope;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: u16 = 1;
const ADAPTERS: [&str; 1] = ["llamacpp"];

pub(super) async fn snapshot(nodes: &HashMap<String, NodeOwner>) -> Value {
    let mut registered: Vec<Value> = nodes
        .iter()
        .map(|(node_id, owner)| {
            json!({
                "node_id": node_id,
                "generation": owner.generation,
                "adapter_kind": owner.adapter_kind,
                "state": owner.adapter.snapshot(),
            })
        })
        .collect();
    registered.sort_by(|left, right| {
        left["node_id"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["node_id"].as_str().unwrap_or_default())
    });
    let generated_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let hardware = tokio::task::spawn_blocking(hardware::observe)
        .await
        .unwrap_or_else(|error| hardware::failed(format!("hardware probe task failed: {error}")));

    json!({
        "schema": SCHEMA,
        "protocol_version": Envelope::VERSION,
        "generated_at_unix_ms": generated_at_unix_ms,
        "machine": hardware.with_adapters(ADAPTERS),
        "nodes": registered,
    })
}

#[cfg(test)]
mod tests;
