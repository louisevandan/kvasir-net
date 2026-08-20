//! The three names, and the one place they differ.
//!
//! llama.cpp, vLLM and SGLang serve the same OpenAI-compatible HTTP, which is
//! why they are one adapter. Registering them is not enough to say they work:
//! what has to be checked is the difference that made three names necessary in
//! the first place.
//!
//! vLLM matches a request's `model` against what it is serving and answers 404
//! to anything else. A plan that names no model would therefore fail on the
//! first inference rather than on the load — and a load is where an operator is
//! still watching, so the failure is moved there by asking the server what it
//! holds. The other two are lenient and are not charged that round trip.
//!
//! The server here behaves like vLLM: it lists one model and refuses any other
//! name. Nothing about it is llama.cpp's or SGLang's, which is the point — the
//! adapter is checked against the behaviour, not against a brand.

use p4_adapter::{Adapter, Event, EventSink, Hop, Load, Phase, Sequence, Work};
use p4_llamacpp_served::Served;
use p4_llamacpp_served::flavour::Flavour;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;

#[derive(Default)]
struct Seen(Mutex<Vec<Event>>);

impl EventSink for Seen {
    fn raise(&self, event: Event) {
        self.0.lock().expect("seen").push(event);
    }
}

impl Seen {
    fn failure(&self) -> Option<String> {
        self.0
            .lock()
            .expect("seen")
            .iter()
            .find_map(|event| match event {
                Event::Failed { detail, .. } => Some(detail.clone()),
                _ => None,
            })
    }

    fn bound(&self) -> bool {
        self.0
            .lock()
            .expect("seen")
            .iter()
            .any(|event| matches!(event, Event::Loaded { .. }))
    }

    fn progress(&self) -> Vec<String> {
        self.0
            .lock()
            .expect("seen")
            .iter()
            .filter_map(|event| match event {
                Event::LoadProgress { detail, .. } => Some(detail.clone()),
                _ => None,
            })
            .collect()
    }

    fn tokens(&self) -> Vec<String> {
        self.0
            .lock()
            .expect("seen")
            .iter()
            .flat_map(|event| match event {
                Event::HopComplete { outcomes, .. } => {
                    outcomes.iter().map(|o| o.text.clone()).collect()
                }
                _ => Vec::new(),
            })
            .collect()
    }
}

/// Lists one model, and refuses a completion asking for any other — which is
/// what vLLM does, and what the resolution exists for.
fn strict_server(serving: &'static str, lists: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || answer(stream, serving, lists));
        }
    });
    port
}

fn answer(mut stream: TcpStream, serving: &str, lists: bool) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request = String::new();
    reader.read_line(&mut request).ok();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        if line.trim().is_empty() {
            break;
        }
    }
    let mut body = vec![0u8; length];
    if length > 0 {
        use std::io::Read;
        reader.read_exact(&mut body).ok();
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    if request.starts_with("GET /v1/models") {
        let listing = if lists {
            format!(r#"{{"object":"list","data":[{{"id":"{serving}","object":"model"}}]}}"#)
        } else {
            r#"{"object":"list","data":[]}"#.to_owned()
        };
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{listing}",
            listing.len()
        );
        return;
    }

    // The refusal vLLM gives, in vLLM's own shape.
    if !body.contains(&format!("\"model\":\"{serving}\"")) {
        let refusal = r#"{"object":"error","message":"The model does not exist.","code":404}"#;
        let _ = write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{refusal}",
            refusal.len()
        );
        return;
    }

    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n"
    );
    for chunk in [
        r#"{"choices":[{"delta":{"content":"served"},"finish_reason":null}]}"#,
        r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
    ] {
        let _ = write!(stream, "data: {chunk}\n\n");
    }
    let _ = write!(stream, "data: [DONE]\n\n");
    let _ = stream.flush();
}

fn load(adapter: &Served, seen: &Seen, plan: String) {
    adapter.start(
        Work::Load(Load {
            deployment: "d".into(),
            plan,
            artifact: "model".into(),
            capability_snapshot_id: String::new(),
            capability_expires_at: 0,
        }),
        seen,
    );
}

