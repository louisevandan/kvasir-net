use super::{RunConfig, Sender, config::NodeConfig, replies, wire};
use p4_protocol::{
    Address,
    event::{
        Endpoint, Event, EventClass,
        lifecycle::{
            LIFECYCLE_SCHEMA, LifecycleOperation, LifecycleRequestMetadata,
            LifecycleResultMetadata, LifecycleStatus, NODE_LIFECYCLE_RESULT_CONTENT_TYPE,
            NODE_LOAD_CONTENT_TYPE, NODE_UNLOAD_CONTENT_TYPE, ResourceState, decode_metadata,
            encode_metadata,
        },
    },
};
use std::str::FromStr;
use tokio::io::{AsyncRead, AsyncWrite};

const ADAPTER_KIND: &str = "llamacpp";
const QUEUE_CAPACITY: usize = 65_536;
const COMPLETION_CAPACITY: usize = 65_536;
const RETAINED_CAPACITY: usize = 65_536;
// The retired CREATE path omitted this field and therefore used the agent's
// 256 MiB default. Preserve that exact aggregate store budget; adapter resource
// profiles still enforce their smaller request/completion/edge limits.
const RETAINED_BYTES: usize = 256 * 1024 * 1024;

pub(super) struct DecodedResult<'a> {
    pub metadata: LifecycleResultMetadata,
    pub opaque: &'a [u8],
}

pub(super) fn load_event(
    node: &NodeConfig,
    sender: &mut Sender,
    opaque: Vec<u8>,
) -> Result<Event, Box<dyn std::error::Error>> {
    request_event(
        node,
        sender,
        LifecycleOperation::Load,
        p4_llamacpp_staged_adapter::v2::LOAD_CONTENT_TYPE,
        opaque,
        Some(RETAINED_BYTES),
    )
}

pub(super) fn unload_event(
    node: &NodeConfig,
    sender: &mut Sender,
    opaque: Vec<u8>,
) -> Result<Event, Box<dyn std::error::Error>> {
    request_event(
        node,
        sender,
        LifecycleOperation::Unload,
        p4_llamacpp_staged_adapter::v2::UNLOAD_CONTENT_TYPE,
        opaque,
        None,
    )
}

fn request_event(
    node: &NodeConfig,
    sender: &mut Sender,
    operation: LifecycleOperation,
    adapter_content_type: &str,
    opaque: Vec<u8>,
    retained_bytes: Option<usize>,
) -> Result<Event, Box<dyn std::error::Error>> {
    let allocating = operation == LifecycleOperation::Load;
    let metadata = LifecycleRequestMetadata {
        schema: LIFECYCLE_SCHEMA,
        node_id: node.node.clone(),
        node_generation: node.generation,
        adapter_kind: ADAPTER_KIND.into(),
        adapter_content_type: adapter_content_type.into(),
        queue_capacity: allocating.then_some(QUEUE_CAPACITY),
        completion_capacity: allocating.then_some(COMPLETION_CAPACITY),
        retained_capacity: allocating.then_some(RETAINED_CAPACITY),
        retained_bytes,
    };
    metadata.validate(operation)?;
    let payload = encode_metadata(&metadata, &opaque)?;
    Ok(sender.event(
        Endpoint::agent(Address::from_str(&node.agent)?),
        EventClass::Control,
        match operation {
            LifecycleOperation::Load => NODE_LOAD_CONTENT_TYPE,
            LifecycleOperation::Unload => NODE_UNLOAD_CONTENT_TYPE,
        },
        payload,
        match operation {
            LifecycleOperation::Load => "load",
            LifecycleOperation::Unload => "unload",
        },
    ))
}

pub(super) fn decode_result(
    event: &Event,
    operation: LifecycleOperation,
) -> Result<DecodedResult<'_>, String> {
    let (metadata, opaque): (LifecycleResultMetadata, _) =
        decode_metadata(&event.payload).map_err(|error| error.to_string())?;
    metadata.validate().map_err(|error| error.to_string())?;
    if metadata.operation != operation || metadata.adapter_kind != ADAPTER_KIND {
        return Err("lifecycle result operation or adapter mismatch".into());
    }
    let expected_content_type = match operation {
        LifecycleOperation::Load => p4_llamacpp_staged_adapter::v2::LOADED_CONTENT_TYPE,
        LifecycleOperation::Unload => p4_llamacpp_staged_adapter::v2::UNLOADED_CONTENT_TYPE,
    };
    if metadata.adapter_content_type != expected_content_type {
        return Err("lifecycle result adapter content type mismatch".into());
    }
    Ok(DecodedResult { metadata, opaque })
}

pub(super) fn require_success(result: &DecodedResult<'_>, node: &NodeConfig) -> Result<(), String> {
    if result.metadata.node_id != node.node || result.metadata.node_generation != node.generation {
        return Err("lifecycle result node identity mismatch".into());
    }
    let expected_resource = match result.metadata.operation {
        LifecycleOperation::Load => ResourceState::Present,
        LifecycleOperation::Unload => ResourceState::Absent,
    };
    if result.metadata.status != LifecycleStatus::Succeeded
        || result.metadata.resource_state != expected_resource
    {
        return Err(format!(
            "node lifecycle {:?} {:?}/{:?}: first_error={} cleanup_error={}",
            result.metadata.operation,
            result.metadata.status,
            result.metadata.resource_state,
            result.metadata.first_error.as_deref().unwrap_or("none"),
            result.metadata.cleanup_error.as_deref().unwrap_or("none")
        ));
    }
    Ok(())
}

pub(super) async fn unload_indices<R, W>(
    config: &RunConfig,
    indices: impl IntoIterator<Item = usize>,
    wire: &mut wire::EventWire<R, W>,
    sender: &mut Sender,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let indices = indices.into_iter().collect::<Vec<_>>();
    let mut expected = Vec::with_capacity(indices.len());
    for &index in &indices {
        let node = &config.nodes[index];
        let opaque = serde_json::to_vec(&p4_llamacpp_staged_adapter::v2::UnloadCommand {
            load_generation: config.load_generation,
        })?;
        let event = unload_event(node, sender, opaque)?;
        expected.push(replies::ExpectedReply::from_request(&event));
        wire.send(event).await?;
    }
    let replies = replies::receive_exact(
        wire,
        NODE_LIFECYCLE_RESULT_CONTENT_TYPE,
        expected,
        "unload",
        config.timeout_ms,
    )
    .await?;
    for event in replies {
        let result = decode_result(&event, LifecycleOperation::Unload)?;
        let node = indices
            .iter()
            .map(|&index| &config.nodes[index])
            .find(|node| {
                node.node == result.metadata.node_id
                    && node.generation == result.metadata.node_generation
            })
            .ok_or("UNLOAD lifecycle result has no requested node")?;
        require_success(&result, node)?;
    }
    Ok(())
}
