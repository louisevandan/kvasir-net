//! The reader has to survive being timed out, because the drive times out on
//! purpose at every arrival wave.

use super::EventWire;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, OuterEndpoint, encode};
use std::io;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};

async fn read_body(stream: &mut tokio::io::DuplexStream) -> Vec<u8> {
    let size = stream.read_u32_le().await.unwrap() as usize;
    let mut body = vec![0; size];
    stream.read_exact(&mut body).await.unwrap();
    body
}

async fn write_body(stream: &mut tokio::io::DuplexStream, body: &[u8]) {
    stream.write_u32_le(body.len() as u32).await.unwrap();
    stream.write_all(body).await.unwrap();
    stream.flush().await.unwrap();
}

#[tokio::test]
async fn acknowledged_wire_pipelines_requests_and_retires_both_directions_by_exact_receipt() {
    use p4_protocol::event::hop::{self, HopFrame, ReceiptStatus};
    let (client, mut server) = duplex(64 * 1024);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = EventWire::acknowledged(reader, writer, "outer-a".into(), 7).unwrap();
    let peer = tokio::spawn(async move {
        let Some(HopFrame::Hello { connection_generation, .. }) =
            hop::decode(&read_body(&mut server).await).unwrap() else { panic!("hello"); };
        write_body(&mut server, &hop::encode(&HopFrame::HelloAck {
            accepted_connection_generation: connection_generation, sender_id: "agent-a".into(),
            connection_generation: 9, max_outstanding: 2, max_receipt_bytes: 1024 * 1024,
        }).unwrap()).await;
        let mut requests = Vec::new();
        for _ in 0..2 {
            let Some(frame @ HopFrame::Data { .. }) = hop::decode(&read_body(&mut server).await).unwrap()
                else { panic!("data"); };
            requests.push(frame);
        }
        for frame in requests {
            let HopFrame::Data { attempt, digest, .. } = frame else { unreachable!() };
            write_body(&mut server, &hop::encode(&HopFrame::Receipt { attempt, digest,
                status: ReceiptStatus::AcceptedExact, detail: String::new() }).unwrap()).await;
        }
        let response = encode(&event(3)).unwrap();
        let digest = hop::event_digest(&response);
        write_body(&mut server, &hop::encode(&HopFrame::Data { attempt: 11, digest,
            event: response }).unwrap()).await;
        let mut request_acks = 0;
        let mut response_receipt = false;
        loop {
            match hop::decode(&read_body(&mut server).await).unwrap().unwrap() {
                HopFrame::ReceiptAck { .. } => request_acks += 1,
                HopFrame::Receipt { attempt: 11, digest: seen, status: ReceiptStatus::AcceptedExact, .. } => {
                    assert_eq!(seen, digest);
                    write_body(&mut server, &hop::encode(&HopFrame::ReceiptAck {
                        sender_id: "agent-a".into(), connection_generation: 9,
                        attempt: 11, digest }).unwrap()).await;
                    response_receipt = true;
                }
                other => panic!("unexpected {other:?}"),
            }
            if request_acks == 2 && response_receipt { break; }
        }
        assert_eq!(server.read_u32_le().await.unwrap(), 0);
        server.write_u32_le(0).await.unwrap();
    });
    wire.send(event(1)).await.unwrap();
    wire.send(event(2)).await.unwrap();
    assert_eq!(wire.receive(Instant::now() + Duration::from_secs(2)).await.unwrap(), event(3));
    wire.finish(Instant::now() + Duration::from_secs(2)).await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn old_peer_is_refused_after_hello_before_any_event_data() {
    let (client, mut server) = duplex(64 * 1024);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = EventWire::acknowledged(reader, writer, "outer-a".into(), 7).unwrap();
    let peer = tokio::spawn(async move {
        let hello = read_body(&mut server).await;
        assert_eq!(&hello[..4], b"P4H1");
        let legacy = encode(&event(99)).unwrap();
        write_body(&mut server, &legacy).await;
        let mut byte = [0u8; 1];
        let read = tokio::time::timeout(Duration::from_millis(50), server.read(&mut byte)).await;
        assert!(read.is_err() || matches!(read, Ok(Ok(0))), "no P4H1 Data may follow refusal");
    });
    assert!(wire.send(event(1)).await.unwrap_err().to_string().contains("does not support"));
    drop(wire);
    peer.await.unwrap();
}

#[tokio::test]
async fn explicit_finish_requires_split_ack_and_rejects_later_send() {
    let (client, mut server) = duplex(16);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = EventWire::new(reader, writer);
    let peer = tokio::spawn(async move {
        assert_eq!(server.read_u32_le().await.unwrap(), 0);
        server.write_all(&[0, 0]).await.unwrap();
        tokio::task::yield_now().await;
        server.write_all(&[0, 0]).await.unwrap();
    });
    wire.finish(Instant::now() + Duration::from_secs(2)).await.unwrap();
    assert!(wire.send(event(1)).await.is_err());
    peer.await.unwrap();
}

#[tokio::test]
async fn explicit_finish_rejects_eof_and_preserves_unexpected_output() {
    for bytes in [Vec::new(), frames(1)] {
        let (client, mut server) = duplex(4096);
        let (reader, writer) = tokio::io::split(client);
        let mut wire = EventWire::new(reader, writer);
        let expected = bytes.clone();
        let peer = tokio::spawn(async move {
            assert_eq!(server.read_u32_le().await.unwrap(), 0);
            server.write_all(&bytes).await.unwrap();
        });
        let error = wire.finish(Instant::now() + Duration::from_secs(2)).await.unwrap_err();
        assert_eq!(wire.buffer, expected, "unexpected output remains available to diagnose the failed close");
        if !wire.buffer.is_empty() {
            assert!(error.to_string().contains(&format!("frame_bytes={}", wire.buffer.len())));
        }
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn explicit_finish_preserves_partial_unexpected_frame_on_eof() {
    let bytes = frames(1);
    let split = bytes.len() / 2;
    let expected = bytes[..split].to_vec();
    let (client, mut server) = duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = EventWire::new(reader, writer);
    let peer = tokio::spawn(async move {
        assert_eq!(server.read_u32_le().await.unwrap(), 0);
        server.write_all(&bytes[..split]).await.unwrap();
    });
    let error = wire.finish(Instant::now() + Duration::from_secs(2)).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert_eq!(wire.buffer, expected);
    assert!(error.to_string().contains(&format!("buffered_bytes={split}")));
    peer.await.unwrap();
}

#[tokio::test]
async fn explicit_finish_preserves_partial_unexpected_frame_on_timeout() {
    let bytes = frames(1);
    let split = bytes.len() / 2;
    let expected = bytes[..split].to_vec();
    let (client, mut server) = duplex(4096);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = EventWire::new(reader, writer);
    let peer = tokio::spawn(async move {
        assert_eq!(server.read_u32_le().await.unwrap(), 0);
        server.write_all(&bytes[..split]).await.unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    let error = wire.finish(Instant::now() + Duration::from_millis(50)).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(wire.buffer, expected);
    assert!(error.to_string().contains(&format!("buffered_bytes={split}")));
    peer.abort();
}

#[tokio::test]
async fn explicit_finish_rejects_oversize_before_reading_a_body() {
    let prefix = ((super::MAX_FRAME as u32) + 1).to_le_bytes();
    let (client, mut server) = duplex(16);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = EventWire::new(reader, writer);
    let peer = tokio::spawn(async move {
        assert_eq!(server.read_u32_le().await.unwrap(), 0);
        server.write_all(&prefix).await.unwrap();
    });
    let error = wire.finish(Instant::now() + Duration::from_secs(2)).await.unwrap_err();
    assert!(error.to_string().contains("exceeds frame bound"));
    assert_eq!(wire.buffer, prefix);
    peer.await.unwrap();
}

fn outer() -> OuterEndpoint {
    OuterEndpoint {
        ingress_agent: Address::from_str("tcp://127.0.0.1:42003").expect("address"),
        channel: "wire".into(),
        connection_generation: 1,
    }
}

fn event(sequence: u64) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: format!("wire-{sequence}"),
            correlation_id: "request".into(),
            causation_id: None,
            source: Endpoint::Outer(outer()),
            target: Endpoint::Outer(outer()),
            return_route: Some(outer()),
            class: EventClass::Output,
            sequence,
            deadline_unix_ms: None,
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: "application/json".into(),
        },
        payload: vec![b'x'; 512],
    }
}

fn frames(count: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    for sequence in 1..=count {
        let body = encode(&event(sequence)).expect("encode");
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&body);
    }
    bytes
}

#[tokio::test]
async fn a_timeout_mid_frame_loses_nothing() {
    // The failure this guards: an unbuffered reader cancelled mid-frame takes
    // the bytes it had consumed with it, and every later frame is read from
    // the wrong offset - which the drive saw as a silent run of missing
    // output events rather than as an error.
    let (mut writer, reader) = duplex(64 * 1024);
    let (sink, sink_peer) = duplex(64);
    drop(sink_peer);
    let mut wire = EventWire::new(reader, sink);

    let bytes = frames(8);
    let split = bytes.len() / 3;
    writer.write_all(&bytes[..split]).await.expect("write head");

    let mut received = Vec::new();
    loop {
        match wire
            .receive(Instant::now() + Duration::from_millis(60))
            .await
        {
            Ok(event) => received.push(event.envelope.sequence),
            Err(error) if error.kind() == io::ErrorKind::TimedOut => break,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }
    assert!(!received.is_empty(), "some frames should have arrived");

    writer.write_all(&bytes[split..]).await.expect("write tail");
    while received.len() < 8 {
        let event = wire
            .receive(Instant::now() + Duration::from_secs(5))
            .await
            .expect("remaining frames");
        received.push(event.envelope.sequence);
    }
    assert_eq!(received, (1..=8).collect::<Vec<_>>());
}

#[tokio::test]
async fn repeated_timeouts_do_not_desync_the_stream() {
    // Small chunks with a short deadline after each: every frame boundary is
    // crossed under cancellation, which is the worst case of the wave loop.
    let (mut writer, reader) = duplex(64 * 1024);
    let (sink, sink_peer) = duplex(64);
    drop(sink_peer);
    let mut wire = EventWire::new(reader, sink);
    let bytes = frames(4);

    let feeder = tokio::spawn(async move {
        for chunk in bytes.chunks(7) {
            writer.write_all(chunk).await.expect("write chunk");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        writer
    });

    let mut received = Vec::new();
    while received.len() < 4 {
        match wire
            .receive(Instant::now() + Duration::from_millis(2))
            .await
        {
            Ok(event) => received.push(event.envelope.sequence),
            Err(error) if error.kind() == io::ErrorKind::TimedOut => continue,
            Err(error) => panic!("unexpected error: {error}"),
        }
    }
    assert_eq!(received, vec![1, 2, 3, 4]);
    feeder.await.expect("feeder");
}

#[tokio::test]
async fn an_ended_stream_is_an_error_not_a_silent_stop() {
    let (writer, reader) = duplex(1024);
    drop(writer);
    let (sink, sink_peer) = duplex(64);
    drop(sink_peer);
    let mut wire = EventWire::new(reader, sink);
    let error = wire
        .receive(Instant::now() + Duration::from_secs(1))
        .await
        .expect_err("ended stream");
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
}
