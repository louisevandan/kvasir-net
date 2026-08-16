use super::*;

#[test]
fn a_request_asks_for_a_stream() {
    let body = Request {
        model: "qwen",
        prompt: "안녕",
        max_tokens: 16,
        options: "{}",
    }
    .body();
    let value: Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(value["stream"], json!(true));
    assert_eq!(value["max_tokens"], json!(16));
    assert_eq!(value["messages"][0]["content"], json!("안녕"));
}

/// Sampling is the caller's business and travels whole.
#[test]
fn options_are_merged_rather_than_interpreted() {
    let body = Request {
        model: "qwen",
        prompt: "hi",
        max_tokens: 4,
        options: r#"{"temperature":0.2,"top_p":0.9,"seed":7}"#,
    }
    .body();
    let value: Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(value["temperature"], json!(0.2));
    assert_eq!(value["seed"], json!(7));
    assert_eq!(value["stream"], json!(true), "and do not lose the stream");
}

#[test]
fn options_that_are_not_an_object_are_ignored_rather_than_refused() {
    for options in ["", "null", "[]", "not json at all"] {
        let body = Request {
            model: "qwen",
            prompt: "hi",
            max_tokens: 1,
            options,
        }
        .body();
        assert!(
            serde_json::from_str::<Value>(&body).is_ok(),
            "{options} still produced a valid request"
        );
    }
}

#[test]
fn a_delta_chunk_is_one_token() {
    let parsed = chunk(r#"{"choices":[{"delta":{"content":"안"},"finish_reason":null}]}"#).unwrap();
    assert_eq!(parsed.text, "안");
    assert_eq!(parsed.stop, None);
}

#[test]
fn the_last_chunk_carries_why_it_ended() {
    let parsed = chunk(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#).unwrap();
    assert_eq!(parsed.text, "");
    assert_eq!(parsed.stop.as_deref(), Some("stop"));
}

/// A backend may answer the non-streamed shape even when asked to stream, and
/// dropping the text because it was in the wrong field would look like a model
/// that produced nothing.
#[test]
fn a_whole_message_is_read_as_well_as_a_delta() {
    let parsed =
        chunk(r#"{"choices":[{"message":{"content":"whole"},"finish_reason":"length"}]}"#).unwrap();
    assert_eq!(parsed.text, "whole");
    assert_eq!(parsed.stop.as_deref(), Some("length"));
}

#[test]
fn a_keep_alive_with_no_choices_is_not_a_failure() {
    assert_eq!(
        chunk(r#"{"usage":{"total_tokens":3}}"#).unwrap(),
        Chunk::default()
    );
}

/// An error inside a 200 stream is ordinary. Reading it as an empty token
/// would hang the sequence until its deadline instead of reporting it.
#[test]
fn an_error_payload_is_an_error() {
    let error = chunk(r#"{"error":{"message":"context is full","code":400}}"#).unwrap_err();
    assert!(error.contains("context is full"), "{error}");
}

#[test]
fn an_error_without_a_message_still_says_something() {
    let error = chunk(r#"{"error":{"code":500}}"#).unwrap_err();
    assert!(error.contains("500"), "{error}");
}

#[test]
fn a_payload_that_is_not_json_is_an_error_rather_than_an_empty_token() {
    assert!(chunk("<html>502 Bad Gateway</html>").is_err());
}

#[test]
fn a_failure_body_is_recognised_and_a_normal_one_is_not() {
    assert_eq!(
        failure(r#"{"error":{"message":"no model loaded"}}"#).as_deref(),
        Some("no model loaded")
    );
    assert_eq!(failure(r#"{"model":"qwen"}"#), None);
}
