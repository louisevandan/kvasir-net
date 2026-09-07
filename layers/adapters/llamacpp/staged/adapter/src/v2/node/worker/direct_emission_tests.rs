//! Direct responses cross the actual SESSION/emit -> committed FIFO -> mailbox
//! consumers. The existing post-LOAD fixture has no native server. These tests
//! do not prove reserved storage, an asynchronous pump, or actor-ring progress.
use super::effects::{CommittedEffect, PublicationAfter};
use super::observe::{PreparedTelemetry, TelemetryPayload};
use super::*;
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};

fn fixture() -> (Worker, Arc<CompletionMailbox>) {
    let (mut worker, mailbox) = super::release_tests::fixture();
    worker.state.next_event = 41;
    (worker, mailbox)
}

fn session_input(worker: &Worker, id: &str) -> Event {
    let mut command = worker.state.sessions["pipeline"].command.clone();
    command.session_id = "direct-session".into();
    let mut input = crate::v2::tests::request_state(vec![7]).template;
    input.envelope.event_id = id.into();
    input.envelope.correlation_id = "one-source-one-correlation".into();
    input.envelope.target = worker.endpoint.clone();
    input.envelope.class = EventClass::Control;
    input.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    input.payload = serde_json::to_vec(&command).unwrap();
    p4_protocol::event::decode(&p4_protocol::event::encode(&input).unwrap()).unwrap()
}

fn ready_body() -> Vec<u8> {
    br#"{"load_generation":1,"session_id":"direct-session","state":"ready"}"#.to_vec()
}

fn expected_response(
    input: &Event,
    source: Endpoint,
    class: EventClass,
    content_type: &str,
    payload: Vec<u8>,
    sequence: u64,
) -> Event {
    Event {
        envelope: input.envelope.next(
            format!("{}:llamacpp:{sequence}", input.envelope.event_id),
            source,
            reply_target(input),
            class,
            sequence,
            content_type,
        ),
        payload,
    }
}

fn take(mailbox: &CompletionMailbox) -> Event {
    let Poll::Event(event) = mailbox.try_take() else {
        panic!("one exact response must have reached the real mailbox")
    };
    event
}

fn telemetry(input: &Event) -> PreparedTelemetry {
    let route = input.envelope.return_route.as_ref().unwrap();
    PreparedTelemetry {
        base: input.envelope.clone(),
        ingress: route.ingress_agent.clone(),
        reply: ReplySpec {
            ingress_agent: route.ingress_agent.to_string(),
            channel: route.channel.clone(),
            connection_generation: route.connection_generation,
            correlation_id: input.envelope.correlation_id.clone(),
            deadline_unix_ms: input.envelope.deadline_unix_ms,
        },
        payload: TelemetryPayload::Span(StageSpan {
            load_generation: 1,
            session_id: "pipeline".into(),
            execution_ids: vec![9],
            executions: Vec::new(),
            rows: 1,
            ingress_unix_ms: 10,
            start_unix_ms: 11,
            end_unix_ms: 12,
            forward_unix_ms: 0,
        }),
    }
}

