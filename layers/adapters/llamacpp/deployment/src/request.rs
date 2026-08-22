//! Translation from P4's backend-neutral submission request to the request
//! object understood by the llama pipeline submission server.
//!
//! This is intentionally inside the llama deployment client. P4 carries one
//! prompt, its output bound, and opaque option text; only this adapter knows
//! that llama's server expects an OpenAI-compatible `messages` array and a
//! streaming request. Keeping the translation here prevents backend API
//! vocabulary from leaking into the broker.

use serde_json::{Value, json};

pub(crate) fn for_llama(request: &Value) -> Value {
    let Some(prompt) = request.get("prompt").and_then(Value::as_str) else {
        return request.clone();
    };
    let Some(max_tokens) = request.get("max_tokens").and_then(Value::as_u64) else {
        return request.clone();
    };

    let mut llama = json!({
        "messages": [{ "role": "user", "content": prompt }],
        "max_tokens": max_tokens,
        "stream": true,
    });
    let Some(options) = request.get("options").and_then(Value::as_str) else {
        return llama;
    };
    let Ok(Value::Object(options)) = serde_json::from_str::<Value>(options) else {
        return llama;
    };
    let fields = llama
        .as_object_mut()
        .expect("llama request root is an object");
    for (key, value) in options {
        fields.insert(key, value);
    }
    llama
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_llama_adapter_owns_chat_shape_and_option_merge() {
        let request = json!({
            "prompt": "러스트를 설명하라",
            "max_tokens": 64,
            "options": r#"{"temperature":0.2,"stream":false}"#,
        });

        assert_eq!(
            for_llama(&request),
            json!({
                "messages": [{ "role": "user", "content": "러스트를 설명하라" }],
                "max_tokens": 64,
                "stream": false,
                "temperature": 0.2,
            })
        );
    }

    #[test]
    fn a_direct_backend_request_is_not_reinterpreted() {
        let request = json!({
            "messages": [{ "role": "user", "content": "direct" }],
            "max_tokens": 4,
            "stream": true,
        });
        assert_eq!(for_llama(&request), request);
    }
}
