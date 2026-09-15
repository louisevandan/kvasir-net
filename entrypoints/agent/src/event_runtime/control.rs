mod inspection;

use p4_adapter::node_adapter::{
    AdapterLifecycleCompletion, CompletionMailbox, PublishError, RetainedCompletion,
    RetainedNodeAdapter, completion_mailbox_with_limits,
};
use p4_agent_core::event_broker::{NodeAdmissionPause, RetainedEventBroker};
use p4_agent_core::event_node::{RetainedEventNode, RetainedEventNodeFailure};
use p4_protocol::Address;
use p4_protocol::event::lifecycle::{
    LIFECYCLE_SCHEMA, LifecycleOperation, LifecycleRequestMetadata, LifecycleResultMetadata,
    LifecycleStatus, NODE_LIFECYCLE_RESULT_CONTENT_TYPE, NODE_LOAD_CONTENT_TYPE,
    NODE_UNLOAD_CONTENT_TYPE, ResourceState, decode_metadata, encode_metadata,
};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecyclePhase {
    Legacy,
    Loading,
    Loaded,
    Unloading,
    Failed,
}

impl LifecyclePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Loading => "loading",
            Self::Loaded => "loaded",
            Self::Unloading => "unloading",
            Self::Failed => "failed",
        }
    }
}

struct PendingLifecycle {
    operation: LifecycleOperation,
    metadata: LifecycleRequestMetadata,
    adapter_event_id: String,
    request: RetainedCompletion,
}

struct NodeOwner {
    generation: u64,
    adapter_kind: String,
    adapter: Arc<dyn RetainedNodeAdapter>,
    inbound: Arc<CompletionMailbox>,
    // A completed task retains its failure and held Events until this handle
    // is consumed/dropped. This is in-memory ownership, not restart recovery.
    task: JoinHandle<Result<(), RetainedEventNodeFailure>>,
    lifecycle_phase: LifecyclePhase,
    pending_lifecycle: Option<PendingLifecycle>,
    admission_pause: Option<NodeAdmissionPause>,
    last_lifecycle_result: Option<LifecycleResultMetadata>,
}

impl NodeOwner {
    fn lifecycle_state(&self) -> &'static str {
        self.lifecycle_phase.as_str()
    }
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

enum ControlAction {
    Deferred,
    Reply {
        input: RetainedCompletion,
        lifecycle_request: Option<RetainedCompletion>,
        output: Event,
    },
}

struct LifecycleStartFailure {
    input: RetainedCompletion,
    operation: LifecycleOperation,
    metadata: Option<LifecycleRequestMetadata>,
    resource_state: ResourceState,
    detail: String,
}

