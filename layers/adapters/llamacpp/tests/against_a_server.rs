//! The adapter against something that answers like `llama-server`.
//!
//! A stub rather than the real binary, so this runs anywhere and in a second —
//! but a stub of the *wire*, not of the adapter: a real socket, real HTTP,
//! real chunked framing, real server-sent events. Everything between
//! `Adapter::start` and the bytes is exercised.
//!
//! What a stub cannot prove is that a real llama.cpp answers this shape. That
//! is checked separately against the Metal build on the fleet and recorded in
//! `docs/runtime-evidence.md`; this file is what keeps it honest afterwards.

use p4_adapter::{Adapter, Distribution, Event, EventSink, Hop, Load, Phase, Sequence, Work};
use p4_llamacpp::LlamaCpp;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;

/// Collects what the adapter reported.
#[derive(Default)]
struct Seen(Mutex<Vec<Event>>);

impl EventSink for Seen {
    fn raise(&self, event: Event) {
        self.0.lock().expect("events").push(event);
    }
}

impl Seen {
    fn events(&self) -> Vec<Event> {
        self.0.lock().expect("events").clone()
    }

    fn text(&self) -> String {
        self.events()
            .iter()
            .filter_map(|event| match event {
                Event::HopComplete { outcomes, .. } => Some(
                    outcomes
                        .iter()
                        .map(|outcome| outcome.text.clone())
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect()
    }

    fn failure(&self) -> Option<String> {
        self.events().iter().find_map(|event| match event {
            Event::Failed { detail, .. } => Some(detail.clone()),
            _ => None,
        })
    }
}

/// What the stub should do when asked for a completion.
#[derive(Clone, Copy)]
enum Answer {
    /// Stream these tokens, then finish.
    Tokens(&'static [&'static str]),
    /// Report an error inside a 200 stream, which is what a real server does
    /// when a context is full.
    ErrorMidStream,
    /// Close the socket partway, which is what a server being killed does.
    HangUp,
}

/// A server that speaks the contract, on a port it chooses.
fn stub(answer: Answer) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || serve(stream, answer));
        }
    });
    port
}

