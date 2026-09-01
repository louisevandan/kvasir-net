//! The conversation identity has to survive the wire, not just parse.
//!
//! The grammar tests in `session_key` prove a string is well formed. These
//! prove the field reaches the adapter intact, that a malformed key is refused
//! at admission rather than carried into a store path, and that a request
//! identity cannot change which conversation it belongs to.

use super::{InferenceCommand, SessionKey};

fn command(session_key: Option<&str>) -> InferenceCommand {
    InferenceCommand {
        load_generation: 1,
        session_id: "pipeline".into(),
        request_id: "req-001".into(),
        tokens: vec![1, 2, 3],
        prompt: None,
        options: String::new(),
        session_key: session_key.map(str::to_owned),
        max_tokens: 16,
    }
}

#[test]
fn a_session_key_survives_a_json_round_trip() {
    let sent = command(Some("sk1:tenant-a/conv-7f3c"));
    let wire = serde_json::to_vec(&sent).expect("serialise");
    let received: InferenceCommand = serde_json::from_slice(&wire).expect("deserialise");
    assert_eq!(received, sent);
    assert_eq!(received.session_key.as_deref(), Some("sk1:tenant-a/conv-7f3c"));
    assert_eq!(
        received.parsed_session_key().map(|key| key.owner().to_owned()),
        Some("tenant-a".to_owned())
    );
}

#[test]
fn a_payload_without_the_field_still_decodes() {
    // An OUTER that never persists need not mint a key, and an older payload
    // must not become undecodable because this field was added.
    let wire = serde_json::json!({
        "load_generation": 1,
        "session_id": "pipeline",
        "request_id": "req-001",
        "tokens": [1, 2, 3],
        "max_tokens": 16,
    });
    let received: InferenceCommand = serde_json::from_value(wire).expect("deserialise");
    assert_eq!(received.session_key, None);
    assert_eq!(received.validate(), Ok(()));
}

#[test]
fn admission_refuses_a_malformed_key() {
    // Refused here rather than carried: the key becomes a store path
    // component, and two conversations that collide there cannot be told
    // apart afterwards by comparing bytes.
    for malformed in ["tenant/conv", "sk1:tenant", "sk1:/conv", "sk1:tenant/   "] {
        let result = command(Some(malformed)).validate();
        assert!(result.is_err(), "{malformed} should be refused");
    }
}

#[test]
fn admission_accepts_a_well_formed_key() {
    assert_eq!(command(Some("sk1:tenant-a/conv-7f3c")).validate(), Ok(()));
}

#[test]
fn one_conversation_spans_many_request_identities() {
    // The point of the key: separate turns of the same conversation share it
    // while their request ids differ.
    let mut first = command(Some("sk1:tenant-a/conv-7f3c"));
    first.request_id = "req-001".into();
    let mut second = command(Some("sk1:tenant-a/conv-7f3c"));
    second.request_id = "req-002".into();
    assert_eq!(first.validate(), Ok(()));
    assert_eq!(second.validate(), Ok(()));
    assert_eq!(first.parsed_session_key(), second.parsed_session_key());
    assert_ne!(first.request_id, second.request_id);
}

#[test]
fn normalisation_forms_are_different_conversations_on_the_wire() {
    let composed = command(Some("sk1:owner/\u{AC00}"));
    let decomposed = command(Some("sk1:owner/\u{1100}\u{1161}"));
    assert_eq!(composed.validate(), Ok(()));
    assert_eq!(decomposed.validate(), Ok(()));
    assert_ne!(composed.parsed_session_key(), decomposed.parsed_session_key());
}

#[test]
fn the_parsed_key_matches_direct_parsing() {
    let raw = "sk1:tenant-a/team/conv/1";
    let parsed = command(Some(raw)).parsed_session_key().expect("valid");
    assert_eq!(parsed, SessionKey::parse(raw).expect("valid"));
    assert_eq!(parsed.conversation(), "team/conv/1");
}
