use super::*;

#[tokio::test]
async fn an_empty_agent_still_reports_machine_and_protocol_identity() {
    let snapshot = snapshot(&HashMap::new()).await;

    assert_eq!(snapshot["schema"], 1);
    assert_eq!(snapshot["protocol_version"], Envelope::VERSION);
    assert_eq!(
        snapshot["machine"]["capability"]["os"],
        std::env::consts::OS
    );
    assert_eq!(
        snapshot["machine"]["capability"]["arch"],
        std::env::consts::ARCH
    );
    assert_eq!(
        snapshot["machine"]["capability"]["adapters"],
        json!(["llamacpp"])
    );
    assert!(snapshot["machine"]["capability"]["cpu"]["logical_cores"].is_u64());
    assert!(snapshot["machine"]["capability"]["memory"].is_object());
    assert!(snapshot["machine"]["capability"]["gpus"].is_array());
    assert!(snapshot["machine"]["occupancy"]["memory"].is_object());
    assert!(snapshot["machine"]["occupancy"]["gpus"].is_array());
    assert!(snapshot["machine"]["probes"]["gpus"].is_object());
    assert_eq!(snapshot["nodes"], json!([]));
    assert!(snapshot["generated_at_unix_ms"].as_u64().unwrap_or(0) > 0);
}
