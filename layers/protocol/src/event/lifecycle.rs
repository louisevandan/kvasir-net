//! Backend-neutral node lifecycle payload framing.

use crate::ProtocolError;
use serde::{Deserialize, Serialize};

pub const NODE_LOAD_CONTENT_TYPE: &str = "application/vnd.p4.node.load-v1";
pub const NODE_UNLOAD_CONTENT_TYPE: &str = "application/vnd.p4.node.unload-v1";
pub const NODE_LIFECYCLE_RESULT_CONTENT_TYPE: &str = "application/vnd.p4.node.lifecycle-result-v1";
pub const LIFECYCLE_SCHEMA: u16 = 1;
pub const MAX_LIFECYCLE_METADATA_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleOperation {
    Load,
    Unload,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStatus {
    Succeeded,
    Rejected,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceState {
    Absent,
    Present,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleRequestMetadata {
    pub schema: u16,
    pub node_id: String,
    pub node_generation: u64,
    pub adapter_kind: String,
    pub adapter_content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_capacity: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_capacity: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_capacity: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_bytes: Option<usize>,
}

impl LifecycleRequestMetadata {
    pub fn validate(&self, operation: LifecycleOperation) -> Result<(), ProtocolError> {
        validate_identity(
            self.schema,
            &self.node_id,
            self.node_generation,
            &self.adapter_kind,
            &self.adapter_content_type,
        )?;
        let capacities = [
            self.queue_capacity,
            self.completion_capacity,
            self.retained_capacity,
            self.retained_bytes,
        ];
        match operation {
            LifecycleOperation::Load
                if capacities
                    .into_iter()
                    .any(|value| !matches!(value, Some(1..))) =>
            {
                Err(ProtocolError::new(
                    "node LOAD requires positive queue, completion and retained capacities",
                ))
            }
            LifecycleOperation::Unload if capacities.into_iter().any(|value| value.is_some()) => {
                Err(ProtocolError::new(
                    "node UNLOAD must not carry allocation capacities",
                ))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleResultMetadata {
    pub schema: u16,
    pub node_id: String,
    pub node_generation: u64,
    pub adapter_kind: String,
    pub adapter_content_type: String,
    pub operation: LifecycleOperation,
    pub status: LifecycleStatus,
    pub resource_state: ResourceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_error: Option<String>,
}

impl LifecycleResultMetadata {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_identity(
            self.schema,
            &self.node_id,
            self.node_generation,
            &self.adapter_kind,
            &self.adapter_content_type,
        )?;
        if self.status == LifecycleStatus::Succeeded
            && (self.first_error.is_some() || self.cleanup_error.is_some())
        {
            return Err(ProtocolError::new(
                "successful lifecycle result cannot contain an error",
            ));
        }
        if self.status != LifecycleStatus::Succeeded
            && self.first_error.as_deref().is_none_or(str::is_empty)
        {
            return Err(ProtocolError::new(
                "rejected or failed lifecycle result requires first_error",
            ));
        }
        Ok(())
    }
}

fn validate_identity(
    schema: u16,
    node_id: &str,
    node_generation: u64,
    adapter_kind: &str,
    adapter_content_type: &str,
) -> Result<(), ProtocolError> {
    if schema != LIFECYCLE_SCHEMA {
        return Err(ProtocolError::new(format!(
            "node lifecycle schema {schema} is not {LIFECYCLE_SCHEMA}"
        )));
    }
    if node_id.is_empty()
        || node_generation == 0
        || adapter_kind.is_empty()
        || adapter_content_type.is_empty()
    {
        return Err(ProtocolError::new(
            "node lifecycle requires node, generation, adapter and adapter content type",
        ));
    }
    Ok(())
}

pub fn encode_metadata<T: Serialize>(
    metadata: &T,
    opaque: &[u8],
) -> Result<Vec<u8>, ProtocolError> {
    let json = serde_json::to_vec(metadata)
        .map_err(|error| ProtocolError::new(format!("invalid lifecycle metadata: {error}")))?;
    if json.len() > MAX_LIFECYCLE_METADATA_BYTES {
        return Err(ProtocolError::new("node lifecycle metadata is too large"));
    }
    let length = u32::try_from(json.len())
        .map_err(|_| ProtocolError::new("node lifecycle metadata length overflow"))?;
    let capacity = 4usize
        .checked_add(json.len())
        .and_then(|value| value.checked_add(opaque.len()))
        .ok_or_else(|| ProtocolError::new("node lifecycle payload length overflow"))?;
    let mut payload = Vec::with_capacity(capacity);
    payload.extend_from_slice(&length.to_le_bytes());
    payload.extend_from_slice(&json);
    payload.extend_from_slice(opaque);
    Ok(payload)
}

pub fn decode_metadata<T: for<'de> Deserialize<'de>>(
    payload: &[u8],
) -> Result<(T, &[u8]), ProtocolError> {
    let prefix: [u8; 4] = payload
        .get(..4)
        .ok_or_else(|| ProtocolError::new("node lifecycle metadata length is incomplete"))?
        .try_into()
        .expect("exact prefix length");
    let length = u32::from_le_bytes(prefix) as usize;
    if length > MAX_LIFECYCLE_METADATA_BYTES {
        return Err(ProtocolError::new("node lifecycle metadata is too large"));
    }
    let end = 4usize
        .checked_add(length)
        .ok_or_else(|| ProtocolError::new("node lifecycle metadata length overflow"))?;
    let json = payload
        .get(4..end)
        .ok_or_else(|| ProtocolError::new("node lifecycle metadata is truncated"))?;
    let metadata = serde_json::from_slice(json)
        .map_err(|error| ProtocolError::new(format!("invalid lifecycle metadata: {error}")))?;
    Ok((metadata, &payload[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> LifecycleRequestMetadata {
        LifecycleRequestMetadata {
            schema: 1,
            node_id: "n0".into(),
            node_generation: 7,
            adapter_kind: "neutral".into(),
            adapter_content_type: "application/x-neutral-load".into(),
            queue_capacity: Some(1),
            completion_capacity: Some(2),
            retained_capacity: Some(3),
            retained_bytes: Some(4096),
        }
    }

    #[test]
    fn node_load_lifecycle_codec_preserves_literal_prefix_and_opaque_bytes() {
        let metadata = load();
        metadata.validate(LifecycleOperation::Load).unwrap();
        let payload = encode_metadata(&metadata, b"\0opaque\xff").unwrap();
        let length = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
        assert_eq!(length, payload.len() - 4 - 8);
        let (decoded, opaque): (LifecycleRequestMetadata, _) = decode_metadata(&payload).unwrap();
        assert_eq!(decoded, metadata);
        assert_eq!(opaque, b"\0opaque\xff");
    }

    #[test]
    fn node_load_lifecycle_codec_accepts_python_canonical_fixture() {
        let json = br#"{"adapter_content_type":"application/vnd.p4.hf.command-v2","adapter_kind":"hf-transformers","completion_capacity":1,"node_generation":7,"node_id":"n0","queue_capacity":1,"retained_bytes":4096,"retained_capacity":2,"schema":1}"#;
        let mut payload = (json.len() as u32).to_le_bytes().to_vec();
        payload.extend_from_slice(json);
        payload.extend_from_slice(b"python-opaque");
        let (metadata, opaque): (LifecycleRequestMetadata, _) = decode_metadata(&payload).unwrap();
        metadata.validate(LifecycleOperation::Load).unwrap();
        assert_eq!(metadata.adapter_kind, "hf-transformers");
        assert_eq!(opaque, b"python-opaque");
    }

    #[test]
    fn node_load_lifecycle_codec_rejects_length_and_schema_before_use() {
        let mut truncated = (9u32).to_le_bytes().to_vec();
        truncated.extend_from_slice(b"{}");
        assert!(decode_metadata::<LifecycleRequestMetadata>(&truncated).is_err());
        let mut metadata = load();
        metadata.schema = 2;
        assert!(metadata.validate(LifecycleOperation::Load).is_err());
        metadata.schema = 1;
        metadata.retained_bytes = Some(0);
        assert!(metadata.validate(LifecycleOperation::Load).is_err());
        assert!(metadata.validate(LifecycleOperation::Unload).is_err());
    }

    #[test]
    fn node_load_lifecycle_metadata_limit_is_exact() {
        let exact_json_string = "x".repeat(MAX_LIFECYCLE_METADATA_BYTES - 2);
        let payload = encode_metadata(&exact_json_string, b"").unwrap();
        assert_eq!(
            u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize,
            MAX_LIFECYCLE_METADATA_BYTES
        );
        let over_json_string = "x".repeat(MAX_LIFECYCLE_METADATA_BYTES - 1);
        assert!(encode_metadata(&over_json_string, b"").is_err());

        let mut declared_over = ((MAX_LIFECYCLE_METADATA_BYTES + 1) as u32)
            .to_le_bytes()
            .to_vec();
        declared_over.extend(std::iter::repeat_n(b' ', MAX_LIFECYCLE_METADATA_BYTES + 1));
        assert!(decode_metadata::<LifecycleRequestMetadata>(&declared_over).is_err());
    }

    #[test]
    fn node_load_lifecycle_result_rejects_false_success_and_unexplained_failure() {
        let mut result = LifecycleResultMetadata {
            schema: 1,
            node_id: "n0".into(),
            node_generation: 7,
            adapter_kind: "neutral".into(),
            adapter_content_type: "application/x-neutral-result".into(),
            operation: LifecycleOperation::Load,
            status: LifecycleStatus::Succeeded,
            resource_state: ResourceState::Present,
            first_error: Some("hidden".into()),
            cleanup_error: None,
        };
        assert!(result.validate().is_err());
        result.status = LifecycleStatus::Failed;
        result.first_error = None;
        assert!(result.validate().is_err());
        result.first_error = Some("load failed".into());
        assert!(result.validate().is_ok());
    }
}
