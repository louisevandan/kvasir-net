mod inspection;

use p4_adapter::node_adapter::{
    CompletionMailbox, PublishError, RetainedCompletion, RetainedNodeAdapter,
    completion_mailbox_with_limits,
};
use p4_agent_core::event_broker::RetainedEventBroker;
use p4_agent_core::event_node::{RetainedEventNode, RetainedEventNodeFailure};
use p4_protocol::Address;
use p4_protocol::event::{
    AGENT_INSPECT_CONTENT_TYPE, AGENT_SNAPSHOT_CONTENT_TYPE, Endpoint, Event, EventClass,
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
const RECONCILE: &str = "application/vnd.p4.transport.reconcile-v1+json";

#[derive(Deserialize)]
struct CreateNode {
    node_id: String,
    node_generation: u64,
    adapter_kind: String,
    #[serde(default = "default_capacity")]
    queue_capacity: usize,
    #[serde(default = "default_capacity")]
    completion_capacity: usize,
    retained_capacity: Option<usize>,
    retained_bytes: Option<usize>,
}

#[derive(Deserialize)]
struct DeleteNode {
    node_id: String,
    node_generation: u64,
}

#[derive(Deserialize)]
struct ReconcileTransport {
    failure_id: String,
}

fn default_capacity() -> usize {
    65_536
}

struct NodeOwner {
    generation: u64,
    adapter_kind: String,
    adapter: Arc<dyn RetainedNodeAdapter>,
    inbound: Arc<CompletionMailbox>,
    // A completed task retains its failure and held Events until this handle
    // is consumed/dropped. This is in-memory ownership, not restart recovery.
    task: JoinHandle<Result<(), RetainedEventNodeFailure>>,
}

impl Drop for NodeOwner {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum PendingReply {
    Raw(Event),
    Owned(RetainedCompletion),
}

// Returned into the root-owned task handle, including unread input and nodes.
// A stopped control task does not turn accepted work into retired storage.
pub(super) struct Remainder {
    nodes: HashMap<String, NodeOwner>,
    receiver: Arc<CompletionMailbox>,
    held_input: Option<RetainedCompletion>,
    held_reply: Option<PendingReply>,
    error: Option<String>,
}

pub(super) async fn run(
    own: Address,
    broker: Arc<RetainedEventBroker>,
    receiver: Arc<CompletionMailbox>,
    limits: super::RuntimeLimits,
    transport: super::transport::Inspector,
) -> Remainder {
    let sequence = AtomicU64::new(1);
    let mut nodes: HashMap<String, NodeOwner> = HashMap::new();
    let (replies, reply_store) = super::RuntimeLimits {
        queue: 1,
        retained: 1,
        ..limits
    }
    .mailbox();
    while let Some(input) = super::next(&receiver).await {
        let event = input.event();
        let (payload_content_type, payload) = match event.envelope.payload_content_type.as_str() {
            AGENT_INSPECT_CONTENT_TYPE => (
                AGENT_SNAPSHOT_CONTENT_TYPE,
                inspection::snapshot(&nodes, &broker, &transport).await,
            ),
            content_type => {
                let payload = if content_type == RECONCILE {
                    match serde_json::from_slice::<ReconcileTransport>(&event.payload) {
                        Ok(request) if !request.failure_id.is_empty() => {
                            transport.reconcile(&request.failure_id).await
                        }
                        Ok(_) => {
                            json!({"ok":false,"state":"rejected_local","detail":"failure_id is required"})
                        }
                        Err(error) => {
                            json!({"ok":false,"state":"rejected_local","detail":error.to_string()})
                        }
                    }
                } else {
                    let result = match content_type {
                        CREATE => create(&own, &broker, &mut nodes, event, limits, &transport),
                        DELETE => remove(&broker, &mut nodes, event).await,
                        other => Err(format!("unsupported agent control content type {other}")),
                    };
                    match result {
                        Ok(node) => json!({"ok":true,"node_id":node}),
                        Err(detail) => json!({"ok":false,"detail":detail}),
                    }
                };
                (RESULT, payload)
            }
        };
        let output = match reply(&own, event, &sequence, payload_content_type, payload) {
            Ok(reply) => reply,
            Err(error) => {
                return Remainder {
                    nodes,
                    receiver,
                    held_input: Some(input),
                    held_reply: None,
                    error: Some(error),
                };
            }
        };
        if let Err(failure) = replies.try_publish_owned(output) {
            let (reason, output) = match failure {
                PublishError::Full(event) => ("reply storage Full", event),
                PublishError::Closed(event) => ("reply storage Closed", event),
                PublishError::TooLarge { event, .. } => ("reply exceeds retained bytes", event),
                PublishError::CostOverflow(event) => ("reply cost overflow", event),
            };
            eprintln!("P4_EVENT_CONTROL_REPLY_FAILED error={reason}");
            return Remainder {
                nodes,
                receiver,
                held_input: Some(input),
                held_reply: Some(PendingReply::Raw(output)),
                error: Some(reason.into()),
            };
        }
        let output = super::next(&reply_store)
            .await
            .expect("single control reply owner");
        if let Err(failure) = super::dispatch(&broker, output).await {
            eprintln!("P4_EVENT_CONTROL_REPLY_FAILED error={}", failure.error);
            return Remainder {
                nodes,
                receiver,
                held_input: Some(input),
                held_reply: Some(PendingReply::Owned(*failure.completion)),
                error: Some(failure.error.to_string()),
            };
        }
    }
    Remainder {
        nodes,
        receiver,
        held_input: None,
        held_reply: None,
        error: None,
    }
}

fn create(
    own: &Address,
    broker: &Arc<RetainedEventBroker>,
    nodes: &mut HashMap<String, NodeOwner>,
    event: &Event,
    limits: super::RuntimeLimits,
    transport: &super::transport::Inspector,
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
    let retained_capacity = command.retained_capacity.unwrap_or(limits.retained);
    let retained_bytes = command.retained_bytes.unwrap_or(limits.bytes);
    let (sender, inbound) =
        completion_mailbox_with_limits(command.queue_capacity, retained_capacity, retained_bytes)
            .map_err(|error| format!("invalid node retained storage: {error:?}"))?;
    let resource_probe = super::adapters::runtime_resource_probe(broker, transport);
    let adapter = super::adapters::create(
        &command.adapter_kind,
        endpoint,
        command.queue_capacity,
        command.completion_capacity,
        retained_capacity,
        retained_bytes,
        resource_probe,
    )?;
    broker
        .register_node(command.node_id.clone(), command.node_generation, sender)
        .map_err(|error| error.to_string())?;
    let node = RetainedEventNode::new(adapter.clone(), Arc::clone(&inbound), Arc::clone(broker));
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
            inbound,
            task,
        },
    );
    Ok(command.node_id)
}

