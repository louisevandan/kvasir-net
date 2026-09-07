//! The real PREFILL consumer, using its parent's post-LOAD SESSION fixture.
//! These tests prove preflight/commit ordering, not byte grants, native parser
//! recovery, asynchronous actor progress, or GPU execution.
use super::*;

fn prefill_fixture() -> (Worker, Arc<CompletionMailbox>) {
    let (mut worker, mailbox) = fixture("head");
    worker.state.context_size = 32;
    worker.state.sequence_capacity = 4;
    worker.state.free_sequences = (0..4).collect();
    worker.handle(event(&worker, &command(0))).unwrap();
    assert_eq!(
        response(&mailbox).envelope.payload_content_type,
        SESSION_READY_CONTENT_TYPE
    );
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    (worker, mailbox)
}

fn submission(worker: &Worker, name: &str) -> Event {
    let mut input = event(worker, &command(0));
    input.envelope.event_id = format!("original-{name}");
    input.envelope.correlation_id = format!("correlation-{name}");
    input.envelope.source = Endpoint::outer(Address::tcp("127.0.0.1", 43991), "owner", 5);
    let Endpoint::Outer(route) = &input.envelope.source else {
        unreachable!()
    };
    input.envelope.return_route = Some(route.clone());
    input.envelope.class = EventClass::Data;
    input.envelope.payload_content_type = PREFILL_CONTENT_TYPE.into();
    input.payload = serde_json::to_vec(&InferenceCommand {
        load_generation: 7,
        session_id: "declared-pipeline".into(),
        request_id: name.into(),
        tokens: vec![7],
        prompt: None,
        options: "{}".into(),
        session_key: Some(format!("sk1:owner/{name}")),
        max_tokens: 1,
    })
    .unwrap();
    wire(input)
}

fn wire(input: Event) -> Event {
    let command: InferenceCommand = serde_json::from_slice(&input.payload).unwrap();
    command.validate().unwrap();
    let decoded = p4_protocol::event::decode(&p4_protocol::event::encode(&input).unwrap()).unwrap();
    assert_eq!(decoded, input);
    decoded
}

fn change_command(input: &Event, change: impl FnOnce(&mut InferenceCommand)) -> Event {
    let mut command: InferenceCommand = serde_json::from_slice(&input.payload).unwrap();
    change(&mut command);
    let mut next = input.clone();
    next.payload = serde_json::to_vec(&command).unwrap();
    wire(next)
}

fn admission_snapshot(worker: &Worker) -> serde_json::Value {
    let mut value = super::super::release_tests::snapshot(worker);
    value.as_object_mut().unwrap().remove("next_event");
    value["lifecycle"] = serde_json::json!(format!("{:?}", worker.lifecycle.state()));
    value["has_server"] = serde_json::json!(worker.lifecycle.has_server());
    value["active_publications"] = serde_json::json!(worker.active_publications);
    value["active_effect_ids"] = serde_json::json!(worker.active_effect_ids);
    value["held_input"] = serde_json::json!(format!("{:?}", worker.held_input));
    value["deferred_ack_error"] = serde_json::json!(format!("{:?}", worker.deferred_ack_error));
    value
}

fn rejected_without_admission(
    worker: &mut Worker,
    mailbox: &CompletionMailbox,
    input: &Event,
    expected_detail: &str,
) {
    let before = admission_snapshot(worker);
    let routes = installed(worker);
    let next_event = worker.state.next_event;
    let detail = worker.prefill(input.clone()).unwrap_err();
    assert!(detail.contains(expected_detail), "{detail}");
    assert_eq!(
        admission_snapshot(worker),
        before,
        "direct refusal mutated admission"
    );
    assert_eq!(worker.state.next_event, next_event);
    assert_eq!(installed(worker), routes);
    assert!(matches!(mailbox.try_take(), Poll::Empty));

    worker.handle(input.clone()).unwrap();
    let error = response(mailbox);
    let expected = Event {
        envelope: input.envelope.next(
            format!("{}:llamacpp:{next_event}", input.envelope.event_id),
            worker.endpoint.clone(),
            reply_target(input),
            EventClass::Output,
            next_event,
            ERROR_CONTENT_TYPE,
        ),
        payload: serde_json::to_vec(&ErrorPayload {
            code: "LLAMA_ADAPTER_EVENT_REJECTED".into(),
            detail,
        })
        .unwrap(),
    };
    assert_eq!(
        error, expected,
        "only this input's rejection may be emitted"
    );
    assert_eq!(
        p4_protocol::event::decode(&p4_protocol::event::encode(&error).unwrap()).unwrap(),
        expected
    );
    assert_eq!(worker.state.next_event, next_event + 1);
    assert_eq!(
        admission_snapshot(worker),
        before,
        "handle refusal mutated admission"
    );
    assert_eq!(installed(worker), routes);
    assert!(
        matches!(mailbox.try_take(), Poll::Empty),
        "exactly one ERROR is owed"
    );
}

