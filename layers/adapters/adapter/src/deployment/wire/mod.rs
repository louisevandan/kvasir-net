//! The JSON encoding a P4 llama client and the llama TypeScript server must
//! produce and accept identically.
//!
//! Hand-written on both sides, the same way every parser in
//! `packages/llama_domain/src/common/protocol` validates one field at a time
//! rather than trusting a schema to reject what a fixture says must fail --
//! see `parse.ts` in
//! `packages/llama_domain/src/common/protocol/pipeline-submission/` for the
//! TypeScript twin this must agree with, and `fixtures.json` next to this
//! file for the canonical shapes both sides accept and the malformed ones
//! both must reject. The two fixture files are hand-kept in sync; nothing in
//! this checkpoint generates one from the other.

use serde_json::{Value, json};

use super::command::{Cancel, Submit};
use super::event::{
    Accepted, DeploymentEvent, Produced, Rejected, RejectedReason, Settled, SettledReason,
};

/// The wire discriminant every command and event carries, matching the
/// TypeScript twin's `PIPELINE_SUBMISSION_PROTOCOL`.
pub const PROTOCOL: &str = "linker-pipeline-submission-v1";

const MAX_ID_LEN: usize = 256;

pub fn encode_submit(command: &Submit) -> Value {
    json!({
        "protocol": PROTOCOL,
        "type": "submit",
        "deployment_id": command.deployment_id,
        "deployment_generation": command.deployment_generation,
        "submission_id": command.submission_id,
        "request": command.request,
    })
}

pub fn encode_cancel(command: &Cancel) -> Value {
    json!({
        "protocol": PROTOCOL,
        "type": "cancel",
        "submission_id": command.submission_id,
    })
}

pub fn encode_event(event: &DeploymentEvent) -> Value {
    match event {
        DeploymentEvent::Accepted(event) => json!({
            "protocol": PROTOCOL,
            "type": "accepted",
            "submission_id": event.submission_id,
        }),
        DeploymentEvent::Rejected(event) => json!({
            "protocol": PROTOCOL,
            "type": "rejected",
            "submission_id": event.submission_id,
            "reason": rejected_reason_str(event.reason),
        }),
        DeploymentEvent::Produced(event) => json!({
            "protocol": PROTOCOL,
            "type": "produced",
            "submission_id": event.submission_id,
            "event_ordinal": event.event_ordinal,
            "text": event.text,
            "generated_tokens": event.generated_tokens,
        }),
        DeploymentEvent::Settled(event) => json!({
            "protocol": PROTOCOL,
            "type": "settled",
            "submission_id": event.submission_id,
            "reason": settled_reason_str(event.reason),
            "generated_tokens": event.generated_tokens,
        }),
    }
}

/// Parses a `Submit`. Rejects anything not matching every required field's
/// shape -- see the malformed fixtures next to this file for what must fail.
pub fn parse_submit(value: &Value) -> Result<Submit, String> {
    let object = require_object(value)?;
    require_protocol(object)?;
    require_type(object, "submit")?;
    let request = object.get("request").cloned().ok_or("missing request")?;
    Ok(Submit {
        deployment_id: require_id(object, "deployment_id")?,
        deployment_generation: require_u64(object, "deployment_generation")?,
        submission_id: require_id(object, "submission_id")?,
        request,
    })
}

pub fn parse_cancel(value: &Value) -> Result<Cancel, String> {
    let object = require_object(value)?;
    require_protocol(object)?;
    require_type(object, "cancel")?;
    Ok(Cancel {
        submission_id: require_id(object, "submission_id")?,
    })
}

