mod inspection;

use p4_adapter::node_adapter::NodeAdapter;
use p4_agent_core::event_broker::{EventBroker, EventReceiver, bounded_queue};
use p4_agent_core::event_node::{EventNode, EventNodeFailure};
use p4_llamacpp_staged_adapter::v2::LlamaNodeAdapter;
use p4_protocol::Address;
use p4_protocol::event::{
    AGENT_INSPECT_CONTENT_TYPE, AGENT_SNAPSHOT_CONTENT_TYPE, Endpoint, Envelope, Event, EventClass,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::task::JoinHandle;

const CREATE: &str = "application/vnd.p4.node.create-v3+json";
const DELETE: &str = "application/vnd.p4.node.delete-v3+json";
const RESULT: &str = "application/vnd.p4.node.result-v3+json";

#[derive(Deserialize)]
struct CreateNode {
    node_id: String,
    node_generation: u64,
    adapter_kind: String,
    #[serde(default = "default_capacity")]
    queue_capacity: usize,
    #[serde(default = "default_capacity")]
    completion_capacity: usize,
}

#[derive(Deserialize)]
struct DeleteNode {
    node_id: String,
    node_generation: u64,
}

fn default_capacity() -> usize {
    65_536
}

struct NodeOwner {
    generation: u64,
    adapter_kind: String,
    adapter: Arc<dyn NodeAdapter>,
    // A completed task retains its failure and held Events until this handle
    // is consumed/dropped. This is in-memory ownership, not restart recovery.
    task: JoinHandle<Result<(), EventNodeFailure>>,
}

pub async fn run(own: Address, broker: Arc<EventBroker>, mut receiver: EventReceiver) {
    let sequence = AtomicU64::new(1);
    let mut nodes: HashMap<String, NodeOwner> = HashMap::new();
    while let Some(event) = receiver.recv().await {
        let (payload_content_type, payload) = match event.envelope.payload_content_type.as_str() {
            AGENT_INSPECT_CONTENT_TYPE => (
                AGENT_SNAPSHOT_CONTENT_TYPE,
                inspection::snapshot(&nodes).await,
            ),
            content_type => {
                let result = match content_type {
                    CREATE => create(&own, &broker, &mut nodes, &event),
                    DELETE => remove(&broker, &mut nodes, &event).await,
                    other => Err(format!("unsupported agent control content type {other}")),
                };
                let payload = match result {
                    Ok(node) => json!({"ok":true,"node_id":node}),
                    Err(detail) => json!({"ok":false,"detail":detail}),
                };
                (RESULT, payload)
            }
        };
        if let Ok(reply) = reply(&own, &event, &sequence, payload_content_type, payload)
            && let Err(failure) = broker.dispatch(reply)
        {
            // The broker returns the original reply on every refusal. This
            // current control loop still discards it after logging: a bounded
            // reply outbox and receiver acceptance are not implemented here.
            // Log only the reason, never the returned Event/payload.
            eprintln!("P4_EVENT_CONTROL_REPLY_FAILED error={}", failure.error);
        }
    }
}

fn create(
    own: &Address,
    broker: &Arc<EventBroker>,
    nodes: &mut HashMap<String, NodeOwner>,
    event: &Event,
) -> Result<String, String> {
    let command: CreateNode = serde_json::from_slice(&event.payload)
        .map_err(|error| format!("invalid node create payload: {error}"))?;
    if command.node_id.is_empty()
        || command.node_generation == 0
        || command.queue_capacity == 0
        || command.completion_capacity == 0
    {
        return Err("node id and positive queue capacities are required".into());
    }
    if nodes.contains_key(&command.node_id) {
        return Err("node already exists".into());
    }
    let endpoint = Endpoint::node(
        own.clone(),
        command.node_id.clone(),
        command.node_generation,
    );
    let adapter: Arc<dyn NodeAdapter> = match command.adapter_kind.as_str() {
        "llamacpp" => Arc::new(LlamaNodeAdapter::new(
            endpoint,
            command.queue_capacity,
            command.completion_capacity,
        )),
        other => return Err(format!("unsupported adapter kind {other}")),
    };
    let (sender, inbound) = bounded_queue(command.queue_capacity);
    broker
        .register_node(command.node_id.clone(), command.node_generation, sender)
        .map_err(|error| error.to_string())?;
    let node = EventNode::new(Arc::clone(&adapter), inbound, Arc::clone(broker));
    let id = command.node_id.clone();
    let task = tokio::spawn(async move {
        let result = node.run().await;
        if let Err(failure) = &result {
            eprintln!("P4_EVENT_NODE_STOPPED node={id} error={:?}", failure.error);
        }
        result
    });
    nodes.insert(
        command.node_id.clone(),
        NodeOwner {
            generation: command.node_generation,
            adapter_kind: command.adapter_kind,
            adapter,
            task,
        },
    );
    Ok(command.node_id)
}

async fn remove(
    broker: &Arc<EventBroker>,
    nodes: &mut HashMap<String, NodeOwner>,
    event: &Event,
) -> Result<String, String> {
    let command: DeleteNode = serde_json::from_slice(&event.payload)
        .map_err(|error| format!("invalid node delete payload: {error}"))?;
    let owner = nodes
        .get(&command.node_id)
        .ok_or_else(|| "node does not exist".to_owned())?;
    if owner.generation != command.node_generation {
        return Err(format!(
            "node generation is stale; current={} incoming={}",
            owner.generation, command.node_generation
        ));
    }
    let state = owner.adapter.snapshot();
    if !matches!(state.as_str(), "empty" | "unloaded" | "closed") {
        return Err(format!(
            "node must be unloaded before deletion; state={state}"
        ));
    }
    broker
        .unregister_node(&command.node_id, command.node_generation)
        .map_err(|error| error.to_string())?;
    let owner = nodes.remove(&command.node_id).expect("checked node exists");
    // Preserve the existing unloaded-node deletion behavior. Aborting and
    // dropping this handle can discard a retained failure/held Events; the
    // adapter snapshot is not a graceful transport drain or a durable receipt.
    owner.task.abort();
    tokio::task::spawn_blocking(move || drop(owner.adapter))
        .await
        .map_err(|error| format!("node adapter cleanup failed: {error}"))?;
    Ok(command.node_id)
}

fn reply(
    own: &Address,
    base: &Event,
    sequence: &AtomicU64,
    payload_content_type: &str,
    payload: serde_json::Value,
) -> Result<Event, String> {
    let number = sequence.fetch_add(1, Ordering::Relaxed);
    if number == u64::MAX {
        return Err("agent event sequence exhausted".into());
    }
    let target = base
        .envelope
        .return_route
        .clone()
        .map(Endpoint::Outer)
        .unwrap_or_else(|| base.envelope.source.clone());
    let envelope = Envelope {
        protocol_version: Envelope::VERSION,
        event_id: format!("{own}:agent:{number}"),
        correlation_id: base.envelope.correlation_id.clone(),
        causation_id: Some(base.envelope.event_id.clone()),
        source: Endpoint::agent(own.clone()),
        target,
        return_route: base.envelope.return_route.clone(),
        class: EventClass::Telemetry,
        sequence: number,
        deadline_unix_ms: base.envelope.deadline_unix_ms,
        adapter_kind: None,
        payload_content_type: payload_content_type.into(),
    };
    let payload = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    Ok(Event { envelope, payload })
}