fn one_hop(adapter: &Served, seen: &Seen) {
    adapter.start(
        Work::Hop(Hop {
            id: 1,
            deployment: "d".into(),
            phase: Phase::Prefill,
            sequences: vec![Sequence {
                sequence: "s0".into(),
                state: None,
                prompt: Some("hello".into()),
                remaining: 4,
                options: "{}".into(),
            }],
        }),
        seen,
    );
}

/// The difference, working: a plan that names no model is served anyway,
/// because the load asked.
#[test]
fn a_vllm_load_finds_out_what_the_server_is_serving() {
    let port = strict_server("Qwen/Qwen3-32B", true);
    let adapter = Served::new(Flavour::Vllm);
    let seen = Seen::default();
    load(
        &adapter,
        &seen,
        format!(r#"{{"endpoint":"127.0.0.1:{port}"}}"#),
    );

    assert!(seen.bound(), "bound: {:?}", seen.failure());
    assert!(
        seen.progress()
            .iter()
            .any(|line| line.contains("Qwen/Qwen3-32B") && line.contains("did not name")),
        "the load says which name it adopted: {:?}",
        seen.progress()
    );

    one_hop(&adapter, &seen);
    assert_eq!(
        seen.tokens(),
        vec!["served".to_owned()],
        "the request used the name the server serves: {:?}",
        seen.failure()
    );
}

/// A plan that does name one is left alone. Resolution is a fallback, not a
/// correction — an operator naming a model on a server holding several is
/// making a choice, and overwriting it would be the adapter deciding placement.
#[test]
fn a_named_model_is_not_replaced_by_what_the_server_lists() {
    let port = strict_server("chosen", true);
    let adapter = Served::new(Flavour::Vllm);
    let seen = Seen::default();
    load(
        &adapter,
        &seen,
        format!(r#"{{"endpoint":"127.0.0.1:{port}","model":"chosen"}}"#),
    );

    assert!(seen.bound(), "bound: {:?}", seen.failure());
    assert!(
        !seen
            .progress()
            .iter()
            .any(|line| line.contains("did not name")),
        "nothing was resolved: {:?}",
        seen.progress()
    );
    one_hop(&adapter, &seen);
    assert_eq!(seen.tokens(), vec!["served".to_owned()]);
}

/// A server holding nothing is a load failure rather than an inference one.
///
/// Refusing here is the whole reason the round trip exists: the alternative is
/// a deployment that binds, reports itself healthy, and refuses every request
/// afterwards for a model that does not exist.
#[test]
fn a_vllm_server_listing_nothing_refuses_the_load() {
    let port = strict_server("unused", false);
    let seen = Seen::default();
    load(
        &Served::new(Flavour::Vllm),
        &seen,
        format!(r#"{{"endpoint":"127.0.0.1:{port}"}}"#),
    );

    assert!(
        !seen.bound(),
        "a deployment that cannot serve must not bind"
    );
    let detail = seen.failure().unwrap_or_default();
    assert!(detail.contains("lists no model"), "{detail}");
}

/// The lenient two are not charged for the strict one's rule.
///
/// llama.cpp and SGLang answer to any name, so their loads do not ask and their
/// plans do not have to say. Sending them through the resolution anyway would
/// hide the difference rather than state it, and would fail them against a
/// server that lists nothing while serving perfectly well.
#[test]
fn llamacpp_and_sglang_bind_against_a_server_that_lists_nothing() {
    for flavour in [Flavour::LlamaCpp, Flavour::Sglang] {
        let port = strict_server("default", false);
        let seen = Seen::default();
        load(
            &Served::new(flavour),
            &seen,
            format!(r#"{{"endpoint":"127.0.0.1:{port}"}}"#),
        );
        assert!(
            seen.bound(),
            "{} bound: {:?}",
            flavour.name(),
            seen.failure()
        );
    }
}