fn accepted_without_native(worker: &mut Worker, mailbox: &CompletionMailbox, input: &Event) {
    let command: InferenceCommand = serde_json::from_slice(&input.payload).unwrap();
    assert!(command.prompt.is_none());
    let key = request_key(&command.session_id, &command.request_id);
    let incarnation = worker.state.next_incarnation;
    let event_id = worker.state.next_event;
    let count = worker.state.requests.len();
    worker.handle(input.clone()).unwrap();
    let request = &worker.state.requests[&key];
    assert_eq!(request.command, command);
    assert_eq!(request.template, *input);
    assert_eq!(request.incarnation, incarnation);
    assert_eq!(request.outstanding, 0);
    assert_eq!(request.prompt_cursor, 0);
    assert_eq!(request.prompt_issued, 0);
    assert_eq!(request.generated, 0);
    assert_eq!(worker.state.requests.len(), count + 1);
    assert_eq!(worker.state.next_incarnation, incarnation + 1);
    assert_eq!(worker.state.next_event, event_id);
    assert!(worker.effects.is_empty() && !worker.effects_fenced);
    assert!(!worker.lifecycle.has_server());
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn context_refusal_does_not_bind_the_rejected_conversation_or_consume_a_slot() {
    let (mut worker, mailbox) = prefill_fixture();
    let valid = submission(&worker, "context");
    let invalid = change_command(&valid, |command| command.tokens = vec![7; 32]);
    rejected_without_admission(
        &mut worker,
        &mailbox,
        &invalid,
        "prompt plus max_tokens exceeds loaded per-sequence context",
    );
    // A refused request has not bound its original session_key. Correcting
    // this same request identity must not encounter a stale alias record.
    let corrected = change_command(&valid, |command| {
        command.tokens = vec![7; 31];
        command.session_key = Some("sk1:owner/corrected-context".into());
    });
    accepted_without_native(&mut worker, &mailbox, &corrected);
    assert_eq!(worker.state.free_sequences, [1, 2, 3]);
    assert!(worker.state.pending.is_empty());
    assert_eq!(
        worker.state.requests[&request_key("declared-pipeline", "context")].sequence_id,
        Some(0)
    );
}

#[test]
fn zero_or_exhausted_incarnation_refuses_before_any_admission_write() {
    for incarnation in [0, u64::MAX] {
        let (mut worker, mailbox) = prefill_fixture();
        worker.state.next_incarnation = incarnation;
        let input = submission(&worker, "incarnation");
        rejected_without_admission(
            &mut worker,
            &mailbox,
            &input,
            "request incarnation exhausted",
        );
        worker.state.next_incarnation = 7;
        accepted_without_native(&mut worker, &mailbox, &input);
        let request = &worker.state.requests[&request_key("declared-pipeline", "incarnation")];
        assert_eq!((request.incarnation, request.sequence_id), (7, Some(0)));
    }
}

#[test]
fn invalid_free_slot_refuses_the_whole_existing_prefix_and_new_candidate() {
    for duplicate in [false, true] {
        let (mut worker, mailbox) = prefill_fixture();
        worker.state.free_sequences.clear();
        let earlier = submission(&worker, "earlier");
        accepted_without_native(&mut worker, &mailbox, &earlier);
        worker.state.free_sequences = if duplicate {
            [0, 0].into()
        } else {
            [4, 1].into()
        };
        let input = submission(&worker, "new-slot");
        rejected_without_admission(
            &mut worker,
            &mailbox,
            &input,
            "admission slot or pending request is duplicated",
        );
        worker.state.free_sequences = [0, 1].into();
        accepted_without_native(&mut worker, &mailbox, &input);
        assert_eq!(
            worker.state.requests[&request_key("declared-pipeline", "earlier")].sequence_id,
            Some(0)
        );
        assert_eq!(
            worker.state.requests[&request_key("declared-pipeline", "new-slot")].sequence_id,
            Some(1)
        );
        assert!(worker.state.pending.is_empty() && worker.state.free_sequences.is_empty());
    }
}

#[test]
fn a_free_slot_that_is_still_owned_cannot_be_reassigned_by_a_new_prefill() {
    let (mut worker, mailbox) = prefill_fixture();
    let earlier = submission(&worker, "owner");
    accepted_without_native(&mut worker, &mailbox, &earlier);
    worker.state.free_sequences = [0, 1].into();
    let input = submission(&worker, "next-owner");
    rejected_without_admission(
        &mut worker,
        &mailbox,
        &input,
        "admission slot or pending request is duplicated",
    );
    worker.state.free_sequences = [1].into();
    accepted_without_native(&mut worker, &mailbox, &input);
    assert_eq!(
        worker.state.requests[&request_key("declared-pipeline", "owner")].sequence_id,
        Some(0)
    );
    assert_eq!(
        worker.state.requests[&request_key("declared-pipeline", "next-owner")].sequence_id,
        Some(1)
    );
    assert!(worker.state.pending.is_empty() && worker.state.free_sequences.is_empty());
}

#[test]
fn an_invalid_later_pending_member_cannot_partially_admit_its_valid_prefix() {
    for missing in [false, true] {
        let (mut worker, mailbox) = prefill_fixture();
        worker.state.free_sequences.clear();
        for name in ["earlier", "bad-later"] {
            let input = submission(&worker, name);
            accepted_without_native(&mut worker, &mailbox, &input);
        }
        let bad_key = request_key("declared-pipeline", "bad-later");
        let removed = if missing {
            worker.state.requests.remove(&bad_key)
        } else {
            worker.state.requests.get_mut(&bad_key).unwrap().sequence_id = Some(3);
            None
        };
        worker.state.free_sequences = [0, 1, 2].into();
        let input = submission(&worker, "new-prefix");
        rejected_without_admission(
            &mut worker,
            &mailbox,
            &input,
            if missing {
                "pending request identity is missing"
            } else {
                "pending request already owns a sequence"
            },
        );
        if let Some(request) = removed {
            worker.state.requests.insert(bad_key, request);
        } else {
            worker.state.requests.get_mut(&bad_key).unwrap().sequence_id = None;
        }
        accepted_without_native(&mut worker, &mailbox, &input);
        for (slot, name) in ["earlier", "bad-later", "new-prefix"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                worker.state.requests[&request_key("declared-pipeline", name)].sequence_id,
                Some(slot as u32)
            );
        }
        assert!(worker.state.pending.is_empty() && worker.state.free_sequences.is_empty());
    }
}

