//! Actual Worker::run admission boundary, with the existing independent native
//! KV/token fake. Literal JSON bytes define the oracle, not a production size
//! validator. No native/parser limit is widened. Options are preserved as bytes;
//! successful fake execution does not prove actual llama option parsing.
//! A source freeze covers both these consumers and the codecs they actually use.
use super::*;
use sha2::{Digest, Sha256};

const WIRE_LIMIT: usize = 4096;
const REPLY_PREFIX: &str = "{\"ingress_agent\":\"tcp://127.0.0.1:42999\",\"channel\":\"loop-output\",\"connection_generation\":1,\"correlation_id\":";
const REPLY_SUFFIX: &str = ",\"deadline_unix_ms\":null}";

#[derive(Clone, Copy, Debug)]
enum ReplyText {
    Ascii,
    Escaped,
    Unicode,
}

#[derive(Clone, Copy, Debug)]
enum LimitedField {
    Reply(ReplyText),
    Options,
}

fn literal_reply(correlation_json: &str) -> String {
    format!("{REPLY_PREFIX}{correlation_json}{REPLY_SUFFIX}")
}

fn sized_correlation(kind: ReplyText, byte_len: usize) -> (String, String) {
    let overhead = literal_reply("\"\"").len();
    let room = byte_len.checked_sub(overhead).unwrap();
    // Escaped contains a quote, a backslash and an actual newline: 3 input
    // bytes, 6 serialized bytes. Unicode is one scalar but 3 UTF-8 bytes.
    let (raw_unit, json_unit) = match kind {
        ReplyText::Ascii => ("a", "a"),
        ReplyText::Escaped => ("\"\\\n", "\\\"\\\\\\n"),
        ReplyText::Unicode => ("한", "한"),
    };
    let repeats = room / json_unit.len();
    let rest = "x".repeat(room % json_unit.len());
    let correlation = format!("{}{rest}", raw_unit.repeat(repeats));
    let json = format!("\"{}{rest}\"", json_unit.repeat(repeats));
    assert_eq!(serde_json::from_str::<String>(&json).unwrap(), correlation);
    assert_eq!(serde_json::to_string(&correlation).unwrap(), json);
    let reply = literal_reply(&json);
    assert_eq!(reply.len(), byte_len);
    match kind {
        ReplyText::Escaped => assert!(correlation.len() < room),
        ReplyText::Unicode => assert!(correlation.chars().count() < correlation.len()),
        ReplyText::Ascii => assert_eq!(correlation.len(), correlation.chars().count()),
    }
    (correlation, reply)
}

struct InputCase {
    event: Event,
    command: InferenceCommand,
    reply: String,
    options: String,
}

fn input_case(field: LimitedField, byte_len: usize, prompt: bool) -> InputCase {
    let mut command = request("row-limit-request", 1, 3);
    command.session_key = Some("sk1:loop/boundary-input".into());
    if prompt {
        command.tokens.clear();
        command.prompt = Some("Explain how independent requests preserve their state.".into());
    }
    let (correlation, reply) = match field {
        LimitedField::Reply(kind) => sized_correlation(kind, byte_len),
        LimitedField::Options => {
            // Legal JSON whitespace preserves the same declared object. The
            // wire field is the original string, not its trimmed/parsed form.
            const JSON: &str = "{\"seed\":1}";
            command.options = format!("{JSON}{}", " ".repeat(byte_len - JSON.len()));
            assert_eq!(command.options.len(), byte_len);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&command.options).unwrap(),
                serde_json::json!({"seed": 1})
            );
            let correlation = "separate-valid-correlation".to_owned();
            let reply = literal_reply("\"separate-valid-correlation\"");
            (correlation, reply)
        }
    };
    command.validate().unwrap();
    let options = command.options.clone();
    let mut event = submission_event(&command, 1, default_route());
    event.envelope.correlation_id = correlation;
    let actual_serialized_reply = serde_json::to_string(&ReplySpec {
        ingress_agent: "tcp://127.0.0.1:42999".into(),
        channel: "loop-output".into(),
        connection_generation: 1,
        correlation_id: event.envelope.correlation_id.clone(),
        deadline_unix_ms: None,
    })
    .unwrap();
    assert_eq!(
        actual_serialized_reply, reply,
        "literal JSON matches current serialization without using the size validator"
    );
    event.validate().unwrap();
    let wire = p4_protocol::event::encode(&event).unwrap();
    assert_eq!(p4_protocol::event::decode(&wire).unwrap(), event);
    eprintln!(
        "ROW_STRING_BOUNDARY field={field:?} target_bytes={byte_len} prompt={prompt} correlation_bytes={} correlation_scalars={} reply_bytes={} options_bytes={} original_payload_bytes={} event_wire_bytes={} event_wire_sha256={:x}",
        event.envelope.correlation_id.len(),
        event.envelope.correlation_id.chars().count(),
        reply.len(),
        options.len(),
        event.payload.len(),
        wire.len(),
        Sha256::digest(&wire)
    );
    InputCase {
        event,
        command,
        reply,
        options,
    }
}

