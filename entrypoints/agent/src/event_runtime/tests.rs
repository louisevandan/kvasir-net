use super::*;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, decode, encode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub(super) fn limits() -> RuntimeLimits {
    RuntimeLimits {
        queue: 1,
        retained: 8,
        bytes: 1024 * 1024,
        connections: 16,
        hop_receipts: 8,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 4,
    }
}

pub(super) fn event(
    own: &Address,
    target: Endpoint,
    number: u64,
    content_type: &str,
    payload: &[u8],
) -> Event {
    let source = Endpoint::outer(own.clone(), "owned-runtime", 1);
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: format!("input-{number}"),
            correlation_id: format!("request-{number}"),
            causation_id: None,
            source: source.clone(),
            target,
            return_route: match source {
                Endpoint::Outer(route) => Some(route),
                _ => unreachable!(),
            },
            class: EventClass::Control,
            sequence: number,
            deadline_unix_ms: None,
            adapter_kind: None,
            payload_content_type: content_type.into(),
        },
        payload: payload.to_vec(),
    }
}

pub(super) async fn send(stream: &mut TcpStream, event: &Event) {
    let bytes = encode(event).unwrap();
    stream.write_u32_le(bytes.len() as u32).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
}
pub(super) async fn receive(stream: &mut TcpStream) -> Event {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let len = stream.read_u32_le().await.unwrap();
        let mut bytes = vec![0; len as usize];
        stream.read_exact(&mut bytes).await.unwrap();
        decode(&bytes).unwrap()
    })
    .await
    .unwrap()
}

fn lifecycle_payload(
    operation: p4_protocol::event::lifecycle::LifecycleOperation,
    node_id: &str,
    generation: u64,
    adapter_kind: &str,
    adapter_content_type: &str,
    opaque: &[u8],
) -> Vec<u8> {
    use p4_protocol::event::lifecycle::{
        LIFECYCLE_SCHEMA, LifecycleOperation, LifecycleRequestMetadata, encode_metadata,
    };
    let load = operation == LifecycleOperation::Load;
    encode_metadata(
        &LifecycleRequestMetadata {
            schema: LIFECYCLE_SCHEMA,
            node_id: node_id.into(),
            node_generation: generation,
            adapter_kind: adapter_kind.into(),
            adapter_content_type: adapter_content_type.into(),
            queue_capacity: load.then_some(1),
            completion_capacity: load.then_some(1),
            retained_capacity: load.then_some(8),
            retained_bytes: load.then_some(1024 * 1024),
        },
        opaque,
    )
    .unwrap()
}

#[tokio::test]
async fn owned_runtime_actual_tcp_inspection_retires_connection_after_explicit_finish() {
    use p4_protocol::event::{AGENT_INSPECT_CONTENT_TYPE, AGENT_SNAPSHOT_CONTENT_TYPE};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let _runtime = Runtime::start(
        listener,
        own.clone(),
        RuntimeLimits {
            connections: 2,
            ..limits()
        },
    );
    // More independent OUTER generations than the actual admission budget.
    for number in 1..=6 {
        let mut stream = TcpStream::connect((own.host.as_str(), own.port))
            .await
            .unwrap();
        let mut input = event(
            &own,
            Endpoint::agent(own.clone()),
            number,
            AGENT_INSPECT_CONTENT_TYPE,
            b"{}",
        );
        let route = Endpoint::outer(own.clone(), format!("inspection-{number}"), number);
        input.envelope.source = route.clone();
        input.envelope.return_route = match route {
            Endpoint::Outer(route) => Some(route),
            _ => unreachable!(),
        };
        send(&mut stream, &input).await;
        let output = receive(&mut stream).await;
        assert_eq!(
            output.envelope.payload_content_type,
            AGENT_SNAPSHOT_CONTENT_TYPE
        );
        assert_eq!(output.envelope.causation_id, Some(input.envelope.event_id));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.payload).unwrap()["nodes"],
            serde_json::json!([])
        );
        stream.write_u32_le(0).await.unwrap();
        let ack =
            tokio::time::timeout(std::time::Duration::from_secs(2), stream.read_u32_le()).await;
        assert!(
            matches!(ack, Ok(Ok(0))),
            "explicit finish must drain the writer and acknowledge route retirement: {ack:?}"
        );
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
    }
}