// Returned into the root-owned task handle, including unread input and nodes.
// A stopped control task does not turn accepted work into retired storage.
pub(super) struct Remainder {
    nodes: HashMap<String, NodeOwner>,
    receiver: Arc<CompletionMailbox>,
    held_input: Option<RetainedCompletion>,
    held_lifecycle_request: Option<RetainedCompletion>,
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
        let content_type = input.event().envelope.payload_content_type.clone();
        let action = match content_type.as_str() {
            NODE_LOAD_CONTENT_TYPE => {
                match begin_load(&own, &broker, &mut nodes, input, &transport, &sequence) {
                    Ok(()) => ControlAction::Deferred,
                    Err(failure) => match lifecycle_rejection_reply(
                        &own,
                        failure.input.event(),
                        &sequence,
                        failure.operation,
                        failure.metadata.as_ref(),
                        failure.resource_state,
                        &failure.detail,
                    ) {
                        Ok(output) => ControlAction::Reply {
                            input: failure.input,
                            lifecycle_request: None,
                            output,
                        },
                        Err(error) => {
                            return Remainder {
                                nodes,
                                receiver,
                                held_input: Some(failure.input),
                                held_lifecycle_request: None,
                                held_reply: None,
                                error: Some(error),
                            };
                        }
                    },
                }
            }
            NODE_UNLOAD_CONTENT_TYPE => {
                match begin_unload(&own, &broker, &mut nodes, input, &sequence) {
                    Ok(()) => ControlAction::Deferred,
                    Err(failure) => match lifecycle_rejection_reply(
                        &own,
                        failure.input.event(),
                        &sequence,
                        failure.operation,
                        failure.metadata.as_ref(),
                        failure.resource_state,
                        &failure.detail,
                    ) {
                        Ok(output) => ControlAction::Reply {
                            input: failure.input,
                            lifecycle_request: None,
                            output,
                        },
                        Err(error) => {
                            return Remainder {
                                nodes,
                                receiver,
                                held_input: Some(failure.input),
                                held_lifecycle_request: None,
                                held_reply: None,
                                error: Some(error),
                            };
                        }
                    },
                }
            }
            _ if matches!(&input.event().envelope.source, Endpoint::Node { .. }) => {
                match complete_lifecycle(&own, &broker, &mut nodes, input, &sequence) {
                    Ok((input, request, output)) => ControlAction::Reply {
                        input,
                        lifecycle_request: Some(request),
                        output,
                    },
                    Err((input, detail)) => {
                        let output = match reply(
                            &own,
                            input.event(),
                            &sequence,
                            RESULT,
                            json!({"ok":false,"detail":detail}),
                        ) {
                            Ok(output) => output,
                            Err(error) => {
                                return Remainder {
                                    nodes,
                                    receiver,
                                    held_input: Some(input),
                                    held_lifecycle_request: None,
                                    held_reply: None,
                                    error: Some(error),
                                };
                            }
                        };
                        ControlAction::Reply {
                            input,
                            lifecycle_request: None,
                            output,
                        }
                    }
                }
            }
            AGENT_INSPECT_CONTENT_TYPE => {
                let payload = inspection::snapshot(&nodes, &broker, &transport).await;
                let output = match reply(
                    &own,
                    input.event(),
                    &sequence,
                    AGENT_SNAPSHOT_CONTENT_TYPE,
                    payload,
                ) {
                    Ok(output) => output,
                    Err(error) => {
                        return Remainder {
                            nodes,
                            receiver,
                            held_input: Some(input),
                            held_lifecycle_request: None,
                            held_reply: None,
                            error: Some(error),
                        };
                    }
                };
                ControlAction::Reply {
                    input,
                    lifecycle_request: None,
                    output,
                }
            }
            _ => {
                let payload = if content_type == RECONCILE {
                    match serde_json::from_slice::<ReconcileTransport>(&input.event().payload) {
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
                    let result = match content_type.as_str() {
                        CREATE => {
                            create(&own, &broker, &mut nodes, input.event(), limits, &transport)
                        }
                        DELETE => remove(&broker, &mut nodes, input.event()).await,
                        other => Err(format!("unsupported agent control content type {other}")),
                    };
                    match result {
                        Ok(node) => json!({"ok":true,"node_id":node}),
                        Err(detail) => json!({"ok":false,"detail":detail}),
                    }
                };
                let output = match reply(&own, input.event(), &sequence, RESULT, payload) {
                    Ok(output) => output,
                    Err(error) => {
                        return Remainder {
                            nodes,
                            receiver,
                            held_input: Some(input),
                            held_lifecycle_request: None,
                            held_reply: None,
                            error: Some(error),
                        };
                    }
                };
                ControlAction::Reply {
                    input,
                    lifecycle_request: None,
                    output,
                }
            }
        };

        let ControlAction::Reply {
            input,
            lifecycle_request,
            output,
        } = action
        else {
            continue;
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
                held_lifecycle_request: lifecycle_request,
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
                held_lifecycle_request: lifecycle_request,
                held_reply: Some(PendingReply::Owned(*failure.completion)),
                error: Some(failure.error.to_string()),
            };
        }
        drop(lifecycle_request);
        drop(input);
    }
    Remainder {
        nodes,
        receiver,
        held_input: None,
        held_lifecycle_request: None,
        held_reply: None,
        error: None,
    }
}

