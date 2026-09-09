//! The whole CLI path, over a real socket: `execute` -> teardown -> artifact
//! file -> exit code.
//!
//! The in-crate tests fix what a broken run keeps. They cannot fix that the
//! caller ever sees it: `main` writes the file and chooses the exit code, and
//! the four 2026-09-09 failures produced no file at all. This spawns the built
//! binary against a scripted peer on a local TCP listener, so the assertions
//! are about the artifact on disk and the process's status.
//!
//! The peer is a protocol fixture, not a node: it answers CREATE, LOAD and
//! SESSION, then reports an inference error and closes the connection so the
//! teardown that follows also fails. It runs no model and approves no output.

use p4_llamacpp_staged_adapter::v2::{
    ERROR_CONTENT_TYPE, LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE, PREFILL_CONTENT_TYPE,
    SESSION_CONTENT_TYPE, SESSION_READY_CONTENT_TYPE,
};
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, decode, encode};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;

const CREATE: &str = "application/vnd.p4.node.create-v3+json";
const NODE_RESULT: &str = "application/vnd.p4.node.result-v3+json";
const NODE_ERROR: &str = "stage refused decode: sequence 41 has no resident slot";

fn read_event(stream: &mut TcpStream) -> Option<Event> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).ok()?;
    let mut body = vec![0u8; u32::from_le_bytes(header) as usize];
    stream.read_exact(&mut body).ok()?;
    Some(decode(&body).expect("the drive sends decodable frames"))
}

fn write_event(stream: &mut TcpStream, event: &Event) {
    let bytes = encode(event).expect("reply encodes");
    stream
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .expect("length");
    stream.write_all(&bytes).expect("frame");
    stream.flush().expect("flush");
}

/// A reply that the drive's own matching accepts: same correlation, caused by
/// the request, and sourced from the endpoint the request was addressed to.
fn reply(request: &Event, content_type: &str, payload: Vec<u8>, serial: u64) -> Event {
    let outer = request
        .envelope
        .return_route
        .clone()
        .expect("the drive routes its replies back to itself");
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: format!("peer-{serial}"),
            correlation_id: request.envelope.correlation_id.clone(),
            causation_id: Some(request.envelope.event_id.clone()),
            source: request.envelope.target.clone(),
            target: Endpoint::Outer(outer.clone()),
            return_route: Some(outer),
            class: EventClass::Control,
            sequence: serial,
            deadline_unix_ms: None,
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: content_type.into(),
        },
        payload,
    }
}

fn config(port: u16, requests: usize) -> serde_json::Value {
    let agent = format!("tcp://127.0.0.1:{port}");
    let node = |name: &str, endpoint: u16| {
        serde_json::json!({
            "agent": agent, "node": name, "generation": 3,
            "binary": "not-started", "endpoint": format!("tcp://127.0.0.1:{endpoint}"),
            "plan": "not-loaded", "args": [], "environment": [],
            "n_batch": 32, "n_ubatch": 32, "context_size": 128,
            "total_context_size": 256, "sequence_capacity": 2,
        })
    };
    serde_json::json!({
        "ingress_agent": agent,
        "channel": "cli-artifact-test",
        "connection_generation": 7,
        "load_generation": 9,
        "session_id": "session",
        "request_id": "request",
        "nodes": [node("head", 52502), node("tail", 52503)],
        "prompt": "타입스크립트에 대해 한국어로 설명하라",
        "max_tokens": 4,
        "waves": [{ "after_ms": 0, "count": requests }],
        "pre_inference_hold_ms": 0,
        "timeout_ms": 10_000,
        "acceptance": { "minimum_generated_tokens": 1 },
    })
}

