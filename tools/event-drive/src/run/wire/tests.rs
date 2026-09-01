//! The reader has to survive being timed out, because the drive times out on
//! purpose at every arrival wave.

use super::EventWire;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, OuterEndpoint, encode};
use std::io;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, duplex};

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