fn begin_load(
    own: &Address,
    broker: &Arc<RetainedEventBroker>,
    nodes: &mut HashMap<String, NodeOwner>,
    input: RetainedCompletion,
    transport: &super::transport::Inspector,
    sequence: &AtomicU64,
) -> Result<(), LifecycleStartFailure> {
    let (metadata, opaque) =
        match parse_lifecycle_request(own, input.event(), LifecycleOperation::Load) {
            Ok((metadata, opaque)) => (metadata, opaque.to_vec()),
            Err(detail) => {
                return Err(LifecycleStartFailure {
                    input,
                    operation: LifecycleOperation::Load,
                    metadata: None,
                    resource_state: ResourceState::Absent,
                    detail,
                });
            }
        };
    if !super::adapters::supports(&metadata.adapter_kind) {
        return Err(start_failure(
            input,
            LifecycleOperation::Load,
            metadata,
            ResourceState::Absent,
            "unsupported adapter kind",
        ));
    }
    if let Some(owner) = nodes.get(&metadata.node_id) {
        let resource_state = match owner.lifecycle_phase {
            LifecyclePhase::Loaded | LifecyclePhase::Legacy => ResourceState::Present,
            LifecyclePhase::Loading | LifecyclePhase::Unloading | LifecyclePhase::Failed => owner
                .last_lifecycle_result
                .as_ref()
                .map_or(ResourceState::Unknown, |result| result.resource_state),
        };
        return Err(start_failure(
            input,
            LifecycleOperation::Load,
            metadata,
            resource_state,
            "node already exists",
        ));
    }
    let endpoint = Endpoint::node(
        own.clone(),
        metadata.node_id.clone(),
        metadata.node_generation,
    );
    let adapter_event = match derived_lifecycle_event(
        own,
        input.event(),
        &metadata,
        LifecycleOperation::Load,
        endpoint.clone(),
        opaque,
        sequence,
    ) {
        Ok(event) => event,
        Err(detail) => {
            return Err(start_failure(
                input,
                LifecycleOperation::Load,
                metadata,
                ResourceState::Absent,
                detail,
            ));
        }
    };
    let adapter_event_id = adapter_event.envelope.event_id.clone();
    let queue_capacity = metadata
        .queue_capacity
        .expect("validated LOAD queue capacity");
    let completion_capacity = metadata
        .completion_capacity
        .expect("validated LOAD completion capacity");
    let retained_capacity = metadata
        .retained_capacity
        .expect("validated LOAD retained capacity");
    let retained_bytes = metadata
        .retained_bytes
        .expect("validated LOAD retained bytes");
    let (sender, inbound) =
        match completion_mailbox_with_limits(queue_capacity, retained_capacity, retained_bytes) {
            Ok(mailbox) => mailbox,
            Err(error) => {
                return Err(start_failure(
                    input,
                    LifecycleOperation::Load,
                    metadata,
                    ResourceState::Absent,
                    format!("invalid node retained storage: {error:?}"),
                ));
            }
        };
    let pause = match broker.register_node_paused(
        metadata.node_id.clone(),
        metadata.node_generation,
        sender,
    ) {
        Ok(pause) => pause,
        Err(error) => {
            return Err(start_failure(
                input,
                LifecycleOperation::Load,
                metadata,
                ResourceState::Absent,
                error.to_string(),
            ));
        }
    };
    let resource_probe = super::adapters::runtime_resource_probe(broker, transport);
    let adapter = match super::adapters::create(
        &metadata.adapter_kind,
        endpoint,
        queue_capacity,
        completion_capacity,
        retained_capacity,
        retained_bytes,
        resource_probe,
    ) {
        Ok(adapter) => adapter,
        Err(error) => {
            let cleanup = broker.unregister_node(&metadata.node_id, metadata.node_generation);
            drop(pause);
            let (resource_state, detail) = match cleanup {
                Ok(true) => (ResourceState::Absent, error),
                Ok(false) => (
                    ResourceState::Unknown,
                    format!("{error}; registered route disappeared before cleanup"),
                ),
                Err(cleanup) => (
                    ResourceState::Unknown,
                    format!("{error}; route cleanup failed: {cleanup}"),
                ),
            };
            return Err(start_failure(
                input,
                LifecycleOperation::Load,
                metadata,
                resource_state,
                detail,
            ));
        }
    };
    let node = RetainedEventNode::new(adapter.clone(), Arc::clone(&inbound), Arc::clone(broker));
    let id = metadata.node_id.clone();
    let task = tokio::spawn(async move {
        let result = node.run().await;
        if let Err(failure) = &result {
            eprintln!("P4_EVENT_NODE_STOPPED node={id} error={:?}", failure.error);
        }
        result
    });
    if let Err(failure) = broker.dispatch_ingress_while_paused(&pause, adapter_event) {
        let detail = failure.error.to_string();
        let cleanup = broker.unregister_node(&metadata.node_id, metadata.node_generation);
        task.abort();
        drop(pause);
        tokio::task::spawn_blocking(move || drop((adapter, inbound, task)));
        let (resource_state, detail) = match cleanup {
            Ok(true) => (ResourceState::Absent, detail),
            Ok(false) => (
                ResourceState::Unknown,
                format!("{detail}; registered route disappeared before cleanup"),
            ),
            Err(cleanup) => (
                ResourceState::Unknown,
                format!("{detail}; route cleanup failed: {cleanup}"),
            ),
        };
        return Err(start_failure(
            input,
            LifecycleOperation::Load,
            metadata,
            resource_state,
            detail,
        ));
    }
    let previous = nodes.insert(
        metadata.node_id.clone(),
        NodeOwner {
            generation: metadata.node_generation,
            adapter_kind: metadata.adapter_kind.clone(),
            adapter,
            inbound,
            task,
            lifecycle_phase: LifecyclePhase::Loading,
            pending_lifecycle: Some(PendingLifecycle {
                operation: LifecycleOperation::Load,
                metadata,
                adapter_event_id,
                request: input,
            }),
            admission_pause: Some(pause),
            last_lifecycle_result: None,
        },
    );
    debug_assert!(
        previous.is_none(),
        "node occupancy checked before registration"
    );
    Ok(())
}

