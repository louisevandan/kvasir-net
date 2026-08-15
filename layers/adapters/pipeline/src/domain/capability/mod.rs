//! Pipeline-native compatibility data advertised through the generic P4 adapter descriptor.

use crate::infrastructure::http::synchronous as http;
use serde_json::{Value, json};

pub(crate) const DESCRIPTOR_SCHEMA: &str = "p4.adapter-capability/v1";

pub(crate) fn descriptor(host: &str) -> String {
    let runtime = match http::json(host, "GET", "/api/runtime", None) {
        Ok((200, body, _)) => body
            .get("identity")
            .map(descriptor_from_identity)
            .unwrap_or_else(|| unavailable("runtime identity is missing")),
        Ok((status, _, text)) => unavailable(&format!("runtime identity HTTP {status}: {text}")),
        Err(error) => unavailable(&format!("runtime identity request failed: {error}")),
    };
    json!({
        "schema": DESCRIPTOR_SCHEMA,
        "lifecycle": "model-load-creates-runtime",
        "model_binding": "replaceable",
        "pipeline_runtime": runtime,
    })
    .to_string()
}

fn descriptor_from_identity(identity: &Value) -> Value {
    let Some(pipeline) = identity.get("pipeline").filter(|value| value.is_object()) else {
        return unavailable("runtime identity does not advertise Pipeline capabilities");
    };
    let Some(protocol) = pipeline.get("protocol").and_then(Value::as_str) else {
        return unavailable("runtime identity has no Pipeline protocol");
    };
    let Some(adapter_abi) = pipeline.get("adapter_abi").and_then(Value::as_u64) else {
        return unavailable("runtime identity has no Adapter ABI");
    };
    let Some(capability_bits) = pipeline.get("capability_bits").and_then(Value::as_u64) else {
        return unavailable("runtime identity has no Pipeline capability bits");
    };
    json!({
        "available": true,
        "protocol": protocol,
        "adapter_abi": adapter_abi,
        "capability_bits": capability_bits,
        "build_id": pipeline.get("build_id").cloned().unwrap_or(Value::Null),
        "runtime_contract": identity.get("runtime_contract").cloned().unwrap_or(Value::Null),
    })
}

fn unavailable(detail: &str) -> Value {
    json!({ "available": false, "detail": detail })
}

#[cfg(test)]
mod tests;