fn expected_tokenized(mut command: InferenceCommand) -> InferenceCommand {
    if command.prompt.take().is_some() {
        // This is the fake Tokenize's declared one-token output, not a real
        // tokenizer result and not a value inferred from generated OUTPUTs.
        command.tokens = vec![10];
    }
    command
}

fn assert_native_strings(h: &Harness, expected_reply: &str, expected_options: &str) {
    let native = h.nodes[0].native.lock().unwrap();
    assert_eq!(
        native.issued_native.len(),
        3,
        "one prefill and two decode native calls"
    );
    for record in &native.issued_native {
        let logical = LogicalBatch::decode(&record.input).unwrap();
        assert_eq!(logical.0.len(), 1);
        for row in logical.0 {
            assert_eq!(row.owner.reply, expected_reply);
            assert_eq!(row.owner.options, expected_options);
        }
        let actual_result = CapsuleSet::decode(record.result.as_ref().unwrap()).unwrap();
        for capsule in actual_result.0 {
            for owner in capsule.owners {
                assert_eq!(owner.reply, expected_reply);
                assert_eq!(owner.options, expected_options);
            }
        }
    }
    drop(native);
    let mut delivered = 0;
    for event in &h.stage_events {
        if event.envelope.payload_content_type == PHYSICAL_BATCH_CONTENT_TYPE {
            delivered += 1;
            for capsule in CapsuleSet::decode(&event.payload).unwrap().0 {
                for owner in capsule.owners {
                    assert_eq!(owner.reply, expected_reply);
                    assert_eq!(owner.options, expected_options);
                }
            }
        }
    }
    assert_eq!(
        delivered, 3,
        "all three accepted native calls traverse the next stage"
    );
}

fn legal_boundary(field: LimitedField, byte_len: usize, prompt: bool) {
    assert!(byte_len <= WIRE_LIMIT);
    let case = input_case(field, byte_len, prompt);
    let expected = expected_tokenized(case.command);
    let mut h = Harness::configured_events(2, 1, 8, std::slice::from_ref(&case.event), 0, None);
    h.finish(std::slice::from_ref(&expected));
    release_notifications::assert_complete(&h);
    assert_native_strings(&h, &case.reply, &case.options);
    assert_eq!(
        h.submissions,
        vec![case.event],
        "the original prompt/tokens envelope is not rewritten by the fixture"
    );
    assert!(
        !h.received
            .iter()
            .any(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE)
    );
    for (index, node) in h.nodes.iter().enumerate() {
        let native = node.native.lock().unwrap();
        assert_eq!(native.tokenize_calls, usize::from(prompt && index == 0));
        assert_eq!(native.shutdowns, 0);
        assert!(
            native
                .written
                .keys()
                .all(|(slot, _, incarnation)| *slot == 0 && *incarnation == 1)
        );
    }
}

#[derive(Clone, Debug)]
struct Admission {
    point: &'static str,
    next_incarnation: u64,
    pending: Vec<String>,
    free: Vec<u32>,
    requests: Vec<(String, Option<u32>, u64, bool)>,
    session_keys: Vec<((u64, String, String), Option<String>)>,
}