fn begin_unload(
    own: &Address,
    broker: &Arc<RetainedEventBroker>,
    nodes: &mut HashMap<String, NodeOwner>,
    input: RetainedCompletion,
    sequence: &AtomicU64,
) -> Result<(), LifecycleStartFailure> {
    let (metadata, opaque) =
        match parse_lifecycle_request(own, input.event(), LifecycleOperation::Unload) {
            Ok((metadata, opaque)) => (metadata, opaque.to_vec()),
            Err(detail) => {
                return Err(LifecycleStartFailure {
                    input,
                    operation: LifecycleOperation::Unload,
                    metadata: None,
                    resource_state: ResourceState::Unknown,
                    detail,
                });
            }
        };
    let Some(owner) = nodes.get(&metadata.node_id) else {
        return Err(start_failure(
            input,
            LifecycleOperation::Unload,
            metadata,
            ResourceState::Absent,
            "node does not exist",
        ));
    };
    if owner.generation != metadata.node_generation {
        let detail = format!(
            "node generation is stale; current={} incoming={}",
            owner.generation, metadata.node_generation
        );
        return Err(start_failure(
            input,
            LifecycleOperation::Unload,
            metadata,
            ResourceState::Present,
            detail,
        ));
    }
    if owner.adapter_kind != metadata.adapter_kind {
        return Err(start_failure(
            input,
            LifecycleOperation::Unload,
            metadata,
            ResourceState::Present,
            "adapter kind does not match the loaded node",
        ));
    }
    if owner.lifecycle_phase != LifecyclePhase::Loaded || owner.pending_lifecycle.is_some() {
        let state = owner
            .last_lifecycle_result
            .as_ref()
            .map_or(ResourceState::Unknown, |result| result.resource_state);
        return Err(start_failure(
            input,
            LifecycleOperation::Unload,
            metadata,
            state,
            format!("node is not unloadable; state={}", owner.lifecycle_state()),
        ));
    }
    let target = Endpoint::node(
        own.clone(),
        metadata.node_id.clone(),
        metadata.node_generation,
    );
    let adapter_event = match derived_lifecycle_event(
        own,
        input.event(),
        &metadata,
        LifecycleOperation::Unload,
        target,
        opaque,
        sequence,
    ) {
        Ok(event) => event,
        Err(detail) => {
            return Err(start_failure(
                input,
                LifecycleOperation::Unload,
                metadata,
                ResourceState::Present,
                detail,
            ));
        }
    };
    let adapter_event_id = adapter_event.envelope.event_id.clone();
    let pause = match broker.pause_node_admission(&metadata.node_id, metadata.node_generation) {
        Ok(pause) => pause,
        Err(error) => {
            return Err(start_failure(
                input,
                LifecycleOperation::Unload,
                metadata,
                ResourceState::Present,
                error.to_string(),
            ));
        }
    };
    if let Err(failure) = broker.dispatch_ingress_while_paused(&pause, adapter_event) {
        drop(pause);
        return Err(start_failure(
            input,
            LifecycleOperation::Unload,
            metadata,
            ResourceState::Present,
            failure.error.to_string(),
        ));
    }
    let owner = nodes
        .get_mut(&metadata.node_id)
        .expect("node cannot change while the control loop is synchronous");
    owner.lifecycle_phase = LifecyclePhase::Unloading;
    owner.pending_lifecycle = Some(PendingLifecycle {
        operation: LifecycleOperation::Unload,
        metadata,
        adapter_event_id,
        request: input,
    });
    owner.admission_pause = Some(pause);
    Ok(())
}

