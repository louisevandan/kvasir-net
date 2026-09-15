use super::load::{self, LoadedBuild};
use super::{RunConfig, Sender, wire};
use p4_llamacpp_staged_adapter::v2::{LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE};
use p4_protocol::{
    Address,
    event::{
        Endpoint, Event, EventClass, OuterEndpoint,
        lifecycle::{
            LIFECYCLE_SCHEMA, LifecycleOperation, LifecycleRequestMetadata,
            LifecycleResultMetadata, LifecycleStatus, NODE_LIFECYCLE_RESULT_CONTENT_TYPE,
            NODE_LOAD_CONTENT_TYPE, NODE_UNLOAD_CONTENT_TYPE, ResourceState, decode_metadata,
            encode_metadata,
        },
    },
};
use serde_json::json;
use std::time::{Duration, Instant};

fn config(profile: &str) -> RunConfig {
    let mut value = json!({
        "ingress_agent":"tcp://127.0.0.1:50001", "channel":"load-abi", "connection_generation":1,
        "load_generation":1, "session_id":"s", "request_id":"r", "max_tokens":10,
        "prompt":"not submitted", "pipeline_compatibility":profile,
        "nodes":[
            {"agent":"tcp://127.0.0.1:50001","node":"head","generation":1,"binary":"fixture","endpoint":"127.0.0.1:1","plan":"fixture","n_batch":4,"n_ubatch":4,"context_size":8,"total_context_size":8,"sequence_capacity":1},
            {"agent":"tcp://127.0.0.2:50001","node":"tail","generation":2,"binary":"fixture","endpoint":"127.0.0.1:2","plan":"fixture","n_batch":4,"n_ubatch":4,"context_size":8,"total_context_size":8,"sequence_capacity":1}
        ], "timeout_ms":1000
    });
    let resource_profile = serde_json::to_value(super::config::test_resource_profile()).unwrap();
    for node in value["nodes"].as_array_mut().unwrap() {
        node["resource_profile"] = resource_profile.clone();
    }
    serde_json::from_value(value).unwrap()
}

fn loaded_reply(request: &Event, body: serde_json::Value, serial: usize) -> Event {
    let (metadata, opaque): (LifecycleRequestMetadata, _) =
        decode_metadata(&request.payload).unwrap();
    assert_eq!(metadata.adapter_kind, "llamacpp");
    assert_eq!(metadata.adapter_content_type, LOAD_CONTENT_TYPE);
    assert!(!opaque.is_empty());
    let result = LifecycleResultMetadata {
        schema: LIFECYCLE_SCHEMA,
        node_id: metadata.node_id,
        node_generation: metadata.node_generation,
        adapter_kind: metadata.adapter_kind,
        adapter_content_type: LOADED_CONTENT_TYPE.into(),
        operation: LifecycleOperation::Load,
        status: LifecycleStatus::Succeeded,
        resource_state: ResourceState::Present,
        first_error: None,
        cleanup_error: None,
    };
    let mut reply = request.clone();
    reply.envelope.event_id = format!("fixture-load-{serial}");
    reply.envelope.causation_id = Some(request.envelope.event_id.clone());
    reply.envelope.source = request.envelope.target.clone();
    reply.envelope.target = request.envelope.source.clone();
    reply.envelope.class = EventClass::Control;
    reply.envelope.payload_content_type = NODE_LIFECYCLE_RESULT_CONTENT_TYPE.into();
    reply.payload = encode_metadata(&result, &serde_json::to_vec(&body).unwrap()).unwrap();
    reply
}

fn rejected_load_reply(request: &Event, serial: usize) -> Event {
    let (metadata, _): (LifecycleRequestMetadata, _) = decode_metadata(&request.payload).unwrap();
    let result = LifecycleResultMetadata {
        schema: LIFECYCLE_SCHEMA,
        node_id: metadata.node_id,
        node_generation: metadata.node_generation,
        adapter_kind: metadata.adapter_kind,
        adapter_content_type: LOADED_CONTENT_TYPE.into(),
        operation: LifecycleOperation::Load,
        status: LifecycleStatus::Rejected,
        resource_state: ResourceState::Absent,
        first_error: Some("fixture LOAD rejection".into()),
        cleanup_error: None,
    };
    let mut reply = request.clone();
    reply.envelope.event_id = format!("fixture-rejected-{serial}");
    reply.envelope.causation_id = Some(request.envelope.event_id.clone());
    reply.envelope.source = request.envelope.target.clone();
    reply.envelope.target = request.envelope.source.clone();
    reply.envelope.class = EventClass::Control;
    reply.envelope.payload_content_type = NODE_LIFECYCLE_RESULT_CONTENT_TYPE.into();
    reply.payload = encode_metadata(&result, b"").unwrap();
    reply
}

fn unloaded_reply(request: &Event, serial: usize) -> Event {
    let (metadata, opaque): (LifecycleRequestMetadata, _) =
        decode_metadata(&request.payload).unwrap();
    assert_eq!(
        metadata.adapter_content_type,
        p4_llamacpp_staged_adapter::v2::UNLOAD_CONTENT_TYPE
    );
    assert!(!opaque.is_empty());
    let result = LifecycleResultMetadata {
        schema: LIFECYCLE_SCHEMA,
        node_id: metadata.node_id,
        node_generation: metadata.node_generation,
        adapter_kind: metadata.adapter_kind,
        adapter_content_type: p4_llamacpp_staged_adapter::v2::UNLOADED_CONTENT_TYPE.into(),
        operation: LifecycleOperation::Unload,
        status: LifecycleStatus::Succeeded,
        resource_state: ResourceState::Absent,
        first_error: None,
        cleanup_error: None,
    };
    let mut reply = request.clone();
    reply.envelope.event_id = format!("fixture-unload-{serial}");
    reply.envelope.causation_id = Some(request.envelope.event_id.clone());
    reply.envelope.source = request.envelope.target.clone();
    reply.envelope.target = request.envelope.source.clone();
    reply.envelope.class = EventClass::Control;
    reply.envelope.payload_content_type = NODE_LIFECYCLE_RESULT_CONTENT_TYPE.into();
    reply.payload = encode_metadata(&result, b"{}").unwrap();
    reply
}