#[test]
fn a_real_session_waits_for_the_older_observed_suffix_before_its_id_is_issued() {
    let (mut worker, mailbox) = fixture();
    let input = session_input(&worker, "session-after-observed");
    let source = worker.endpoint.clone();
    worker.effects.push_back(CommittedEffect::ForwardObserved {
        base: input.envelope.clone(),
        target: worker.state.sessions["pipeline"].next.clone().unwrap(),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body: vec![0, 255, 128, 9],
        telemetry: vec![telemetry(&input), telemetry(&input)],
    });
    assert_eq!(
        super::obligations::effect_event_count(&worker.effects),
        Ok(3)
    );
    worker.handle(input.clone()).unwrap();
    let events: Vec<_> = (0..4).map(|_| take(&mailbox)).collect();
    assert_eq!(
        events
            .iter()
            .map(|event| event.envelope.sequence)
            .collect::<Vec<_>>(),
        [41, 42, 43, 44]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.envelope.payload_content_type.as_str())
            .collect::<Vec<_>>(),
        [
            PHYSICAL_BATCH_CONTENT_TYPE,
            STAGE_SPAN_CONTENT_TYPE,
            STAGE_SPAN_CONTENT_TYPE,
            SESSION_READY_CONTENT_TYPE
        ]
    );
    for event in &events {
        assert_eq!(event.envelope.source, source);
        assert_eq!(event.envelope.correlation_id, input.envelope.correlation_id);
        assert_eq!(
            event.envelope.causation_id.as_deref(),
            Some(input.envelope.event_id.as_str())
        );
        assert_eq!(
            p4_protocol::event::decode(&p4_protocol::event::encode(event).unwrap()).unwrap(),
            *event
        );
    }
    assert_eq!(events[0].payload, [0, 255, 128, 9]);
    assert_eq!(
        events[3],
        expected_response(
            &input,
            source,
            EventClass::Telemetry,
            SESSION_READY_CONTENT_TYPE,
            ready_body(),
            44
        )
    );
    assert_eq!(worker.state.next_event, 45);
    assert!(worker.state.sessions.contains_key("direct-session"));
    assert!(worker.effects.is_empty() && !worker.effects_fenced);
    worker.flush_effects().unwrap();
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn failed_direct_publication_keeps_its_entire_event_and_original_body_allocation() {
    let (mut worker, mailbox) = fixture();
    let input = session_input(&worker, "direct-closed");
    let mut body = Vec::with_capacity(127);
    body.extend_from_slice(&[0, 255, 128, 4]);
    let pointer = body.as_ptr();
    let capacity = body.capacity();
    let expected = expected_response(
        &input,
        worker.endpoint.clone(),
        EventClass::Output,
        "application/test-direct",
        body.clone(),
        41,
    );
    drop(mailbox);
    assert!(
        worker
            .emit_bytes(
                &input,
                reply_target(&input),
                EventClass::Output,
                "application/test-direct",
                body
            )
            .is_err()
    );
    assert_eq!(worker.state.next_event, 42);
    assert!(worker.effects_fenced);
    assert_eq!(worker.effects.len(), 1);
    let CommittedEffect::Publication {
        event,
        after: PublicationAfter::Direct { diagnostic: false },
    } = &worker.effects[0]
    else {
        panic!("permanent publication failure must retain the whole exact Event")
    };
    assert_eq!(event, &expected);
    assert_eq!(event.payload.as_ptr(), pointer);
    assert_eq!(event.payload.capacity(), capacity);
    let retained = format!("{:?}", worker.effects);
    assert!(
        worker.flush_effects().is_err(),
        "production does not reopen a failed publication"
    );
    assert_eq!(format!("{:?}", worker.effects), retained);
    assert_eq!(worker.state.next_event, 42);

    // Test-only storage replacement/fence removal inspects retry identity; it
    // is not a product recovery or uncertain-native replay mechanism.
    let (publisher, mailbox) = completion_mailbox(8);
    worker.publisher = publisher;
    worker.effects_fenced = false;
    worker.flush_effects().unwrap();
    let delivered = take(&mailbox);
    assert_eq!(delivered, expected);
    assert_eq!(delivered.payload.as_ptr(), pointer);
    assert_eq!(delivered.payload.capacity(), capacity);
    worker.flush_effects().unwrap();
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    assert_eq!(worker.state.next_event, 42);
}