/// Parses one event. The `type` field selects the variant; every other field
/// required by that variant must be present and well-shaped or this fails --
/// see the malformed fixtures next to this file.
pub fn parse_event(value: &Value) -> Result<DeploymentEvent, String> {
    let object = require_object(value)?;
    require_protocol(object)?;
    match object.get("type").and_then(Value::as_str) {
        Some("accepted") => Ok(DeploymentEvent::Accepted(Accepted {
            submission_id: require_id(object, "submission_id")?,
        })),
        Some("rejected") => Ok(DeploymentEvent::Rejected(Rejected {
            submission_id: require_id(object, "submission_id")?,
            reason: parse_rejected_reason(object.get("reason"))?,
        })),
        Some("produced") => Ok(DeploymentEvent::Produced(Produced {
            submission_id: require_id(object, "submission_id")?,
            event_ordinal: require_u64(object, "event_ordinal")?,
            text: require_string(object, "text")?,
            generated_tokens: require_u32(object, "generated_tokens")?,
        })),
        Some("settled") => Ok(DeploymentEvent::Settled(Settled {
            submission_id: require_id(object, "submission_id")?,
            reason: parse_settled_reason(object.get("reason"))?,
            generated_tokens: require_u32(object, "generated_tokens")?,
        })),
        other => Err(format!("unrecognised event type: {other:?}")),
    }
}

fn rejected_reason_str(reason: RejectedReason) -> &'static str {
    match reason {
        RejectedReason::Full => "full",
        RejectedReason::Conflict => "conflict",
        RejectedReason::Invalid => "invalid",
        RejectedReason::DeploymentClosed => "deployment_closed",
    }
}

fn settled_reason_str(reason: SettledReason) -> &'static str {
    match reason {
        SettledReason::Stop => "stop",
        SettledReason::Length => "length",
        SettledReason::Canceled => "canceled",
        SettledReason::Error => "error",
    }
}

/// The one place a `RejectedReason` is recovered from wire text. Every other
/// consumer of `RejectedReason` matches the enum this returns -- see
/// `RejectedReason`'s own doc for why nothing downstream may search the raw
/// string instead.
fn parse_rejected_reason(value: Option<&Value>) -> Result<RejectedReason, String> {
    match value.and_then(Value::as_str) {
        Some("full") => Ok(RejectedReason::Full),
        Some("conflict") => Ok(RejectedReason::Conflict),
        Some("invalid") => Ok(RejectedReason::Invalid),
        Some("deployment_closed") => Ok(RejectedReason::DeploymentClosed),
        other => Err(format!("unrecognised rejected reason: {other:?}")),
    }
}

fn parse_settled_reason(value: Option<&Value>) -> Result<SettledReason, String> {
    match value.and_then(Value::as_str) {
        Some("stop") => Ok(SettledReason::Stop),
        Some("length") => Ok(SettledReason::Length),
        Some("canceled") => Ok(SettledReason::Canceled),
        Some("error") => Ok(SettledReason::Error),
        other => Err(format!("unrecognised settled reason: {other:?}")),
    }
}

fn require_object(value: &Value) -> Result<&serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "not a JSON object".to_string())
}

fn require_protocol(object: &serde_json::Map<String, Value>) -> Result<(), String> {
    match object.get("protocol").and_then(Value::as_str) {
        Some(PROTOCOL) => Ok(()),
        other => Err(format!("wrong or missing protocol: {other:?}")),
    }
}

fn require_type(object: &serde_json::Map<String, Value>, expected: &str) -> Result<(), String> {
    match object.get("type").and_then(Value::as_str) {
        Some(found) if found == expected => Ok(()),
        other => Err(format!("expected type {expected:?}, found {other:?}")),
    }
}

fn require_id(object: &serde_json::Map<String, Value>, field: &str) -> Result<String, String> {
    let id = object.get(field).and_then(Value::as_str).unwrap_or("");
    if id.is_empty() || id.len() > MAX_ID_LEN {
        return Err(format!("missing or oversized {field}"));
    }
    Ok(id.to_string())
}

fn require_u64(object: &serde_json::Map<String, Value>, field: &str) -> Result<u64, String> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing or non-integer {field}"))
}

fn require_u32(object: &serde_json::Map<String, Value>, field: &str) -> Result<u32, String> {
    u32::try_from(require_u64(object, field)?).map_err(|_| format!("{field} out of range"))
}

fn require_string(object: &serde_json::Map<String, Value>, field: &str) -> Result<String, String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing {field}"))
}

#[cfg(test)]
mod tests;