async fn consume(profile: &str, changed: Option<&str>) -> Result<LoadedBuild, String> {
    let config = config(profile);
    let outer = OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 50001),
        channel: "load-abi".into(),
        connection_generation: 1,
    };
    let mut sender = Sender::new(outer);
    let (client, peer) = tokio::io::duplex(65536);
    let (reader, writer) = tokio::io::split(client);
    let (reader_peer, writer_peer) = tokio::io::split(peer);
    let mut client = wire::EventWire::new(reader, writer);
    let changed = changed.map(str::to_owned);
    let fixture = tokio::spawn(async move {
        let mut peer = wire::EventWire::new(reader_peer, writer_peer);
        let mut replies = Vec::new();
        for index in 0..2 {
            let request = peer
                .receive(Instant::now() + Duration::from_secs(1))
                .await
                .unwrap();
            assert_eq!(
                request.envelope.payload_content_type,
                NODE_LOAD_CONTENT_TYPE
            );
            assert!(matches!(request.envelope.target, Endpoint::Agent(_)));
            let mut body = json!({"upstream_commit":"pin", "patch_set":"patch",
                "backend_inventory":if index==0 {"CPU[CPU]|CUDA[CUDA0]"} else {"CPU[CPU]|MTL[MTL0]"},
                "stage_wire_abi":format!("p4pb4le64:{}:types=0/1/4,1/1/2,2/32/18", "a".repeat(64))});
            if index == 1 {
                if changed.as_deref() == Some("different_wire") {
                    body["stage_wire_abi"] = json!(format!(
                        "p4pb4le64:{}:types=0/1/4,1/1/2,2/32/18",
                        "b".repeat(64)
                    ));
                } else if let Some(field) = &changed {
                    body.as_object_mut().unwrap().remove(field);
                }
            }
            replies.push(loaded_reply(&request, body, index));
        }
        for reply in replies.into_iter().rev() {
            peer.send(reply).await.unwrap();
        }
    });
    let result = load::drive(&config, &mut client, &mut sender)
        .await
        .map_err(|error| error.to_string());
    fixture.await.unwrap();
    result
}

#[tokio::test]
async fn load_consumer_preserves_heterogeneous_stage_identities_only_with_explicit_wire_profile() {
    assert!(consume("exact-build", None).await.is_err());
    let loaded = consume("physical-wire-v4", None).await.unwrap();
    assert_eq!(loaded.stages.len(), 2);
    assert_eq!(loaded.stages[0].node, "head");
    assert_eq!(loaded.stages[1].node, "tail");
    assert_eq!(loaded.representative, loaded.stages[0].identity);
    assert_ne!(
        loaded.stages[0].identity.backend_inventory,
        loaded.stages[1].identity.backend_inventory
    );
    for field in [
        "upstream_commit",
        "patch_set",
        "backend_inventory",
        "stage_wire_abi",
        "different_wire",
    ] {
        assert!(
            consume("physical-wire-v4", Some(field)).await.is_err(),
            "missing {field}"
        );
    }
}

#[tokio::test]
async fn node_load_lifecycle_partial_rejection_unloads_only_the_prior_success() {
    let config = config("physical-wire-v4");
    let outer = OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 50001),
        channel: "partial-load".into(),
        connection_generation: 2,
    };
    let mut sender = Sender::new(outer);
    let (client, peer) = tokio::io::duplex(65536);
    let (reader, writer) = tokio::io::split(client);
    let (reader_peer, writer_peer) = tokio::io::split(peer);
    let mut client = wire::EventWire::new(reader, writer);
    let fixture = tokio::spawn(async move {
        let mut peer = wire::EventWire::new(reader_peer, writer_peer);
        let first = peer
            .receive(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        let second = peer
            .receive(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(first.envelope.payload_content_type, NODE_LOAD_CONTENT_TYPE);
        assert_eq!(second.envelope.payload_content_type, NODE_LOAD_CONTENT_TYPE);
        peer.send(loaded_reply(
            &first,
            json!({"upstream_commit":"pin","patch_set":"patch","backend_inventory":"CPU[CPU]",
                "stage_wire_abi":format!("p4pb4le64:{}:types=0/1/4,1/1/2,2/32/18", "a".repeat(64))}),
            1,
        ))
        .await
        .unwrap();
        peer.send(rejected_load_reply(&second, 2)).await.unwrap();

        let cleanup = peer
            .receive(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            cleanup.envelope.payload_content_type,
            NODE_UNLOAD_CONTENT_TYPE
        );
        let (metadata, _): (LifecycleRequestMetadata, _) =
            decode_metadata(&cleanup.payload).unwrap();
        assert_eq!(metadata.node_id, "head");
        peer.send(unloaded_reply(&cleanup, 3)).await.unwrap();
    });
    let error = match load::drive(&config, &mut client, &mut sender).await {
        Ok(_) => panic!("fixture rejection unexpectedly loaded every node"),
        Err(error) => error.to_string(),
    };
    fixture.await.unwrap();
    assert!(error.contains("fixture LOAD rejection"), "{error}");
}
