//! The event codec accepts these strings, so rejection belongs at the adapter
//! submission boundary, not after native KV mutation. These tests run genuine
//! Worker::run threads. Only the existing model-free native boundary is fake.
//! They do not call the production witness validator to construct an oracle.
use super::*;

#[derive(Clone, Copy, Debug)]
enum InvalidField {
    EventId,
    OuterChannel,
    IngressHost,
}

#[derive(Clone, Debug)]
struct AdmissionView {
    point: &'static str,
    next_incarnation: u64,
    pending: Vec<String>,
    requests: Vec<(String, Option<u32>, u64)>,
    session_keys: Vec<((u64, String, String), Option<String>)>,
    free: Vec<u32>,
    prepared: Option<String>,
}

fn observe_admission() -> (IssueObserver, Arc<Mutex<Vec<AdmissionView>>>) {
    let views = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&views);
    let observer: IssueObserver = Arc::new(move |point, state| {
        let view = AdmissionView {
            point,
            next_incarnation: state.next_incarnation,
            pending: state.pending.iter().cloned().collect(),
            requests: state
                .requests
                .values()
                .map(|request| {
                    (
                        request.command.request_id.clone(),
                        request.sequence_id,
                        request.incarnation,
                    )
                })
                .collect(),
            session_keys: state
                .session_keys
                .iter()
                .map(|(scope, key)| (scope.clone(), key.clone()))
                .collect(),
            free: state.free_sequences.iter().copied().collect(),
            prepared: state
                .prepared_issue
                .as_ref()
                .map(|issue| format!("{:?}", issue.progress)),
        };
        let mut captured = captured.lock().unwrap();
        assert!(captured.len() < 128, "bounded test observation history");
        captured.push(view);
    });
    (observer, views)
}

fn invalid_submission(field: InvalidField, prompt: bool) -> Event {
    let mut command = request("submission-reused", 1, 2);
    command.session_key = Some("sk1:loop/rejected".into());
    if prompt {
        command.tokens.clear();
        command.prompt = Some("Explain why requests retain their own KV state.".into());
    }
    command.validate().unwrap();
    let mut value = submission_event(&command, 1, default_route());
    // Keep event ID independent of route syntax to isolate one invalid field.
    value.envelope.event_id = "invalid-submission-1".into();
    match field {
        InvalidField::EventId => value.envelope.event_id = "invalid\0submission-1".into(),
        InvalidField::OuterChannel | InvalidField::IngressHost => {
            let mut route = value.envelope.return_route.clone().unwrap();
            match field {
                InvalidField::OuterChannel => route.channel = "loop\0output".into(),
                InvalidField::IngressHost => route.ingress_agent.host = "127.0\0.0.1".into(),
                InvalidField::EventId => unreachable!(),
            }
            value.envelope.source = Endpoint::Outer(route.clone());
            value.envelope.return_route = Some(route);
        }
    }
    value
        .validate()
        .expect("generic event vocabulary admits the counterexample");
    assert_eq!(
        event_wire(value.clone()),
        value,
        "actual wire preserves every original field"
    );
    value
}