fn complete_lifecycle(
    own: &Address,
    broker: &Arc<RetainedEventBroker>,
    nodes: &mut HashMap<String, NodeOwner>,
    input: RetainedCompletion,
    sequence: &AtomicU64,
) -> Result<(RetainedCompletion, RetainedCompletion, Event), (RetainedCompletion, String)> {
    let (node_id, generation) = match &input.event().envelope.source {
        Endpoint::Node {
            agent,
            node,
            generation,
        } if agent == own => (node.clone(), *generation),
        _ => {
            return Err((
                input,
                "lifecycle completion source is not a local node".into(),
            ));
        }
    };
    let owner = match nodes.get(&node_id) {
        Some(owner) => owner,
        None => return Err((input, "lifecycle completion node does not exist".into())),
    };
    if owner.generation != generation {
        return Err((input, "lifecycle completion generation is stale".into()));
    }
    let pending = match owner.pending_lifecycle.as_ref() {
        Some(pending) => pending,
        None => return Err((input, "node has no pending lifecycle operation".into())),
    };
    if input.event().envelope.target != Endpoint::agent(own.clone())
        || input.event().envelope.adapter_kind.as_deref() != Some(owner.adapter_kind.as_str())
        || input.event().envelope.causation_id.as_deref() != Some(pending.adapter_event_id.as_str())
    {
        return Err((
            input,
            "lifecycle completion identity does not match the pending operation".into(),
        ));
    }
    let mut completion = match owner
        .adapter
        .decode_lifecycle_completion(pending.operation, input.event())
    {
        Ok(completion) if completion.validate().is_ok() => completion,
        Ok(_) => AdapterLifecycleCompletion {
            operation: pending.operation,
            status: LifecycleStatus::Failed,
            resource_state: ResourceState::Unknown,
            first_error: Some("adapter returned an invalid lifecycle completion".into()),
            cleanup_error: None,
        },
        Err(error) => AdapterLifecycleCompletion {
            operation: pending.operation,
            status: LifecycleStatus::Failed,
            resource_state: ResourceState::Unknown,
            first_error: Some(format!(
                "adapter lifecycle completion decode failed: {error}"
            )),
            cleanup_error: None,
        },
    };
    let needs_drain = match (
        completion.operation,
        completion.status,
        completion.resource_state,
    ) {
        (LifecycleOperation::Load, _, _) => true,
        (LifecycleOperation::Unload, LifecycleStatus::Succeeded, ResourceState::Absent) => true,
        _ => false,
    };
    if needs_drain && let Err(detail) = delivery_is_drained(owner) {
        completion.status = LifecycleStatus::Failed;
        completion.resource_state = ResourceState::Unknown;
        completion.first_error = Some(detail);
        completion.cleanup_error = None;
    }
    let remove = matches!(
        (
            completion.operation,
            completion.status,
            completion.resource_state
        ),
        (
            LifecycleOperation::Unload,
            LifecycleStatus::Succeeded,
            ResourceState::Absent
        )
    ) || matches!(
        (
            completion.operation,
            completion.status,
            completion.resource_state
        ),
        (
            LifecycleOperation::Load,
            LifecycleStatus::Rejected | LifecycleStatus::Failed,
            ResourceState::Absent
        )
    );
    let metadata = LifecycleResultMetadata {
        schema: LIFECYCLE_SCHEMA,
        node_id: pending.metadata.node_id.clone(),
        node_generation: pending.metadata.node_generation,
        adapter_kind: pending.metadata.adapter_kind.clone(),
        adapter_content_type: input.event().envelope.payload_content_type.clone(),
        operation: completion.operation,
        status: completion.status,
        resource_state: completion.resource_state,
        first_error: completion.first_error.clone(),
        cleanup_error: completion.cleanup_error.clone(),
    };
    if let Err(error) = metadata.validate() {
        return Err((input, error.to_string()));
    }
    let output = match lifecycle_reply(
        own,
        pending.request.event(),
        sequence,
        &metadata,
        &input.event().payload,
    ) {
        Ok(output) => output,
        Err(error) => return Err((input, error)),
    };
    if remove && let Err(error) = broker.unregister_node(&node_id, generation) {
        return Err((
            input,
            format!("lifecycle route removal failed before terminal result: {error}"),
        ));
    }
    let owner = nodes
        .get_mut(&node_id)
        .expect("validated lifecycle owner remains present");
    let pending = owner
        .pending_lifecycle
        .take()
        .expect("validated lifecycle operation remains pending");
    let request = pending.request;
    if remove {
        let owner = nodes
            .remove(&node_id)
            .expect("validated owner is removable");
        tokio::task::spawn_blocking(move || drop(owner));
    } else {
        owner.last_lifecycle_result = Some(metadata);
        match (
            completion.operation,
            completion.status,
            completion.resource_state,
        ) {
            (LifecycleOperation::Load, LifecycleStatus::Succeeded, ResourceState::Present) => {
                owner.lifecycle_phase = LifecyclePhase::Loaded;
                owner.admission_pause = None;
            }
            (LifecycleOperation::Unload, LifecycleStatus::Rejected, ResourceState::Present) => {
                owner.lifecycle_phase = LifecyclePhase::Loaded;
                owner.admission_pause = None;
            }
            _ => {
                owner.lifecycle_phase = LifecyclePhase::Failed;
                // Preserve the exact fence for failed or uncertain resources.
                debug_assert!(owner.admission_pause.is_some());
            }
        }
    }
    Ok((input, request, output))
}

