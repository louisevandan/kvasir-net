use super::*;
use p4_protocol::Phase;

fn request(options: &str) -> ExecutionRequest {
    ExecutionRequest {
        controller_id: "c".into(),
        node_id: "n".into(),
        deployment_id: "d".into(),
        binding_id: "b".into(),
        runtime_generation: 1,
        request_id: "r".into(),
        session_id: "s".into(),
        phase: Phase::Prefill,
        position: 0,
        max_tokens: 8,
        temperature: 0.7,
        prompt: "hello".into(),
        options: options.into(),
    }
}

#[test]
fn preserves_backend_options_and_protects_p4_fields() {
    let body: Value = serde_json::from_str(
        &request_body(
            &request(r#"{"top_p":0.9,"model":"other","stream":false}"#),
            "bound-model",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(body["top_p"], 0.9);
    assert_eq!(body["model"], "bound-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_tokens"], 8);
}
