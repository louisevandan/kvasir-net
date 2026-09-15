//! Read-only machine and node-registry projection for an OUTER.
//!
//! See `docs/event-protocol-v2.md#agent-inspection`.

mod hardware;

use super::NodeOwner;
use p4_agent_core::event_broker::{ReceiptStorageSnapshot, RetainedEventBroker};
use p4_protocol::event::Envelope;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: u16 = 1;

pub(super) async fn snapshot(
    nodes: &HashMap<String, NodeOwner>,
    broker: &RetainedEventBroker,
    transport: &super::super::transport::Inspector,
) -> Value {
    let mut registered: Vec<Value> = nodes
        .iter()
        .map(|(node_id, owner)| {
            let completion = owner.adapter.completion_storage_snapshot();
            let retention = owner.adapter.retention_snapshot();
            json!({
                "node_id": node_id,
                "generation": owner.generation,
                "adapter_kind": owner.adapter_kind,
                "lifecycle_state": owner.lifecycle_state(),
                "lifecycle_result": owner.last_lifecycle_result.as_ref(),
                "state": owner.adapter.snapshot(),
                "delivery": {"stopped":owner.task.is_finished(),
                    "input_retained":owner.inbound.storage_snapshot().retained_count,
                    "completion_retained":completion.map(|value| value.retained_count)},
                "retention":{
                    "pending_requests":retention.map(|value| value.pending_requests),
                    "completions":completion.map(completion_storage),
                    "native_responses":retention.map(|value| value.native_responses),
                },
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
    // Captured before this INSPECT's reply is dispatched. The input INSPECT
    // itself has already entered the broker's ordinary duplicate window.
    let broker = receipt_snapshot(broker, generated_at_unix_ms);
    let hardware = tokio::task::spawn_blocking(hardware::observe)
        .await
        .unwrap_or_else(|error| hardware::failed(format!("hardware probe task failed: {error}")));

    json!({
        "schema": SCHEMA,
        "protocol_version": Envelope::VERSION,
        "generated_at_unix_ms": generated_at_unix_ms,
        "machine": hardware.with_adapters(super::super::adapters::kinds()),
        "nodes": registered,
        "broker": broker,
        "transport": transport.snapshot(),
    })
}

fn completion_storage(value: p4_adapter::node_adapter::CompletionStorageSnapshot) -> Value {
    json!({
        "count_limit":value.capacity,
        "byte_limit":value.byte_limit,
        "retained_count":value.retained_count,
        "retained_bytes":value.retained_bytes,
        "queued_count":value.queued_count,
        "reserved_queue_slots":value.reserved_queue_slots,
        "queue_backing_bytes":value.queue_backing_bytes,
        "closed":value.closed,
    })
}

fn storage(value: ReceiptStorageSnapshot) -> Value {
    json!({"events":value.events, "event_bytes":value.event_bytes,
        "payload_capacity_bytes":value.payload_capacity_bytes,
        "unmeasured_events":value.unmeasured_events})
}

fn receipt_snapshot(broker: &RetainedEventBroker, sampled_at: u64) -> Value {
    match broker.receipt_snapshot() {
        Ok(value) => json!({
            "sampled_at_unix_ms":sampled_at, "state":"ok",
            "receipts":{
                "duplicate_window":value.duplicate_window,
                "indexed":storage(value.indexed), "retired":storage(value.retired),
                "allocated":storage(value.allocated),
                "peak_allocated_event_bytes":value.peak_allocated_event_bytes,
                "committed_events":value.committed_events, "evicted_events":value.evicted_events,
                "freed_events":value.freed_events,
                "event_index_capacity":value.event_index_capacity, "order_capacity":value.order_capacity,
                "sequence_entries":value.sequence_entries, "sequence_capacity":value.sequence_capacity,
            },
        }),
        Err(error) => json!({"sampled_at_unix_ms":sampled_at, "state":"failed",
            "receipts":null, "detail":error.to_string()}),
    }
}

#[cfg(test)]
mod tests;
