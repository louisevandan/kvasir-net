//! Actual prepare_outputs/flush_effects/mailbox consumers with no native stage.
//! Pointer assertions observe ownership of nonempty Vec/String allocations,
//! not throughput, RSS bounds, actor resumption, or native execution safety.
use super::effects::{CommittedEffect, PublicationAfter};
use super::observe::{PreparedTelemetry, TelemetryPayload};
use super::*;
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use p4_protocol::event::Envelope;

fn endpoint(name: &str) -> Endpoint {
    Endpoint::node(Address::tcp("127.0.0.1", 42471), name, 1)
}

fn fixture(capacity: usize) -> (Worker, Arc<CompletionMailbox>) {
    let (_, receiver) = mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(capacity);
    (
        Worker::new(
            endpoint("head"),
            receiver,
            publisher,
            Arc::new(Mutex::new(String::new())),
            Arc::new(AtomicBool::new(false)),
        ),
        mailbox,
    )
}

fn input(payload_size: usize) -> Event {
    let mut event = crate::v2::tests::request_state(vec![11]).template;
    event.envelope.event_id = "tail-cause".into();
    event.envelope.source = endpoint("tail");
    event.envelope.target = endpoint("head");
    event.envelope.payload_content_type = TAIL_BATCH_CONTENT_TYPE.into();
    event.payload = vec![173; payload_size];
    event
}

fn owner() -> RowOwner {
    let request = crate::v2::tests::request_state(vec![11]);
    RowOwner {
        load_generation: 1,
        incarnation: 1,
        request_id: "request".into(),
        sequence_key: request_key("session", "request"),
        session_id: "session".into(),
        reply: request.reply,
        sequence_id: 0,
        phase: Phase::Decode,
        position: 10,
        max_tokens: 64,
        generated_tokens: 1,
        output: true,
        input_token: 11,
        speculative_id: 0,
        speculative_index: 0,
        speculative_count: 0,
        options: String::new(),
    }
}

fn outputs(base: &Event, text: &[&str]) -> std::collections::VecDeque<CommittedEffect> {
    Worker::prepare_outputs(
        base,
        text.iter()
            .enumerate()
            .map(|(index, text)| {
                (
                    owner(),
                    GeneratedToken {
                        token: 100 + index as i32,
                        text: (*text).into(),
                        position: 11 + index as u32,
                        stop: None,
                    },
                    "original-submission".into(),
                    None,
                    None,
                )
            })
            .collect(),
    )
    .unwrap()
}

fn telemetry(base: &Envelope, name: &str) -> PreparedTelemetry {
    let mut reply: ReplySpec = serde_json::from_str(&owner().reply).unwrap();
    reply.correlation_id = name.into();
    PreparedTelemetry {
        base: base.clone(),
        ingress: Address::from_str(&reply.ingress_agent).unwrap(),
        reply,
        payload: TelemetryPayload::Span(StageSpan {
            load_generation: 1,
            session_id: "session".into(),
            execution_ids: vec![41],
            executions: vec![StageExecutionObservation {
                execution_id: 41,
                owned_requests: vec![StageRequestObservation {
                    request_id: name.into(),
                    sequence_id: 0,
                    incarnation: 1,
                }],
            }],
            rows: 1,
            ingress_unix_ms: 10,
            start_unix_ms: 11,
            end_unix_ms: 12,
            forward_unix_ms: 0,
        }),
    }
}

fn next(mailbox: &CompletionMailbox) -> Event {
    let Poll::Event(event) = mailbox.try_take() else {
        panic!("expected an emitted effect")
    };
    event
}

#[test]
fn prepared_outputs_do_not_retain_the_causal_tail_payload_per_token() {
    for count in [1, 8] {
        let texts = vec!["same token"; count];
        let empty = outputs(&input(0), &texts);
        let expected = format!("{empty:?}");
        for payload_size in [8192, 65536] {
            let prepared = outputs(&input(payload_size), &texts);
            assert_eq!(prepared.len(), count);
            assert_eq!(
                format!("{prepared:?}"),
                expected,
                "retained effect fields must not depend on the causal payload size"
            );
            for effect in prepared {
                let CommittedEffect::Output { base, .. } = effect else {
                    panic!("not an output")
                };
                // A type-level ownership boundary, not an empty Vec convention.
                let envelope: &Envelope = &base;
                assert_eq!(envelope.event_id, "tail-cause");
            }
        }
    }
}

