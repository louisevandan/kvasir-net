//! The OpenAI-compatible surface, which is the reason one adapter shape covers
//! three backends.
//!
//! `llama-server`, vLLM and SGLang all answer `/v1/chat/completions`, streamed
//! as server-sent events whose payloads are chat-completion chunks. So what
//! this file knows is that shape and nothing about llama.cpp in particular —
//! the llama.cpp-specific part of this crate is which process to start and
//! what its plan means, not how to talk to it.

use serde_json::{Value, json};

/// What one sequence is asking for.
pub struct Request<'a> {
    pub model: &'a str,
    pub prompt: &'a str,
    pub max_tokens: u32,
    /// Sampling, opaque above the boundary and passed through whole. Anything
    /// that is not an object is ignored rather than refused: options are the
    /// caller's business and a backend that dislikes them will say so.
    pub options: &'a str,
}

impl Request<'_> {
    /// The body to post. Streamed, because a hop reports one token and the
    /// ring laps — asking for the whole completion and holding it would make
    /// the first token cost as much as the last.
    pub fn body(&self) -> String {
        let mut root = json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": self.prompt }],
            "max_tokens": self.max_tokens,
            "stream": true,
        });
        if let Ok(Value::Object(options)) = serde_json::from_str::<Value>(self.options) {
            let map = root.as_object_mut().expect("root is an object");
            for (key, value) in options {
                map.insert(key, value);
            }
        }
        root.to_string()
    }
}

/// One chunk of a streamed completion.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chunk {
    pub text: String,
    /// Present on the last chunk of a sequence, and the reason it ended.
    pub stop: Option<String>,
}

/// Reads one event payload.
///
/// Returns `Err` for a payload that is an error object, because a backend
/// reporting a problem inside a 200 stream is ordinary and treating it as an
/// empty token would hang the sequence until its deadline.
pub fn chunk(payload: &str) -> Result<Chunk, String> {
    let value: Value = serde_json::from_str(payload).map_err(|error| error.to_string())?;
    if let Some(error) = value.get("error") {
        return Err(describe(error));
    }
    let Some(choice) = value.get("choices").and_then(|value| value.get(0)) else {
        // A chunk with no choices is a keep-alive or a usage record. Nothing
        // to report and not a failure.
        return Ok(Chunk::default());
    };
    let text = choice
        .get("delta")
        .and_then(|delta| delta.get("content"))
        // A reasoning model streams its thinking under a different key and
        // leaves `content` null until it has finished. Reading only `content`
        // dropped every token of a fourteen-second answer and reported the
        // request complete having produced nothing — the model looked silent
        // and the layer looked fine.
        //
        // They are merged rather than distinguished because nothing above the
        // adapter has a field for a kind of token. Separating them is a
        // protocol change and worth making deliberately; losing them is not a
        // choice at all.
        .filter(|value| !value.is_null())
        .or_else(|| {
            choice
                .get("delta")
                .and_then(|delta| delta.get("reasoning_content"))
        })
        // Non-streamed answers put it here, and a backend may fall back to
        // that shape even when asked to stream.
        .or_else(|| {
            choice
                .get("message")
                .and_then(|message| message.get("content"))
        })
        .or_else(|| choice.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let stop = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(Chunk { text, stop })
}

/// Reads a whole non-streamed answer, for the calls that are not completions.
pub fn failure(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    value.get("error").map(describe)
}

fn describe(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| error.to_string())
}

#[cfg(test)]
mod tests;