fn serve(mut stream: TcpStream, answer: Answer) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request = String::new();
    reader.read_line(&mut request).ok();
    // Drain headers and any body; the stub does not need to read them, only to
    // get past them.
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let lowered = line.to_ascii_lowercase();
        if let Some(value) = lowered.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        if line.trim().is_empty() {
            break;
        }
    }
    if length > 0 {
        let mut body = vec![0u8; length];
        use std::io::Read;
        reader.read_exact(&mut body).ok();
    }

    if request.starts_with("GET /v1/models") {
        let body = r#"{"data":[{"id":"qwen"}]}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        return;
    }

    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
    );
    let send = |stream: &mut TcpStream, payload: &str| {
        let event = format!("data: {payload}\n\n");
        let _ = write!(stream, "{:x}\r\n{event}\r\n", event.len());
        let _ = stream.flush();
    };
    match answer {
        Answer::Tokens(tokens) => {
            for token in tokens {
                send(
                    &mut stream,
                    &format!(r#"{{"choices":[{{"delta":{{"content":"{token}"}}}}]}}"#),
                );
            }
            send(
                &mut stream,
                r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
            );
            send(&mut stream, "[DONE]");
            let _ = stream.write_all(b"0\r\n\r\n");
        }
        Answer::ErrorMidStream => {
            send(
                &mut stream,
                r#"{"choices":[{"delta":{"content":"partial"}}]}"#,
            );
            send(&mut stream, r#"{"error":{"message":"context is full"}}"#);
            let _ = stream.write_all(b"0\r\n\r\n");
        }
        Answer::HangUp => {
            send(&mut stream, r#"{"choices":[{"delta":{"content":"half"}}]}"#);
            // Drop without the terminating chunk.
        }
    }
}

fn load(adapter: &LlamaCpp, seen: &Seen, port: u16) {
    adapter.start(
        Work::Load(Load {
            deployment: "d1".into(),
            plan: format!(r#"{{"endpoint":"127.0.0.1:{port}","model":"qwen","patience_ms":4000}}"#),
            artifact: "model.gguf".into(),
        }),
        seen,
    );
}

fn sequence(remaining: u32, prompt: Option<&str>) -> Sequence {
    Sequence {
        sequence: "req-1".into(),
        position: 0,
        prompt: prompt.map(str::to_owned),
        remaining,
        options: r#"{"temperature":0}"#.into(),
    }
}

fn hop(phase: Phase, remaining: u32, prompt: Option<&str>) -> Work {
    Work::Hop(Hop {
        deployment: "d1".into(),
        phase,
        sequences: vec![sequence(remaining, prompt)],
    })
}

#[test]
fn a_load_reaches_the_backend_and_binds() {
    let port = stub(Answer::Tokens(&["a"]));
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    load(&adapter, &seen, port);

    assert_eq!(adapter.distribution(), Distribution::Internal);
    assert!(
        seen.events()
            .iter()
            .any(|event| matches!(event, Event::LoadProgress { .. })),
        "the load reported its progress"
    );
    assert!(
        seen.events()
            .iter()
            .any(|event| matches!(event, Event::Loaded { generation: 1, .. })),
        "and bound: {:?}",
        seen.events()
    );
}

/// The whole of it: prefill opens the stream, each lap takes one token, and
/// the last hop carries the stop.
///
/// This is the mapping the adapter exists to make — a backend that streams a
/// whole completion, driven by a ring that expects one token per lap.
#[test]
fn a_prefill_then_laps_produce_one_token_each_and_then_stop() {
    let port = stub(Answer::Tokens(&["안", "녕", "하", "세", "요"]));
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    load(&adapter, &seen, port);

    adapter.start(hop(Phase::Prefill, 5, Some("인사해")), &seen);
    for _ in 0..5 {
        adapter.start(hop(Phase::Decode, 5, None), &seen);
    }

    assert_eq!(seen.text(), "안녕하세요", "every token arrived, in order");
    let stops: Vec<String> = seen
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::HopComplete { outcomes, .. } => outcomes.iter().find_map(|o| o.stop.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(stops, vec!["stop"], "and exactly one hop carried the stop");
    assert_eq!(seen.failure(), None, "with nothing reported failed");

    // Each token says where in the stream it belongs, and the backend is the
    // only thing that knows: the request does not carry its progress back
    // down, so passing through the position the node handed in made every
    // token of an answer claim the same place. A fleet run caught that and
    // this did not, which is what this assertion is for.
    let positions: Vec<u32> = seen
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::HopComplete { outcomes, .. } => outcomes
                .iter()
                .find(|outcome| !outcome.text.is_empty())
                .map(|outcome| outcome.position),
            _ => None,
        })
        .collect();
    assert_eq!(
        positions,
        vec![1, 2, 3, 4, 5],
        "positions counted up across the stream"
    );
}

/// A hop before the sequence has prefilled.
///
/// It has no stream, and inventing one would silently start the conversation
/// again from an empty prompt.
#[test]
fn a_decode_for_a_sequence_that_never_prefilled_is_refused() {
    let port = stub(Answer::Tokens(&["x"]));
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    load(&adapter, &seen, port);

    adapter.start(hop(Phase::Decode, 4, None), &seen);
    assert!(
        seen.failure()
            .unwrap_or_default()
            .contains("never prefilled"),
        "{:?}",
        seen.events()
    );
}

/// An error arriving inside a 200 stream, which is how a real server reports a
/// full context.
#[test]
fn an_error_inside_the_stream_is_reported_rather_than_read_as_silence() {
    let port = stub(Answer::ErrorMidStream);
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    load(&adapter, &seen, port);

    adapter.start(hop(Phase::Prefill, 8, Some("긴 프롬프트")), &seen);
    adapter.start(hop(Phase::Decode, 8, None), &seen);

    assert!(
        seen.failure()
            .unwrap_or_default()
            .contains("context is full"),
        "the backend's own words reached the caller: {:?}",
        seen.events()
    );
}

/// The backend dies mid-answer.
///
/// Reported as an ending rather than waited on: a route whose terminal never
/// comes is what a leak looks like from outside.
#[test]
fn a_backend_that_hangs_up_ends_the_sequence_instead_of_stranding_it() {
    let port = stub(Answer::HangUp);
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    load(&adapter, &seen, port);

    adapter.start(hop(Phase::Prefill, 8, Some("hello")), &seen);
    for _ in 0..2 {
        adapter.start(hop(Phase::Decode, 8, None), &seen);
    }

    let terminal = seen.events().iter().any(|event| match event {
        Event::HopComplete { outcomes, .. } => outcomes.iter().any(|o| o.stop.is_some()),
        Event::Failed { .. } => true,
        _ => false,
    });
    assert!(terminal, "the sequence ended somehow: {:?}", seen.events());
}

/// A hop before any load.
#[test]
fn work_on_a_node_that_was_never_loaded_says_so() {
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    adapter.start(hop(Phase::Prefill, 4, Some("hi")), &seen);
    assert!(
        seen.failure().unwrap_or_default().contains("never loaded"),
        "{:?}",
        seen.events()
    );
}

/// A backend that is not there is a load failure, reported where a caller is
/// waiting rather than at the first inference.
#[test]
fn a_backend_that_is_not_there_fails_the_load() {
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    adapter.start(
        Work::Load(Load {
            deployment: "d1".into(),
            // Nothing listens here.
            plan: r#"{"endpoint":"127.0.0.1:1","model":"qwen"}"#.into(),
            artifact: "model.gguf".into(),
        }),
        &seen,
    );
    assert!(
        seen.failure().unwrap_or_default().contains("not reachable"),
        "{:?}",
        seen.events()
    );
}

/// The cache verbs exist in the protocol and this surface cannot honour them.
///
/// Said plainly, because a caller must be able to tell "not here" from "done"
/// or it will believe a conversation was saved.
#[test]
fn cache_work_is_refused_by_name_rather_than_silently_ignored() {
    let port = stub(Answer::Tokens(&["a"]));
    let adapter = LlamaCpp::new();
    let seen = Seen::default();
    load(&adapter, &seen, port);

    adapter.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d1".into(),
            sequence: "req-1".into(),
            action: p4_adapter::CacheAction::Persist,
        }),
        &seen,
    );
    assert!(
        seen.failure()
            .unwrap_or_default()
            .contains("cannot persist"),
        "{:?}",
        seen.events()
    );
}