#[test]
fn flush_moves_the_original_forward_allocation_into_the_mailbox() {
    let (mut worker, mailbox) = fixture(4);
    let base = input(65536).envelope;
    let body = vec![91; 32768];
    let allocation = body.as_ptr();
    worker.state.next_event = 27;
    worker.effects.push_back(CommittedEffect::Forward {
        base: base.clone(),
        target: endpoint("next"),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body,
    });
    worker.flush_effects().unwrap();
    let emitted = next(&mailbox);
    assert_eq!(
        emitted.payload.as_ptr(),
        allocation,
        "flush must transfer rather than clone the frame body"
    );
    assert_eq!(emitted.payload, vec![91; 32768]);
    assert_eq!(emitted.envelope.event_id, "tail-cause:llamacpp:27");
    assert_eq!(emitted.envelope.causation_id.as_deref(), Some("tail-cause"));
    assert_eq!(emitted.envelope.source, endpoint("head"));
    assert_eq!(emitted.envelope.target, endpoint("next"));
    assert_eq!(emitted.envelope.class, EventClass::Data);
    assert_eq!(emitted.envelope.sequence, 27);
    assert_eq!(emitted.envelope.correlation_id, base.correlation_id);
    assert_eq!(emitted.envelope.return_route, base.return_route);
    assert_eq!(emitted.envelope.deadline_unix_ms, base.deadline_unix_ms);
    assert_eq!(
        emitted.envelope.payload_content_type,
        PHYSICAL_BATCH_CONTENT_TYPE
    );
    assert_eq!(
        p4_protocol::event::decode(&p4_protocol::event::encode(&emitted).unwrap()).unwrap(),
        emitted
    );
    assert_eq!(worker.state.next_event, 28);
    assert!(worker.effects.is_empty());
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn envelope_only_output_effects_keep_wire_identity_body_and_order() {
    let (mut worker, mailbox) = fixture(8);
    let base = input(65536);
    let texts = ["\u{ac00}", "\u{1f642}", "\u{b05d}"];
    worker.effects = outputs(&base, &texts);
    worker.state.next_event = 31;
    let expected_reply: ReplySpec = serde_json::from_str(&owner().reply).unwrap();
    for effect in &worker.effects {
        let CommittedEffect::Output { base: retained, .. } = effect else {
            panic!()
        };
        assert_eq!(retained, &base.envelope);
    }
    worker.flush_effects().unwrap();
    for (index, text) in texts.iter().enumerate() {
        let event = next(&mailbox);
        let bytes = p4_protocol::event::encode(&event).unwrap();
        let decoded = p4_protocol::event::decode(&bytes).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(
            event.envelope.event_id,
            format!("tail-cause:llamacpp:{}", 31 + index)
        );
        assert_eq!(event.envelope.sequence, 31 + index as u64);
        assert_eq!(event.envelope.causation_id.as_deref(), Some("tail-cause"));
        assert_eq!(event.envelope.source, endpoint("head"));
        assert_eq!(
            event.envelope.target,
            Endpoint::outer(
                Address::from_str(&expected_reply.ingress_agent).unwrap(),
                expected_reply.channel.clone(),
                expected_reply.connection_generation
            )
        );
        assert_eq!(event.envelope.correlation_id, expected_reply.correlation_id);
        let Endpoint::Outer(expected_return) = &event.envelope.target else {
            panic!("output must address its original OUTER")
        };
        assert_eq!(event.envelope.return_route.as_ref(), Some(expected_return));
        assert_eq!(
            event.envelope.deadline_unix_ms,
            expected_reply.deadline_unix_ms
        );
        assert_eq!(event.envelope.class, EventClass::Output);
        assert_eq!(event.envelope.payload_content_type, OUTPUT_CONTENT_TYPE);
        let value: ApprovedOutputPayload = serde_json::from_slice(&event.payload).unwrap();
        assert_eq!(value.validate(), Ok(()));
        assert_eq!(value.submission_event_id, "original-submission");
        assert_eq!(value.incarnation, 1);
        assert_eq!(
            (value.outcome.load_generation, value.outcome.sequence_id),
            (1, 0)
        );
        assert_eq!(value.outcome.session_id, "session");
        assert_eq!(value.outcome.request_id, "request");
        assert_eq!(value.outcome.token, 100 + index as i32);
        assert_eq!(value.outcome.text, *text);
        assert_eq!(value.outcome.position, 11 + index as u32);
        assert_eq!(value.outcome.stop, None);
        assert_eq!(value.release_operation_id, None);
        assert_eq!(value.issued_work, None);
    }
    assert_eq!(worker.state.next_event, 34);
    assert!(worker.effects.is_empty());
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn failed_forward_restores_the_same_body_telemetry_and_queued_suffix() {
    for failure in ["closed", "id-exhausted", "full-at-shutdown"] {
        let (mut worker, mailbox) = fixture(1);
        let base = input(65536).envelope;
        let body = vec![71; 32768];
        let allocation = body.as_ptr();
        let deliveries = vec![telemetry(&base, "first"), telemetry(&base, "second")];
        let delivery_allocation = deliveries.as_ptr();
        let deliveries_before = format!("{deliveries:?}");
        worker.effects.push_back(CommittedEffect::ForwardObserved {
            base: base.clone(),
            target: endpoint("next"),
            class: EventClass::Data,
            content_type: PHYSICAL_BATCH_CONTENT_TYPE,
            body,
            telemetry: deliveries,
        });
        worker.effects.extend(outputs(&input(0), &["suffix"]));
        let before = format!("{:?}", worker.effects);
        let suffix_before = format!("{:?}", worker.effects[1]);
        let mut mailbox = Some(mailbox);
        match failure {
            "closed" => drop(mailbox.take()),
            "id-exhausted" => worker.state.next_event = u64::MAX,
            "full-at-shutdown" => {
                worker.publisher.try_publish(input(0)).unwrap();
                worker.shutting_down.store(true, Ordering::SeqCst);
            }
            _ => unreachable!(),
        }
        let old_id = worker.state.next_event;
        assert_eq!(
            worker.flush_effects().unwrap_err(),
            "committed physical result could not be delivered"
        );
        assert!(worker.effects_fenced);
        assert_eq!(worker.effects.len(), 2);
        assert_eq!(format!("{:?}", worker.effects[1]), suffix_before);
        let (body, telemetry) = if failure == "id-exhausted" {
            // No Event can be materialized, so the untouched DTO remains.
            assert_eq!(format!("{:?}", worker.effects), before);
            let CommittedEffect::ForwardObserved {
                body, telemetry, ..
            } = &worker.effects[0]
            else {
                panic!("ID exhaustion must preserve the unmaterialized intent")
            };
            (body, telemetry)
        } else {
            let CommittedEffect::Publication {
                event,
                after: PublicationAfter::Observed(telemetry),
            } = &worker.effects[0]
            else {
                panic!("a failed publication must keep its already allocated Event")
            };
            let mut expected_envelope = base.clone();
            expected_envelope.event_id = format!("tail-cause:llamacpp:{old_id}");
            expected_envelope.causation_id = Some("tail-cause".into());
            expected_envelope.source = endpoint("head");
            expected_envelope.target = endpoint("next");
            expected_envelope.class = EventClass::Data;
            expected_envelope.sequence = old_id;
            expected_envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
            assert_eq!(event.envelope, expected_envelope);
            assert_eq!(event.payload, vec![71; 32768]);
            (&event.payload, telemetry)
        };
        assert_eq!(
            body.as_ptr(),
            allocation,
            "{failure}: original body allocation"
        );
        assert_eq!(
            telemetry.as_ptr(),
            delivery_allocation,
            "{failure}: original observation allocations"
        );
        assert_eq!(
            format!("{telemetry:?}"),
            deliveries_before,
            "all recipients, provenance and observation fields remain unchanged"
        );
        for delivery in telemetry {
            let TelemetryPayload::Span(span) = &delivery.payload else {
                panic!()
            };
            assert_eq!(
                span.forward_unix_ms, 0,
                "a failed forward cannot promote telemetry"
            );
        }
        assert!(matches!(worker.effects[1], CommittedEffect::Output { .. }));
        assert_eq!(
            worker.state.next_event,
            if failure == "id-exhausted" {
                old_id
            } else {
                old_id + 1
            }
        );
        assert!(
            worker.flush_effects().is_err(),
            "fenced retry must not consume another ID"
        );
        if let Some(mailbox) = mailbox {
            if failure == "full-at-shutdown" {
                assert_eq!(next(&mailbox), input(0));
            }
            assert!(matches!(mailbox.try_take(), Poll::Empty));
        }
    }
}

#[test]
fn successful_forward_moves_telemetry_before_suffix_and_does_not_retain_forward() {
    let (mut worker, mailbox) = fixture(4);
    let base = input(65536).envelope;
    let body = vec![63; 32768];
    let body_allocation = body.as_ptr();
    let deliveries = vec![telemetry(&base, "first"), telemetry(&base, "second")];
    let original_executions = deliveries
        .iter()
        .map(|d| {
            let TelemetryPayload::Span(span) = &d.payload else {
                panic!()
            };
            span.executions.as_ptr()
        })
        .collect::<Vec<_>>();
    worker.effects.push_back(CommittedEffect::ForwardObserved {
        base,
        target: endpoint("next"),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body,
        telemetry: deliveries,
    });
    worker.effects.extend(outputs(&input(0), &["suffix"]));
    worker.state.next_event = u64::MAX - 1;
    assert_eq!(
        worker.flush_effects().unwrap_err(),
        "committed observation could not be delivered"
    );
    assert!(worker.effects_fenced);
    let forward = next(&mailbox);
    assert_eq!(forward.payload.as_ptr(), body_allocation);
    assert_eq!(worker.effects.len(), 3);
    let mut stamp = None;
    for (index, name) in ["first", "second"].iter().enumerate() {
        let CommittedEffect::Telemetry(delivery) = &worker.effects[index] else {
            panic!("forward retained or telemetry reordered")
        };
        let TelemetryPayload::Span(span) = &delivery.payload else {
            panic!()
        };
        assert_eq!(delivery.reply.correlation_id, *name);
        assert_eq!(
            span.executions.as_ptr(),
            original_executions[index],
            "telemetry must move rather than clone"
        );
        assert!(span.forward_unix_ms >= span.end_unix_ms);
        assert_ne!(span.forward_unix_ms, 0);
        assert_eq!(
            *stamp.get_or_insert(span.forward_unix_ms),
            span.forward_unix_ms
        );
    }
    assert!(matches!(worker.effects[2], CommittedEffect::Output { .. }));
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    assert!(worker.flush_effects().is_err());
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn failed_output_keeps_its_original_text_and_remaining_output_order() {
    let (mut worker, mailbox) = fixture(4);
    drop(mailbox);
    worker.effects = outputs(&input(65536), &["first text", "second text"]);
    let original_text = worker
        .effects
        .iter()
        .map(|effect| {
            let CommittedEffect::Output { payload, .. } = effect else {
                panic!()
            };
            payload.outcome.text.as_ptr()
        })
        .collect::<Vec<_>>();
    let CommittedEffect::Output {
        base,
        reply,
        ingress,
        payload,
    } = &worker.effects[0]
    else {
        panic!()
    };
    let expected_payload = serde_json::to_vec(payload).unwrap();
    let suffix_before = format!("{:?}", worker.effects[1]);
    let old_id = worker.state.next_event;
    let mut expected_envelope = base.clone();
    expected_envelope.event_id = format!("tail-cause:llamacpp:{old_id}");
    expected_envelope.causation_id = Some("tail-cause".into());
    expected_envelope.source = endpoint("head");
    expected_envelope.target = Endpoint::outer(
        ingress.clone(),
        reply.channel.clone(),
        reply.connection_generation,
    );
    expected_envelope.class = EventClass::Output;
    expected_envelope.sequence = old_id;
    expected_envelope.payload_content_type = OUTPUT_CONTENT_TYPE.into();
    expected_envelope.correlation_id = reply.correlation_id.clone();
    expected_envelope.deadline_unix_ms = reply.deadline_unix_ms;
    let Endpoint::Outer(route) = &expected_envelope.target else {
        unreachable!()
    };
    expected_envelope.return_route = Some(route.clone());
    assert_eq!(
        worker.flush_effects().unwrap_err(),
        "committed output could not be delivered"
    );
    assert!(worker.effects_fenced);
    assert_eq!(worker.effects.len(), 2);
    let CommittedEffect::Publication {
        event,
        after: PublicationAfter::OutputTrace { .. },
    } = &worker.effects[0]
    else {
        panic!("serialized output must survive as its exact Event, not a mutable DTO")
    };
    assert_eq!(event.envelope, expected_envelope);
    assert_eq!(event.payload, expected_payload);
    assert_eq!(
        event.envelope.event_id,
        format!("tail-cause:llamacpp:{old_id}")
    );
    assert_eq!(event.envelope.sequence, old_id);
    assert_eq!(event.envelope.payload_content_type, OUTPUT_CONTENT_TYPE);
    let frozen = event.clone();
    let allocation = event.payload.as_ptr();
    assert_eq!(format!("{:?}", worker.effects[1]), suffix_before);
    let CommittedEffect::Output { payload, .. } = &worker.effects[1] else {
        panic!()
    };
    assert_eq!(payload.outcome.text.as_ptr(), original_text[1]);
    assert!(worker.flush_effects().is_err());
    let CommittedEffect::Publication { event, .. } = &worker.effects[0] else {
        panic!()
    };
    assert_eq!(event, &frozen);
    assert_eq!(event.payload.as_ptr(), allocation);
    assert_eq!(worker.state.next_event, old_id + 1);
}