#[test]
fn tokenize_failure_does_not_bind_or_admit_the_rejected_request() {
    let (mut worker, mailbox) = prefill_fixture();
    let token_input = submission(&worker, "tokenize");
    let prompt_input = change_command(&token_input, |command| {
        command.tokens.clear();
        command.prompt = Some("a normal prompt".into());
    });
    // The reused fixture intentionally has no native server. This reaches
    // tokenize -> lifecycle.request and rejects there, not at command/route
    // validation. It is not an injected native parser or GPU failure.
    rejected_without_admission(
        &mut worker,
        &mailbox,
        &prompt_input,
        "stage request failed:",
    );
    accepted_without_native(&mut worker, &mailbox, &token_input);
    assert_eq!(
        worker.state.requests[&request_key("declared-pipeline", "tokenize")].sequence_id,
        Some(0)
    );
}

#[test]
fn two_available_slots_admit_the_oldest_two_requests_before_the_new_candidate() {
    let (mut worker, mailbox) = prefill_fixture();
    worker.state.free_sequences.clear();
    let earlier = submission(&worker, "first");
    let later = submission(&worker, "second");
    accepted_without_native(&mut worker, &mailbox, &earlier);
    accepted_without_native(&mut worker, &mailbox, &later);
    worker.state.free_sequences = [2, 0].into();
    let newest = submission(&worker, "third");
    accepted_without_native(&mut worker, &mailbox, &newest);
    for (name, slot, original) in [
        ("first", Some(2), &earlier),
        ("second", Some(0), &later),
        ("third", None, &newest),
    ] {
        let request = &worker.state.requests[&request_key("declared-pipeline", name)];
        assert_eq!(request.sequence_id, slot);
        assert_eq!(&request.template, original);
    }
    assert_eq!(
        worker.state.pending,
        [request_key("declared-pipeline", "third")]
    );
    assert!(worker.state.free_sequences.is_empty());
    assert_eq!(worker.state.next_incarnation, 4);
}
