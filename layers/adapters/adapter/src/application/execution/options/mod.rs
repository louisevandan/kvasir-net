use p4_protocol::ExecutionRequest;
use serde_json::{Map, Value, json};

const SUPPORTED: [&str; 5] = ["max_tokens", "temperature", "top_p", "top_k", "seed"];

/// Filters the P4 option map to the exact sampling surface accepted by the
/// current Linker Pipeline chat parser. P4 session, prompt, and stream stay owned
/// by this transport adapter.
pub fn request_body(request: &ExecutionRequest) -> Result<Value, String> {
    let options: Map<String, Value> = serde_json::from_str(&request.options)
        .map_err(|error| format!("options must be a JSON object: {error}"))?;
    let mut body = json!({
        "messages": [{ "role": "user", "content": request.prompt }],
        "session_id": request.session_id,
        "max_tokens": request.max_tokens,
        "temperature": request.temperature,
        "stream": true,
    })
    .as_object()
    .cloned()
    .expect("Pipeline request body is an object");
    for key in SUPPORTED {
        if let Some(value) = options.get(key) {
            body.insert(key.into(), value.clone());
        }
    }
    Ok(Value::Object(body))
}

#[cfg(test)]
mod tests;