fn observer() -> (IssueObserver, Arc<Mutex<Vec<Admission>>>) {
    let views = Arc::new(Mutex::new(Vec::new()));
    let target = Arc::clone(&views);
    let callback: IssueObserver = Arc::new(move |point, state| {
        let value = Admission {
            point,
            next_incarnation: state.next_incarnation,
            pending: state.pending.iter().cloned().collect(),
            free: state.free_sequences.iter().copied().collect(),
            requests: state
                .requests
                .values()
                .map(|request| {
                    (
                        request.command.request_id.clone(),
                        request.sequence_id,
                        request.incarnation,
                        request.issued_work.is_some(),
                    )
                })
                .collect(),
            session_keys: state
                .session_keys
                .iter()
                .map(|(scope, key)| (scope.clone(), key.clone()))
                .collect(),
        };
        let mut target = target.lock().unwrap();
        assert!(target.len() < 128, "test-only bounded read observation");
        target.push(value);
    });
    (callback, views)
}

fn above_limit_rejects_and_recovers(field: LimitedField, prompt: bool) {
    let case = input_case(field, WIRE_LIMIT + 1, prompt);
    let (observer, views) = observer();
    let mut h = Harness::observed_events(2, 1, 8, &[], 0, None, Some(observer), None);
    h.expected_submission_error = Some(case.event.clone());
    h.pending.push_back(case.event.clone());
    h.next_submission = 2;
    h.until(
        "oversized row string yields an explicit submission error",
        |h| {
            h.received
                .iter()
                .any(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE)
        },
    );
    let errors: Vec<_> = h
        .received
        .iter()
        .filter(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE)
        .collect();
    assert_eq!(errors.len(), 1);
    let error = errors[0].clone();
    let body: serde_json::Value = serde_json::from_slice(&error.payload).unwrap();
    // Allow only bounded evidence collection of the old fatal path. The
    // assertion below still insists on nonfatal admission rejection.
    if body["code"] != "LLAMA_ADAPTER_EVENT_REJECTED" {
        h.until("late rejection exposes its real worker termination", |h| {
            h.nodes[0].thread.as_ref().unwrap().is_finished()
        });
    }
    {
        let native = h.nodes[0].native.lock().unwrap();
        eprintln!(
            "ROW_STRING_REJECTION field={field:?} prompt={prompt} error={body} tokenize={} logical={} physical={} writes={:?} shutdown={} admission={:?} snapshot={}",
            native.tokenize_calls,
            native.logical_calls,
            native.physical_calls,
            native.written,
            native.shutdowns,
            views.lock().unwrap(),
            h.nodes[0].snapshot.lock().unwrap()
        );
    }
    assert_eq!(error.envelope.source, case.event.envelope.target);
    assert_eq!(error.envelope.target, case.event.envelope.source);
    assert_eq!(
        error.envelope.return_route,
        case.event.envelope.return_route
    );
    assert_eq!(
        error.envelope.correlation_id,
        case.event.envelope.correlation_id
    );
    assert_eq!(
        error.envelope.causation_id.as_deref(),
        Some(case.event.envelope.event_id.as_str())
    );
    assert_eq!(
        body["code"], "LLAMA_ADAPTER_EVENT_REJECTED",
        "row byte limit must reject at ingress, not terminate the admitted worker in logical encoding"
    );
    assert_eq!(
        body["detail"],
        match field {
            LimitedField::Reply(_) => "serialized reply exceeds row wire limit or is empty",
            LimitedField::Options => "request options exceed row wire limit",
        }
    );
    assert!(
        views.lock().unwrap().is_empty(),
        "invalid input must not reach issue or stopping hooks"
    );
    assert!(h.stage_events.is_empty());
    assert!(h.outputs.is_empty());
    for node in &h.nodes {
        let native = node.native.lock().unwrap();
        assert_eq!(native.tokenize_calls, 0);
        assert_eq!(native.logical_calls, 0);
        assert_eq!(native.physical_calls, 0);
        assert_eq!(native.sampler_calls, 0);
        assert_eq!(native.shutdowns, 0);
        assert!(native.live.is_empty());
        assert!(native.written.is_empty());
        assert!(native.releases.is_empty());
        assert!(native.release_bodies.is_empty());
    }
    h.expected_submission_error = None;
    let mut good = request("row-limit-request", 1, 3);
    good.session_key = Some("sk1:loop/accepted-after-limit".into());
    h.enqueue(&good);
    h.finish(std::slice::from_ref(&good));
    release_notifications::assert_complete(&h);
    let views = views.lock().unwrap();
    let first = views
        .iter()
        .find(|view| view.point == "before_native_issue")
        .unwrap();
    assert_eq!(first.next_incarnation, 2);
    assert!(first.pending.is_empty());
    assert_eq!(first.free, (1..SEQUENCE_CAPACITY).collect::<Vec<_>>());
    assert_eq!(
        first.requests,
        vec![(good.request_id.clone(), Some(0), 1, false)]
    );
    assert_eq!(
        first.session_keys,
        vec![(
            (1, "loop-session".into(), good.request_id.clone()),
            good.session_key.clone()
        )]
    );
    assert!(!views.iter().any(|view| view.point == "run_stopping"));
    assert_eq!(
        h.received
            .iter()
            .filter(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE)
            .count(),
        1
    );
    for node in &h.nodes {
        let native = node.native.lock().unwrap();
        assert_eq!(
            native.tokenize_calls, 0,
            "bad prompt did not invoke Tokenize; retry uses explicit tokens"
        );
        assert_eq!(native.written.len(), 1);
        assert!(
            native
                .written
                .keys()
                .all(|(slot, _, incarnation)| *slot == 0 && *incarnation == 1)
        );
        assert_eq!(native.shutdowns, 0);
    }
}