#[tokio::test]
async fn node_load_lifecycle_actual_tcp_creates_fences_reports_and_removes_node() {
    use p4_protocol::event::lifecycle::{
        LifecycleOperation, LifecycleResultMetadata, LifecycleStatus,
        NODE_LIFECYCLE_RESULT_CONTENT_TYPE, NODE_LOAD_CONTENT_TYPE, NODE_UNLOAD_CONTENT_TYPE,
        ResourceState, decode_metadata,
    };
    use p4_protocol::event::{AGENT_INSPECT_CONTENT_TYPE, AGENT_SNAPSHOT_CONTENT_TYPE};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let runtime = Runtime::start(listener, own.clone(), limits());
    let mut stream = TcpStream::connect((own.host.as_str(), own.port))
        .await
        .unwrap();

    let mut malformed = event(
        &own,
        Endpoint::agent(own.clone()),
        20,
        NODE_LOAD_CONTENT_TYPE,
        &[9, 0, 0, 0, b'{', b'}'],
    );
    malformed.envelope.adapter_kind = Some("neutral-lifecycle".into());
    send(&mut stream, &malformed).await;
    let rejected = receive(&mut stream).await;
    assert_eq!(rejected.envelope.causation_id.as_deref(), Some("input-20"));
    assert_eq!(
        rejected.envelope.payload_content_type,
        "application/vnd.p4.node.result-v3+json"
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&rejected.payload).unwrap()["ok"],
        false
    );

    let unsupported_payload = lifecycle_payload(
        LifecycleOperation::Load,
        "never-created",
        1,
        "disabled-test-adapter",
        "application/x-disabled-load",
        b"{}",
    );
    let unsupported = event(
        &own,
        Endpoint::agent(own.clone()),
        21,
        NODE_LOAD_CONTENT_TYPE,
        &unsupported_payload,
    );
    send(&mut stream, &unsupported).await;
    let rejected = receive(&mut stream).await;
    let (result, opaque): (LifecycleResultMetadata, _) =
        decode_metadata(&rejected.payload).unwrap();
    assert_eq!(rejected.envelope.causation_id.as_deref(), Some("input-21"));
    assert_eq!(
        rejected.envelope.payload_content_type,
        NODE_LIFECYCLE_RESULT_CONTENT_TYPE
    );
    assert_eq!(result.status, LifecycleStatus::Rejected);
    assert_eq!(result.resource_state, ResourceState::Absent);
    assert!(opaque.is_empty());

    let load_payload = lifecycle_payload(
        LifecycleOperation::Load,
        "lifecycle-node",
        7,
        "neutral-lifecycle",
        super::adapters::neutral::LOAD,
        br#"{"delay_ms":2000}"#,
    );
    let load = event(
        &own,
        Endpoint::agent(own.clone()),
        22,
        NODE_LOAD_CONTENT_TYPE,
        &load_payload,
    );
    let duplicate = event(
        &own,
        Endpoint::agent(own.clone()),
        23,
        NODE_LOAD_CONTENT_TYPE,
        &load_payload,
    );
    send(&mut stream, &load).await;
    send(&mut stream, &duplicate).await;
    let rejected = receive(&mut stream).await;
    let (result, _): (LifecycleResultMetadata, _) = decode_metadata(&rejected.payload).unwrap();
    assert_eq!(rejected.envelope.causation_id.as_deref(), Some("input-23"));
    assert_eq!(result.status, LifecycleStatus::Rejected);
    assert_eq!(result.resource_state, ResourceState::Unknown);

    let inspect_loading = event(
        &own,
        Endpoint::agent(own.clone()),
        24,
        AGENT_INSPECT_CONTENT_TYPE,
        b"{}",
    );
    send(&mut stream, &inspect_loading).await;
    let snapshot = receive(&mut stream).await;
    assert_eq!(
        snapshot.envelope.payload_content_type,
        AGENT_SNAPSHOT_CONTENT_TYPE
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&snapshot.payload).unwrap();
    assert_eq!(snapshot["nodes"][0]["node_id"], "lifecycle-node");
    assert_eq!(snapshot["nodes"][0]["lifecycle_state"], "loading");

    let loaded = receive(&mut stream).await;
    let (result, opaque): (LifecycleResultMetadata, _) = decode_metadata(&loaded.payload).unwrap();
    assert_eq!(loaded.envelope.source, Endpoint::agent(own.clone()));
    assert_eq!(loaded.envelope.target, load.envelope.source);
    assert_eq!(loaded.envelope.return_route, load.envelope.return_route);
    assert_eq!(loaded.envelope.causation_id.as_deref(), Some("input-22"));
    assert_eq!(result.operation, LifecycleOperation::Load);
    assert_eq!(result.status, LifecycleStatus::Succeeded);
    assert_eq!(result.resource_state, ResourceState::Present);
    assert_eq!(
        result.adapter_content_type,
        super::adapters::neutral::RESULT
    );
    assert!(!opaque.is_empty());

    let unload_payload = lifecycle_payload(
        LifecycleOperation::Unload,
        "lifecycle-node",
        7,
        "neutral-lifecycle",
        super::adapters::neutral::UNLOAD,
        br#"{"delay_ms":10}"#,
    );
    let unload = event(
        &own,
        Endpoint::agent(own.clone()),
        25,
        NODE_UNLOAD_CONTENT_TYPE,
        &unload_payload,
    );
    send(&mut stream, &unload).await;
    let unloaded = receive(&mut stream).await;
    let (result, _): (LifecycleResultMetadata, _) = decode_metadata(&unloaded.payload).unwrap();
    assert_eq!(unloaded.envelope.causation_id.as_deref(), Some("input-25"));
    assert_eq!(result.operation, LifecycleOperation::Unload);
    assert_eq!(result.status, LifecycleStatus::Succeeded);
    assert_eq!(result.resource_state, ResourceState::Absent);

    let inspect_empty = event(
        &own,
        Endpoint::agent(own.clone()),
        26,
        AGENT_INSPECT_CONTENT_TYPE,
        b"{}",
    );
    send(&mut stream, &inspect_empty).await;
    let snapshot: serde_json::Value =
        serde_json::from_slice(&receive(&mut stream).await.payload).unwrap();
    assert_eq!(snapshot["nodes"], serde_json::json!([]));
    assert!(!runtime.control.is_finished());
}