#[test]
fn future_id_width_is_validated_before_session_authority_changes() {
    let (mut worker, mailbox) = fixture();
    let mut input = session_input(&worker, "x");
    let response = expected_response(
        &input,
        worker.endpoint.clone(),
        EventClass::Telemetry,
        SESSION_READY_CONTENT_TYPE,
        ready_body(),
        41,
    );
    let wire = p4_protocol::event::encode(&response).unwrap();
    let fixed_envelope = u32::from_le_bytes(wire[4..8].try_into().unwrap()) as usize;
    // The codec's combined envelope cap is 256 KiB. Each additional source
    // ID byte appears twice (derived ID + causation). This closed expression
    // fits ID 41 but not a 20-digit future ID; it does not search for a failure.
    let id_length = 1 + (256 * 1024 - fixed_envelope) / 2;
    input.envelope.event_id = "x".repeat(id_length);
    let input = p4_protocol::event::decode(&p4_protocol::event::encode(&input).unwrap()).unwrap();
    let short_id_response = expected_response(
        &input,
        worker.endpoint.clone(),
        EventClass::Telemetry,
        SESSION_READY_CONTENT_TYPE,
        ready_body(),
        41,
    );
    assert_eq!(
        p4_protocol::event::decode(&p4_protocol::event::encode(&short_id_response).unwrap())
            .unwrap(),
        short_id_response
    );
    let before = super::release_tests::snapshot(&worker);
    let error = worker.session(input.clone()).unwrap_err();
    assert!(
        error.contains("completion event cannot be decoded"),
        "{error}"
    );
    assert_eq!(super::release_tests::snapshot(&worker), before);
    assert!(!worker.state.sessions.contains_key("direct-session"));
    assert!(matches!(mailbox.try_take(), Poll::Empty));

    let corrected = session_input(&worker, "short-session-id");
    worker.handle(corrected.clone()).unwrap();
    assert_eq!(
        take(&mailbox),
        expected_response(
            &corrected,
            worker.endpoint.clone(),
            EventClass::Telemetry,
            SESSION_READY_CONTENT_TYPE,
            ready_body(),
            41
        )
    );
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn actual_counter_exhaustion_does_not_move_a_prevalidated_direct_body() {
    let (mut worker, mailbox) = fixture();
    let input = session_input(&worker, "prepared-before-exhaustion");
    let prepared = worker
        .prepare_json_emission(
            &input,
            reply_target(&input),
            EventClass::Output,
            ERROR_CONTENT_TYPE,
            &serde_json::json!({"detail":"fixed"}),
        )
        .unwrap();
    let body = prepared.intent_for_test().body.clone();
    let pointer = prepared.intent_for_test().body.as_ptr();
    let capacity = prepared.intent_for_test().body.capacity();
    let expected = expected_response(
        &input,
        worker.endpoint.clone(),
        EventClass::Output,
        ERROR_CONTENT_TYPE,
        body.clone(),
        41,
    );
    worker.effects.push_back(CommittedEffect::Direct(prepared));
    // Fault injection at the actual materialization boundary, not a reachable
    // healthy-counter claim. The effect must survive even this invalid state.
    worker.state.next_event = u64::MAX;
    assert!(worker.flush_effects().is_err());
    let CommittedEffect::Direct(prepared) = &worker.effects[0] else {
        panic!("failed numbering cannot consume or replace the original intent")
    };
    assert_eq!(prepared.intent_for_test().body, body);
    assert_eq!(prepared.intent_for_test().body.as_ptr(), pointer);
    assert_eq!(prepared.intent_for_test().body.capacity(), capacity);
    assert_eq!(worker.state.next_event, u64::MAX);
    assert!(matches!(mailbox.try_take(), Poll::Empty));

    // Test-only repair checks that the prepared source, not a subsequently
    // changed Worker field, supplies the actual Event's provenance.
    worker.state.next_event = 41;
    worker.effects_fenced = false;
    worker.endpoint = Endpoint::node(Address::tcp("127.0.0.1", 42009), "different-source", 9);
    worker.flush_effects().unwrap();
    assert_eq!(take(&mailbox), expected);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn a_native_fence_with_no_effect_prefix_can_deliver_only_its_new_diagnostic() {
    let (mut worker, mailbox) = fixture();
    let input = session_input(&worker, "failed-native");
    worker.effects_fenced = true;
    worker.set_snapshot("failed:original-native-cause");
    worker
        .emit_error(&input, "NATIVE_FAILED", "original-native-cause".into())
        .unwrap();
    let expected = expected_response(
        &input,
        worker.endpoint.clone(),
        EventClass::Output,
        ERROR_CONTENT_TYPE,
        br#"{"code":"NATIVE_FAILED","detail":"original-native-cause"}"#.to_vec(),
        41,
    );
    assert_eq!(take(&mailbox), expected);
    assert!(worker.effects_fenced && worker.effects.is_empty());
    assert_eq!(
        worker.snapshot.lock().unwrap().as_str(),
        "failed:original-native-cause"
    );
    assert!(
        worker.flush_effects().is_err(),
        "the native fence remains closed"
    );
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn an_old_fenced_publication_is_not_replayed_to_deliver_a_later_diagnostic() {
    let (mut worker, mailbox) = fixture();
    let input = session_input(&worker, "diagnostic-after-failed-publication");
    let old = expected_response(
        &input,
        worker.endpoint.clone(),
        EventClass::Data,
        PHYSICAL_BATCH_CONTENT_TYPE,
        vec![0, 255, 4],
        40,
    );
    worker.effects.push_back(CommittedEffect::Publication {
        event: old.clone(),
        after: PublicationAfter::Forward,
    });
    worker.effects_fenced = true;
    assert!(
        worker
            .emit_error(&input, "NATIVE_FAILED", "prefix-is-uncertain".into())
            .is_err()
    );
    assert_eq!(worker.effects.len(), 2);
    let CommittedEffect::Publication { event, .. } = &worker.effects[0] else {
        panic!("old publication lost")
    };
    assert_eq!(event, &old);
    let CommittedEffect::Direct(prepared) = &worker.effects[1] else {
        panic!("new diagnostic lost")
    };
    let intent = prepared.intent_for_test();
    assert_eq!(intent.base, input.envelope);
    assert_eq!(intent.source, worker.endpoint);
    assert_eq!(intent.target, reply_target(&input));
    assert_eq!(intent.class, EventClass::Output);
    assert_eq!(intent.content_type, ERROR_CONTENT_TYPE);
    assert_eq!(
        intent.body,
        br#"{"code":"NATIVE_FAILED","detail":"prefix-is-uncertain"}"#
    );
    assert_eq!(worker.state.next_event, 41);
    assert!(worker.effects_fenced);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn first_closed_batch_diagnostic_preserves_every_failed_computation_owner() {
    let (mut worker, mailbox) = fixture();
    let first = session_input(&worker, "failed-owner-one");
    let mut second = session_input(&worker, "failed-owner-two");
    second.envelope.correlation_id = "second-owner-correlation".into();
    let Endpoint::Outer(route) = &mut second.envelope.source else {
        unreachable!()
    };
    route.channel = "second-owner".into();
    second.envelope.return_route = Some(route.clone());
    let body = br#"{"code":"BATCH_FAILED","detail":"same-native-failure"}"#.to_vec();
    let expected_first = expected_response(
        &first,
        worker.endpoint.clone(),
        EventClass::Output,
        ERROR_CONTENT_TYPE,
        body.clone(),
        41,
    );
    drop(mailbox);
    assert!(
        worker
            .emit_batch_errors(
                &[first, second.clone()],
                "BATCH_FAILED",
                "same-native-failure"
            )
            .is_err()
    );
    assert_eq!(worker.effects.len(), 2);
    let CommittedEffect::Publication { event, .. } = &worker.effects[0] else {
        panic!("first diagnostic Event lost")
    };
    assert_eq!(event, &expected_first);
    let CommittedEffect::Direct(prepared) = &worker.effects[1] else {
        panic!("later failed owner lost its unissued diagnostic")
    };
    let intent = prepared.intent_for_test();
    assert_eq!(intent.base, second.envelope);
    assert_eq!(intent.source, worker.endpoint);
    assert_eq!(intent.target, reply_target(&second));
    assert_eq!(intent.class, EventClass::Output);
    assert_eq!(intent.content_type, ERROR_CONTENT_TYPE);
    assert_eq!(intent.body, body);
    assert_eq!(
        worker.state.next_event, 42,
        "only the first FIFO item owns an ID"
    );
    assert_eq!(
        super::obligations::effect_event_count(&worker.effects),
        Ok(1)
    );
    assert!(worker.effects_fenced);
}

#[test]
fn a_batch_diagnostic_reserves_all_id_obligations_before_its_first_publication() {
    let (mut worker, mailbox) = fixture();
    let first = session_input(&worker, "id-owner-one");
    let second = session_input(&worker, "id-owner-two");
    assert_eq!(worker.state.pending_releases.len(), 2);
    // The fixture already owes two future receipts. Exactly one additional
    // direct ID fits; two cannot be partially accepted or partially emitted.
    worker.state.next_event = u64::MAX - 3;
    let before = super::release_tests::snapshot(&worker);
    assert!(
        worker
            .emit_batch_errors(&[first.clone(), second], "BATCH_FAILED", "id-limit")
            .is_err()
    );
    assert_eq!(super::release_tests::snapshot(&worker), before);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    worker
        .emit_error(&first, "BATCH_FAILED", "id-limit".into())
        .unwrap();
    assert_eq!(
        take(&mailbox),
        expected_response(
            &first,
            worker.endpoint.clone(),
            EventClass::Output,
            ERROR_CONTENT_TYPE,
            br#"{"code":"BATCH_FAILED","detail":"id-limit"}"#.to_vec(),
            u64::MAX - 3
        )
    );
    assert_eq!(worker.state.next_event, u64::MAX - 2);
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}