#[test]
fn reply_string_4095_and_4096_bytes_tokens_execute_without_truncation() {
    for text in [ReplyText::Ascii, ReplyText::Escaped, ReplyText::Unicode] {
        for bytes in [4095, 4096] {
            legal_boundary(LimitedField::Reply(text), bytes, false);
        }
    }
}

#[test]
fn reply_string_4095_and_4096_bytes_prompt_execute_without_truncation() {
    for text in [ReplyText::Ascii, ReplyText::Escaped, ReplyText::Unicode] {
        for bytes in [4095, 4096] {
            legal_boundary(LimitedField::Reply(text), bytes, true);
        }
    }
}

#[test]
fn options_string_4095_and_4096_bytes_tokens_preserve_original_json() {
    for bytes in [4095, 4096] {
        legal_boundary(LimitedField::Options, bytes, false);
    }
}

#[test]
fn options_string_4095_and_4096_bytes_prompt_preserve_original_json() {
    for bytes in [4095, 4096] {
        legal_boundary(LimitedField::Options, bytes, true);
    }
}

macro_rules! oversized_case {
    ($name:ident, $field:expr, $prompt:expr) => {
        #[test]
        fn $name() {
            above_limit_rejects_and_recovers($field, $prompt);
        }
    };
}

oversized_case!(
    reply_ascii_4097_tokens_is_rejected_before_effects,
    LimitedField::Reply(ReplyText::Ascii),
    false
);
oversized_case!(
    reply_ascii_4097_prompt_is_rejected_before_tokenize,
    LimitedField::Reply(ReplyText::Ascii),
    true
);
oversized_case!(
    reply_escaped_4097_tokens_is_rejected_before_effects,
    LimitedField::Reply(ReplyText::Escaped),
    false
);
oversized_case!(
    reply_escaped_4097_prompt_is_rejected_before_tokenize,
    LimitedField::Reply(ReplyText::Escaped),
    true
);
oversized_case!(
    reply_unicode_4097_tokens_is_rejected_before_effects,
    LimitedField::Reply(ReplyText::Unicode),
    false
);
oversized_case!(
    reply_unicode_4097_prompt_is_rejected_before_tokenize,
    LimitedField::Reply(ReplyText::Unicode),
    true
);
oversized_case!(
    options_4097_tokens_is_rejected_before_effects,
    LimitedField::Options,
    false
);
oversized_case!(
    options_4097_prompt_is_rejected_before_tokenize,
    LimitedField::Options,
    true
);