fn rejects_before_effects_and_continues(field: InvalidField, prompt: bool) {
    let invalid = invalid_submission(field, prompt);
    let (observer, views) = observe_admission();
    let mut h = Harness::observed_events(2, 1, 8, &[], 0, None, Some(observer), None);
    h.expected_submission_error = Some(invalid.clone());
    // This is deliberately not an accepted submission in the OUTPUT/receipt
    // oracle. Its untouched original Event is retained above and goes through
    // the same real bounded input and event codec as a normal request.
    h.pending.push_back(invalid.clone());
    // Rejection does not rewind the OUTER sender's event sequence.
    h.next_submission = 2;
    h.until(
        "the exact invalid submission returns an explicit error",
        |h| {
            h.received.iter().any(|event| {
                event.envelope.payload_content_type == ERROR_CONTENT_TYPE
                    && event.envelope.causation_id.as_deref() == Some(&invalid.envelope.event_id)
            })
        },
    );
    let errors: Vec<_> = h
        .received
        .iter()
        .filter(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "one explicit rejection, not a worker-wide cascade"
    );
    let error = errors[0];
    let body: serde_json::Value = serde_json::from_slice(&error.payload).unwrap();
    eprintln!(
        "submission identity counterexample field={field:?} prompt={prompt}; error={body}; admission={:?}; native={:?}; snapshot={}",
        views.lock().unwrap(),
        h.nodes[0].native.lock().unwrap(),
        h.nodes[0].snapshot.lock().unwrap()
    );
    assert_eq!(error.envelope.source, invalid.envelope.target);
    assert_eq!(error.envelope.target, invalid.envelope.source);
    assert_eq!(error.envelope.return_route, invalid.envelope.return_route);
    assert_eq!(
        error.envelope.correlation_id,
        invalid.envelope.correlation_id
    );
    assert_eq!(
        error.envelope.causation_id.as_deref(),
        Some(invalid.envelope.event_id.as_str())
    );
    assert_eq!(
        body["code"], "LLAMA_ADAPTER_EVENT_REJECTED",
        "identity rejection must precede native issue, not fence a mutated batch"
    );
    assert_eq!(
        body["detail"],
        "issued-work identity string is empty, contains NUL or is too long"
    );
    for node in &h.nodes {
        let native = node.native.lock().unwrap();
        assert_eq!(
            native.tokenize_calls, 0,
            "bad prompt must not enter Tokenize"
        );
        assert_eq!(native.logical_calls, 0);
        assert_eq!(native.physical_calls, 0);
        assert!(native.live.is_empty());
        assert!(native.written.is_empty());
        assert!(native.releases.is_empty());
        assert!(native.release_bodies.is_empty());
        assert_eq!(native.sampler_calls, 0);
        assert_eq!(
            native.shutdowns, 0,
            "submission rejection must not terminate the worker"
        );
    }
    assert!(
        views.lock().unwrap().is_empty(),
        "bad submission must not reach native/accepted/stopping hooks"
    );
    assert!(h.outputs.is_empty());
    assert!(
        h.stage_events.is_empty(),
        "bad submission must not generate a stage flight"
    );
    h.expected_submission_error = None;

    // Reuse the *same* request identity with a different session key. This
    // detects a rejected event's leaked key/admission/incarnation bookkeeping.
    let mut good = request("submission-reused", 1, 2);
    good.session_key = Some("sk1:loop/accepted".into());
    h.enqueue(&good);
    h.finish(std::slice::from_ref(&good));
    release_notifications::assert_complete(&h);
    let all_views = views.lock().unwrap();
    let first = all_views
        .iter()
        .find(|view| view.point == "before_native_issue")
        .unwrap();
    assert_eq!(
        first.next_incarnation, 2,
        "only the accepted request consumes incarnation one"
    );
    assert_eq!(first.requests, vec![(good.request_id.clone(), Some(0), 1)]);
    assert!(first.pending.is_empty());
    assert_eq!(first.free, (1..SEQUENCE_CAPACITY).collect::<Vec<_>>());
    assert_eq!(
        first.session_keys,
        vec![(
            (1, "loop-session".into(), good.request_id.clone()),
            good.session_key.clone()
        )]
    );
    assert!(
        first.prepared.is_some(),
        "observation is in the real prepared/native path"
    );
    assert!(!all_views.iter().any(|view| view.point == "run_stopping"));
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
            "rejected prompt never reached the native tokenizer"
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
fn submission_nul_event_id_tokens_is_rejected_before_native_and_allows_retry() {
    rejects_before_effects_and_continues(InvalidField::EventId, false);
}

#[test]
fn submission_nul_event_id_prompt_is_rejected_before_tokenize_and_allows_retry() {
    rejects_before_effects_and_continues(InvalidField::EventId, true);
}

#[test]
fn submission_nul_channel_tokens_is_rejected_before_native_and_allows_retry() {
    rejects_before_effects_and_continues(InvalidField::OuterChannel, false);
}

#[test]
fn submission_nul_channel_prompt_is_rejected_before_tokenize_and_allows_retry() {
    rejects_before_effects_and_continues(InvalidField::OuterChannel, true);
}

#[test]
fn submission_nul_host_tokens_is_rejected_before_native_and_allows_retry() {
    rejects_before_effects_and_continues(InvalidField::IngressHost, false);
}

#[test]
fn submission_nul_host_prompt_is_rejected_before_tokenize_and_allows_retry() {
    rejects_before_effects_and_continues(InvalidField::IngressHost, true);
}

#[test]
fn submission_unicode_identity_and_distinct_correlation_remain_valid() {
    let command = request("unicode-accepted", 3, 3);
    let mut route = default_route();
    route.channel = "응답-채널".into();
    let mut input = submission_event(&command, 1, route);
    input.envelope.event_id = "합법-제출-식별자".into();
    input.envelope.correlation_id = "요청-ID와-다른-상관관계".into();
    assert_eq!(event_wire(input.clone()), input);
    let mut h = Harness::configured_events(2, 1, 8, &[input], 0, None);
    h.finish(std::slice::from_ref(&command));
    release_notifications::assert_complete(&h);
}
