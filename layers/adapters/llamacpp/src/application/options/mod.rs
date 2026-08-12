use p4_protocol::ExecutionRequest;
use serde_json::{Map, Value, json};

/// Keeps P4-owned routing and streaming fields authoritative while passing all
/// other OpenAI-compatible llama-server options through unchanged.
pub fn request_body(request: &ExecutionRequest, model: &str) -> Result<String, String> {
    let mut body: Map<String, Value> = serde_json::from_str(&request.options)
        .map_err(|error| format!("options must be a JSON object: {error}"))?;
    for protected in ["model", "messages", "stream"] {
        body.remove(protected);
    }
    body.entry("max_tokens")
        .or_insert_with(|| json!(request.max_tokens));
    body.entry("temperature")
        .or_insert_with(|| json!(request.temperature));
    body.insert("model".into(), json!(model));
    body.insert(
        "messages".into(),
        json!([{ "role": "user", "content": request.prompt }]),
    );
    body.insert("stream".into(), json!(true));
    Ok(Value::Object(body).to_string())
}

#[cfg(test)]
mod tests;