fn delivery_is_drained(owner: &NodeOwner) -> Result<(), String> {
    if owner.task.is_finished() {
        return Err("node delivery task stopped before lifecycle cleanup".into());
    }
    if owner.inbound.storage_snapshot().retained_count != 0 {
        return Err("node input remains retained after lifecycle completion".into());
    }
    let completion = owner
        .adapter
        .completion_storage_snapshot()
        .ok_or_else(|| "adapter completion storage is not observable".to_owned())?;
    if completion.retained_count != 0 {
        return Err("adapter completion remains retained after lifecycle completion".into());
    }
    if let Some(retention) = owner.adapter.retention_snapshot()
        && (retention.pending_requests.count != 0 || retention.native_responses.count != 0)
    {
        return Err("adapter-owned request or native response remains retained".into());
    }
    Ok(())
}

fn parse_lifecycle_request<'a>(
    own: &Address,
    event: &'a Event,
    operation: LifecycleOperation,
) -> Result<(LifecycleRequestMetadata, &'a [u8]), String> {
    if event.envelope.target != Endpoint::agent(own.clone())
        || !matches!(&event.envelope.source, Endpoint::Outer(_))
        || event.envelope.class != EventClass::Control
    {
        return Err("node lifecycle request must be OUTER control targeted at this agent".into());
    }
    let (metadata, opaque): (LifecycleRequestMetadata, _) =
        decode_metadata(&event.payload).map_err(|error| error.to_string())?;
    metadata
        .validate(operation)
        .map_err(|error| error.to_string())?;
    if event
        .envelope
        .adapter_kind
        .as_deref()
        .is_some_and(|kind| kind != metadata.adapter_kind)
    {
        return Err("envelope adapter kind differs from lifecycle metadata".into());
    }
    Ok((metadata, opaque))
}