async fn remove(
    broker: &Arc<RetainedEventBroker>,
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
    let _admission = broker
        .pause_node_admission(&command.node_id, command.node_generation)
        .map_err(|error| error.to_string())?;
    let state = owner.adapter.snapshot();
    if !matches!(state.as_str(), "empty" | "unloaded" | "closed") {
        return Err(format!(
            "node must be unloaded before deletion; state={state}"
        ));
    }
    if owner.task.is_finished()
        || owner.inbound.storage_snapshot().retained_count != 0
        || owner
            .adapter
            .completion_storage_snapshot()
            .is_none_or(|value| value.retained_count != 0)
    {
        return Err("node delivery must be drained and healthy before deletion".into());
    }
    broker
        .unregister_node(&command.node_id, command.node_generation)
        .map_err(|error| error.to_string())?;
    let owner = nodes.remove(&command.node_id).expect("checked node exists");
    // Ingress is fenced and queued/held input/output counts are zero. Aborting
    // this idle bridge cannot discard an Event. Failed owners are not deleted.
    owner.task.abort();
    tokio::task::spawn_blocking(move || drop(owner))
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
    let context = base
        .envelope
        .return_context()
        .map_err(|error| error.to_string())?;
    let number = sequence.fetch_add(1, Ordering::Relaxed);
    if number == u64::MAX {
        return Err("agent event sequence exhausted".into());
    }
    let mut envelope = context
        .reply(
            &base.envelope,
            format!("{own}:agent:{number}"),
            Endpoint::agent(own.clone()),
            EventClass::Telemetry,
            number,
            payload_content_type,
        )
        .map_err(|error| error.to_string())?;
    envelope.adapter_kind = None;
    let payload = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    Ok(Event { envelope, payload })
}

#[cfg(test)]
mod tests;
