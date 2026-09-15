use super::*;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, encode, decode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub(super) fn limits() -> RuntimeLimits { RuntimeLimits { queue: 1, retained: 8, bytes: 1024 * 1024, connections: 16 } }

pub(super) fn event(own: &Address, target: Endpoint, number: u64, content_type: &str, payload: &[u8]) -> Event {
    let source = Endpoint::outer(own.clone(), "owned-runtime", 1);
    Event { envelope: Envelope { protocol_version: Envelope::VERSION, event_id: format!("input-{number}"),
        correlation_id: format!("request-{number}"), causation_id: None, source: source.clone(), target,
        return_route: match source { Endpoint::Outer(route) => Some(route), _ => unreachable!() },
        class: EventClass::Control, sequence: number, deadline_unix_ms: None, adapter_kind: None,
        payload_content_type: content_type.into() }, payload: payload.to_vec() }
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
        stream.read_exact(&mut bytes).await.unwrap(); decode(&bytes).unwrap()
    }).await.unwrap()
}

#[tokio::test]
async fn owned_runtime_actual_tcp_inspection_retires_connection_after_explicit_finish() {
    use p4_protocol::event::{AGENT_INSPECT_CONTENT_TYPE, AGENT_SNAPSHOT_CONTENT_TYPE};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let _runtime = Runtime::start(listener, own.clone(), RuntimeLimits { connections: 2, ..limits() });
    // More independent OUTER generations than the actual admission budget.
    for number in 1..=6 {
        let mut stream = TcpStream::connect((own.host.as_str(), own.port)).await.unwrap();
        let mut input = event(&own, Endpoint::agent(own.clone()), number, AGENT_INSPECT_CONTENT_TYPE, b"{}");
        let route = Endpoint::outer(own.clone(), format!("inspection-{number}"), number);
        input.envelope.source = route.clone();
        input.envelope.return_route = match route { Endpoint::Outer(route) => Some(route), _ => unreachable!() };
        send(&mut stream, &input).await;
        let output = receive(&mut stream).await;
        assert_eq!(output.envelope.payload_content_type, AGENT_SNAPSHOT_CONTENT_TYPE);
        assert_eq!(output.envelope.causation_id, Some(input.envelope.event_id));
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.payload).unwrap()["nodes"], serde_json::json!([]));
        stream.write_u32_le(0).await.unwrap();
        let ack = tokio::time::timeout(std::time::Duration::from_secs(2), stream.read_u32_le()).await;
        assert!(matches!(ack, Ok(Ok(0))), "explicit finish must drain the writer and acknowledge route retirement: {ack:?}");
        assert_eq!(stream.read(&mut [0; 1]).await.unwrap(), 0);
    }
}

#[tokio::test]
async fn owned_runtime_actual_tcp_root_creates_uses_and_deletes_real_worker() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let runtime = Runtime::start(listener, own.clone(), limits());
    let mut stream = TcpStream::connect((own.host.as_str(), own.port)).await.unwrap();
    let create = b"{\"node_id\":\"node\",\"node_generation\":1,\"adapter_kind\":\"llamacpp\",\"queue_capacity\":1,\"completion_capacity\":1}";
    send(&mut stream, &event(&own, Endpoint::agent(own.clone()), 1, "application/vnd.p4.node.create-v3+json", create)).await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&receive(&mut stream).await.payload).unwrap()["ok"], true);
    // A real adapter Worker consumes this unknown opaque command and publishes
    // its normal diagnostic through node->broker->OUTER->socket. No mock adapter.
    send(&mut stream, &event(&own, Endpoint::node(own.clone(), "node", 1), 2, "application/x-owned-utf8-test", "긴 입력 원문 유지".as_bytes())).await;
    let output = receive(&mut stream).await;
    assert_eq!(output.envelope.causation_id.as_deref(), Some("input-2"));
    assert!(String::from_utf8(output.payload).unwrap().contains("unsupported llama adapter"));
    // Separate idle node: lifecycle deletion must fence admission and prove
    // actual delivery stores empty, without weakening the unloaded requirement.
    let create_idle = String::from_utf8(create.to_vec()).unwrap().replace("\"node\"", "\"idle\"");
    send(&mut stream, &event(&own, Endpoint::agent(own.clone()), 3, "application/vnd.p4.node.create-v3+json", create_idle.as_bytes())).await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&receive(&mut stream).await.payload).unwrap()["ok"], true);
    send(&mut stream, &event(&own, Endpoint::agent(own.clone()), 4, "application/vnd.p4.node.delete-v3+json", b"{\"node_id\":\"idle\",\"node_generation\":1}")).await;
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&receive(&mut stream).await.payload).unwrap()["ok"], true);
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
    let mut stream = TcpStream::connect((own_a.host.as_str(), own_a.port)).await.unwrap();
    for number in 1..=8 {
        send(&mut stream, &event(&own_a, Endpoint::agent(own_b.clone()), number,
            "application/x-independent-backend", "원격 원문".as_bytes())).await;
        let reply = receive(&mut stream).await;
        assert_eq!(reply.envelope.causation_id, Some(format!("input-{number}")));
        assert_eq!(reply.envelope.source, Endpoint::agent(own_b.clone()));
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&reply.payload).unwrap()["ok"], false);
    }
    assert_eq!(_a.transport.outer_route_count().await, 1, "only the reception agent owns the OUTER socket");
    assert_eq!(_b.transport.outer_route_count().await, 0, "forwarded OUTER identity is not a socket registration");
}
