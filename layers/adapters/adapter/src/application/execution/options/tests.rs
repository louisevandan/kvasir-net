use super::*;
use p4_protocol::Phase;

#[test]
fn copies_only_pipeline_supported_options() {
    let request = ExecutionRequest {
        controller_id: "c".into(),
        node_id: "n".into(),
        deployment_id: "d".into(),
        binding_id: "b".into(),
        runtime_generation: 1,
        request_id: "r".into(),
        session_id: "s".into(),
        phase: Phase::Decode,
        position: 1,
        max_tokens: 8,
        temperature: 0.7,
        prompt: "hello".into(),
        options: r#"{"top_p":0.9,"top_k":20,"seed":7,"repeat_penalty":1.1}"#.into(),
    };
    let body = request_body(&request).unwrap();
    assert_eq!(body["top_p"], 0.9);
    assert_eq!(body["top_k"], 20);
    assert_eq!(body["seed"], 7);
    assert!(body.get("repeat_penalty").is_none());
}