fn derived_lifecycle_event(
    own: &Address,
    base: &Event,
    metadata: &LifecycleRequestMetadata,
    operation: LifecycleOperation,
    target: Endpoint,
    payload: Vec<u8>,
    sequence: &AtomicU64,
) -> Result<Event, String> {
    let number = next_sequence(sequence)?;
    let mut envelope = base.envelope.next(
        format!("{own}:node-lifecycle:{number}"),
        Endpoint::agent(own.clone()),
        target,
        EventClass::Control,
        number,
        metadata.adapter_content_type.clone(),
    );
    envelope.adapter_kind = Some(metadata.adapter_kind.clone());
    let event = Event { envelope, payload };
    event
        .validate()
        .map_err(|error| format!("invalid derived {operation:?} lifecycle event: {error}"))?;
    Ok(event)
}

fn start_failure(
    input: RetainedCompletion,
    operation: LifecycleOperation,
    metadata: LifecycleRequestMetadata,
    resource_state: ResourceState,
    detail: impl Into<String>,
) -> LifecycleStartFailure {
    LifecycleStartFailure {
        input,
        operation,
        metadata: Some(metadata),
        resource_state,
        detail: detail.into(),
    }
}

fn lifecycle_rejection_reply(
    own: &Address,
    base: &Event,
    sequence: &AtomicU64,
    operation: LifecycleOperation,
    metadata: Option<&LifecycleRequestMetadata>,
    resource_state: ResourceState,
    detail: &str,
) -> Result<Event, String> {
    let Some(request) = metadata else {
        return reply(
            own,
            base,
            sequence,
            RESULT,
            json!({"ok":false,"detail":detail}),
        );
    };
    let result = LifecycleResultMetadata {
        schema: LIFECYCLE_SCHEMA,
        node_id: request.node_id.clone(),
        node_generation: request.node_generation,
        adapter_kind: request.adapter_kind.clone(),
        adapter_content_type: request.adapter_content_type.clone(),
        operation,
        status: LifecycleStatus::Rejected,
        resource_state,
        first_error: Some(detail.to_owned()),
        cleanup_error: None,
    };
    lifecycle_reply(own, base, sequence, &result, &[])
}

fn lifecycle_reply(
    own: &Address,
    base: &Event,
    sequence: &AtomicU64,
    metadata: &LifecycleResultMetadata,
    opaque: &[u8],
) -> Result<Event, String> {
    metadata.validate().map_err(|error| error.to_string())?;
    let payload = encode_metadata(metadata, opaque).map_err(|error| error.to_string())?;
    reply_bytes(
        own,
        base,
        sequence,
        EventClass::Control,
        NODE_LIFECYCLE_RESULT_CONTENT_TYPE,
        payload,
    )
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
            lifecycle_phase: LifecyclePhase::Legacy,
            pending_lifecycle: None,
            admission_pause: None,
            last_lifecycle_result: None,
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
    if owner.lifecycle_phase != LifecyclePhase::Legacy {
        return Err("lifecycle-managed node must be removed through NODE_UNLOAD".into());
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

fn next_sequence(sequence: &AtomicU64) -> Result<u64, String> {
    let number = sequence.fetch_add(1, Ordering::Relaxed);
    if number == u64::MAX {
        Err("agent event sequence exhausted".into())
    } else {
        Ok(number)
    }
}

fn reply(
    own: &Address,
    base: &Event,
    sequence: &AtomicU64,
    payload_content_type: &str,
    payload: serde_json::Value,
) -> Result<Event, String> {
    let payload = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    reply_bytes(
        own,
        base,
        sequence,
        EventClass::Telemetry,
        payload_content_type,
        payload,
    )
}

fn reply_bytes(
    own: &Address,
    base: &Event,
    sequence: &AtomicU64,
    class: EventClass,
    payload_content_type: &str,
    payload: Vec<u8>,
) -> Result<Event, String> {
    let context = base
        .envelope
        .return_context()
        .map_err(|error| error.to_string())?;
    let number = next_sequence(sequence)?;
    let mut envelope = context
        .reply(
            &base.envelope,
            format!("{own}:agent:{number}"),
            Endpoint::agent(own.clone()),
            class,
            number,
            payload_content_type,
        )
        .map_err(|error| error.to_string())?;
    envelope.adapter_kind = None;
    Ok(Event { envelope, payload })
}

#[cfg(test)]
mod tests;