/// Answers the handshake, then fails the inference and drops the connection.
fn serve(listener: TcpListener, requests: usize) {
    let (mut stream, _) = listener.accept().expect("the drive connects");
    let mut serial = 1;
    let mut submitted = 0;
    while let Some(event) = read_event(&mut stream) {
        let content_type = event.envelope.payload_content_type.clone();
        let answer = match content_type.as_str() {
            CREATE => Some((NODE_RESULT, serde_json::json!({ "ok": true }))),
            LOAD_CONTENT_TYPE => Some((
                LOADED_CONTENT_TYPE,
                serde_json::json!({
                    "upstream_commit": "0eadefebd3f8f92a86d634a0e5b8fffc9dc792c0",
                    "patch_set": "961bd89cd1197cef0d683f22d99d9451e25aba5eb",
                    "backend_inventory": "CPU[CPU]",
                }),
            )),
            SESSION_CONTENT_TYPE => Some((SESSION_READY_CONTENT_TYPE, serde_json::json!({}))),
            PREFILL_CONTENT_TYPE => {
                submitted += 1;
                // Every request is accepted first, so the failure lands on a
                // run that has something to lose.
                if submitted == requests {
                    let mut error = reply(&event, ERROR_CONTENT_TYPE, NODE_ERROR.into(), serial);
                    error.envelope.class = EventClass::Output;
                    write_event(&mut stream, &error);
                    // Dropping the socket makes the teardown that follows fail
                    // too, which is the pair the artifact must keep apart.
                    return;
                }
                None
            }
            other => panic!("the peer was not scripted for {other}"),
        };
        if let Some((content_type, payload)) = answer {
            let payload = serde_json::to_vec(&payload).expect("payload encodes");
            write_event(&mut stream, &reply(&event, content_type, payload, serial));
            serial += 1;
        }
    }
}

#[test]
fn a_run_that_breaks_and_then_fails_teardown_still_writes_its_artifact_and_exits_nonzero() {
    let requests = 2;
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let port = listener.local_addr().unwrap().port();
    let peer = std::thread::spawn(move || serve(listener, requests));

    let directory = std::env::temp_dir().join(format!("p4-cli-artifact-{port}"));
    std::fs::create_dir_all(&directory).expect("scratch directory");
    let config_path = directory.join("config.json");
    let artifact_path = directory.join("artifact.json");
    let _ = std::fs::remove_file(&artifact_path);
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&config(port, requests)).unwrap(),
    )
    .expect("config");

    let output = Command::new(env!("CARGO_BIN_EXE_p4-event-drive"))
        .arg(&config_path)
        .arg(&artifact_path)
        .output()
        .expect("the drive binary runs");
    peer.join().expect("the peer finishes");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a broken run must exit nonzero: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let written = std::fs::read(&artifact_path).unwrap_or_else(|error| {
        panic!(
            "the artifact must exist, which is the whole point: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let artifact: serde_json::Value = serde_json::from_slice(&written).expect("valid JSON");

    assert_eq!(artifact["passed"], serde_json::json!(false));
    assert_eq!(
        artifact["error"],
        serde_json::json!(NODE_ERROR),
        "the run's own first failure is what it is judged on"
    );
    assert!(
        artifact["cleanup_error"].is_string(),
        "the teardown failed too, in its own field: {}",
        artifact["cleanup_error"]
    );
    assert_ne!(
        artifact["error"], artifact["cleanup_error"],
        "one message must never stand in for the other"
    );
    assert_eq!(
        artifact["submissions"],
        serde_json::json!({
            "configured": requests,
            "delivered": requests,
            "uncertain": 0,
            "unsubmitted": 0,
            "incomplete": requests,
            "unreleased": requests,
        }),
        "the artifact says where every request got to"
    );
    assert_eq!(artifact["request_count"], serde_json::json!(requests));
    assert_eq!(
        artifact["requests"]
            .as_array()
            .expect("the requests it did submit are in the artifact")
            .len(),
        requests
    );
    assert!(
        artifact["evidence_missing"].is_object(),
        "no observation arrived, so the row counts are unattributed and this says so"
    );
    let _ = std::fs::remove_dir_all(&directory);
}