#[tokio::test]
async fn node_load_lifecycle_actual_tcp_rejects_legacy_and_direct_bypass_without_node_effects() {
    use p4_protocol::event::lifecycle::{LifecycleOperation, NODE_LOAD_CONTENT_TYPE};
    use p4_protocol::event::{AGENT_INSPECT_CONTENT_TYPE, AGENT_SNAPSHOT_CONTENT_TYPE};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let runtime = Runtime::start(listener, own.clone(), limits());
    let mut stream = TcpStream::connect((own.host.as_str(), own.port))
        .await
        .unwrap();

    for (number, content_type, payload) in [
        (
            30,
            "application/vnd.p4.node.create-v3+json",
            b"{\"node_id\":\"legacy\",\"node_generation\":1,\"adapter_kind\":\"llamacpp\"}"
                .as_slice(),
        ),
        (
            31,
            "application/vnd.p4.node.delete-v3+json",
            b"{\"node_id\":\"legacy\",\"node_generation\":1}".as_slice(),
        ),
    ] {
        send(
            &mut stream,
            &event(
                &own,
                Endpoint::agent(own.clone()),
                number,
                content_type,
                payload,
            ),
        )
        .await;
        let reply = receive(&mut stream).await;
        let result: serde_json::Value = serde_json::from_slice(&reply.payload).unwrap();
        assert_eq!(result["ok"], false);
        assert!(result["detail"].as_str().unwrap().contains("unsupported"));
    }

    // A lifecycle request addressed straight to a nonexistent node never
    // reaches agent control and cannot create a route as a side effect.
    let bypass = lifecycle_payload(
        LifecycleOperation::Load,
        "bypass",
        1,
        "llamacpp",
        p4_llamacpp_staged_adapter::v2::LOAD_CONTENT_TYPE,
        b"{}",
    );
    send(
        &mut stream,
        &event(
            &own,
            Endpoint::node(own.clone(), "bypass", 1),
            32,
            NODE_LOAD_CONTENT_TYPE,
            &bypass,
        ),
    )
    .await;
    drop(stream);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut inspection_sequence = 33;
    loop {
        let mut inspection = TcpStream::connect((own.host.as_str(), own.port))
            .await
            .unwrap();
        send(
            &mut inspection,
            &event(
                &own,
                Endpoint::agent(own.clone()),
                inspection_sequence,
                AGENT_INSPECT_CONTENT_TYPE,
                b"{}",
            ),
        )
        .await;
        let response = receive(&mut inspection).await;
        assert_eq!(
            response.envelope.payload_content_type,
            AGENT_SNAPSHOT_CONTENT_TYPE
        );
        let snapshot: serde_json::Value = serde_json::from_slice(&response.payload).unwrap();
        assert_eq!(snapshot["nodes"], serde_json::json!([]));
        if snapshot["transport"]["failures"]["count"] == 1 {
            assert_eq!(
                snapshot["transport"]["failures"]["states"]["rejected_remote"],
                1
            );
            break;
        }
        inspection_sequence += 1;
        assert!(std::time::Instant::now() < deadline, "{snapshot}");
        tokio::task::yield_now().await;
    }
    assert!(!runtime.control.is_finished());
}

#[tokio::test]
async fn owned_runtime_actual_tcp_peer_forwards_and_returns_opaque_control() {
    let a = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let b = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own_a = Address::tcp("127.0.0.1", a.local_addr().unwrap().port());
    let own_b = Address::tcp("127.0.0.1", b.local_addr().unwrap().port());
    let _a = Runtime::start(a, own_a.clone(), limits());
    let _b = Runtime::start(b, own_b.clone(), limits());
    let mut stream = TcpStream::connect((own_a.host.as_str(), own_a.port))
        .await
        .unwrap();
    for number in 1..=8 {
        send(
            &mut stream,
            &event(
                &own_a,
                Endpoint::agent(own_b.clone()),
                number,
                "application/x-independent-backend",
                "원격 원문".as_bytes(),
            ),
        )
        .await;
        let reply = receive(&mut stream).await;
        assert_eq!(reply.envelope.causation_id, Some(format!("input-{number}")));
        assert_eq!(reply.envelope.source, Endpoint::agent(own_b.clone()));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&reply.payload).unwrap()["ok"],
            false
        );
    }
    assert_eq!(
        _a.transport.outer_route_count().await,
        1,
        "only the reception agent owns the OUTER socket"
    );
    assert_eq!(
        _b.transport.outer_route_count().await,
        0,
        "forwarded OUTER identity is not a socket registration"
    );
}
