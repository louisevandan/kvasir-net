//! Actual flush -> completion mailbox consumers. Test-only reconnection and
//! fence removal inspect a retained value; they are NOT production replay or
//! native reconciliation APIs, and establish no byte/transport reservation.
use super::effects::{CommittedEffect, PublicationAfter};
use super::observe::{PreparedTelemetry, TelemetryPayload};
use super::*;
use p4_adapter::node_adapter::{
    CompletionMailbox, Poll, completion_mailbox, completion_mailbox_with_budget,
};

fn endpoint(name: &str) -> Endpoint {
    Endpoint::node(Address::tcp("127.0.0.1", 42631), name, 7)
}

fn worker(capacity: usize) -> (Worker, Arc<CompletionMailbox>) {
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

fn cause() -> Event {
    let mut event = crate::v2::tests::request_state(vec![11]).template.clone();
    event.envelope.event_id = "fixed-cause".into();
    event.envelope.correlation_id = "physical-correlation".into();
    event.envelope.source = endpoint("tail");
    event.envelope.target = endpoint("head");
    event.envelope.deadline_unix_ms = Some(87654);
    event
}

fn reply() -> ReplySpec {
    ReplySpec {
        ingress_agent: Address::tcp("127.0.0.1", 42632).to_string(),
        channel: "private-reply".into(),
        connection_generation: 23,
        correlation_id: "original-request".into(),
        deadline_unix_ms: Some(123456),
    }
}

fn forward(body: Vec<u8>) -> CommittedEffect {
    CommittedEffect::Forward {
        base: cause().envelope,
        target: endpoint("next"),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body,
    }
}

fn telemetry(name: &str) -> PreparedTelemetry {
    let mut reply = reply();
    reply.correlation_id = name.into();
    PreparedTelemetry {
        base: cause().envelope,
        ingress: Address::from_str(&reply.ingress_agent).unwrap(),
        reply,
        payload: TelemetryPayload::Span(StageSpan {
            load_generation: 1,
            session_id: "session".into(),
            execution_ids: vec![31],
            executions: Vec::new(),
            rows: 1,
            ingress_unix_ms: 10,
            start_unix_ms: 11,
            end_unix_ms: 12,
            forward_unix_ms: 0,
        }),
    }
}

fn observed() -> CommittedEffect {
    CommittedEffect::ForwardObserved {
        base: cause().envelope,
        target: endpoint("next"),
        class: EventClass::Data,
        content_type: PHYSICAL_BATCH_CONTENT_TYPE,
        body: vec![0, 255, 128, 3],
        telemetry: vec![telemetry("first"), telemetry("second")],
    }
}

fn publication(worker: &Worker) -> (&Event, &PublicationAfter) {
    let CommittedEffect::Publication { event, after } = &worker.effects[0] else {
        panic!("failed publication must retain a whole immutable Event")
    };
    (event, after)
}

fn take(mailbox: &CompletionMailbox) -> Event {
    let Poll::Event(event) = mailbox.try_take() else {
        panic!("missing completion")
    };
    event
}

fn reconnect_for_test(worker: &mut Worker, capacity: usize) -> Arc<CompletionMailbox> {
    let (publisher, mailbox) = completion_mailbox(capacity);
    worker.publisher = publisher;
    worker.shutting_down.store(false, Ordering::SeqCst);
    // The production fence deliberately has no such automatic recovery path.
    worker.effects_fenced = false;
    mailbox
}

/// A missing hook or capacity regression terminates and fails, never hangs the
/// test suite. Normal operation closes the guard before its deadline expires.
fn guarded_flush(worker: &mut Worker) -> Result<(), String> {
    let shutdown = Arc::clone(&worker.shutting_down);
    let (finished, completion) = mpsc::channel();
    let guard = std::thread::spawn(move || {
        if matches!(
            completion.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            shutdown.store(true, Ordering::SeqCst);
        }
    });
    let result = worker.flush_effects();
    let _ = finished.send(());
    guard.join().unwrap();
    result
}

#[test]
fn final_forward_failures_retain_the_exact_event_id_bytes_and_fifo_position() {
    for failure in ["closed", "full-at-shutdown", "too-large"] {
        let (mut worker, mailbox) = worker(1);
        let mut original_mailbox = Some(mailbox);
        match failure {
            "closed" => drop(original_mailbox.take()),
            "full-at-shutdown" => {
                worker.publisher.try_publish(cause()).unwrap();
                worker.shutting_down.store(true, Ordering::SeqCst);
            }
            "too-large" => {
                let (publisher, mailbox) = completion_mailbox_with_budget(1, 0).unwrap();
                worker.publisher = publisher;
                original_mailbox = Some(mailbox);
            }
            _ => unreachable!(),
        }
        let mut body = Vec::with_capacity(2048);
        body.extend_from_slice(&[0, 255, 128, 3]);
        let allocation = body.as_ptr();
        let capacity = body.capacity();
        worker.state.next_event = 41;
        worker.effects.push_back(forward(body));
        worker.effects.push_back(forward(vec![9, 8, 7]));
        assert_eq!(
            worker.flush_effects().unwrap_err(),
            "committed control could not be delivered"
        );
        assert!(worker.effects_fenced);
        assert_eq!(worker.state.next_event, 42);
        let (retained, after) = publication(&worker);
        assert!(matches!(after, PublicationAfter::Forward));
        assert_eq!(retained.payload.as_ptr(), allocation);
        assert_eq!(retained.payload.capacity(), capacity);
        assert_eq!(retained.envelope.event_id, "fixed-cause:llamacpp:41");
        assert_eq!(retained.envelope.sequence, 41);
        assert_eq!(retained.envelope.source, endpoint("head"));
        assert_eq!(retained.envelope.target, endpoint("next"));
        assert_eq!(
            retained.envelope.correlation_id,
            cause().envelope.correlation_id
        );
        assert_eq!(
            retained.envelope.causation_id.as_deref(),
            Some("fixed-cause")
        );
        assert_eq!(
            retained.envelope.return_route,
            cause().envelope.return_route
        );
        assert_eq!(retained.envelope.deadline_unix_ms, Some(87654));
        let wire = p4_protocol::event::encode(retained).unwrap();
        assert!(matches!(worker.effects[1], CommittedEffect::Forward { .. }));
        assert!(worker.flush_effects().unwrap_err().contains("fenced"));
        assert_eq!(worker.state.next_event, 42);
        assert_eq!(
            p4_protocol::event::encode(publication(&worker).0).unwrap(),
            wire
        );
        let mailbox = reconnect_for_test(&mut worker, 2);
        worker.flush_effects().unwrap();
        let retried = take(&mailbox);
        assert_eq!(
            p4_protocol::event::encode(&retried).unwrap(),
            wire,
            "{failure}"
        );
        assert_eq!(retried.payload.as_ptr(), allocation);
        assert_eq!(retried.payload.capacity(), capacity);
        let suffix = take(&mailbox);
        assert_eq!(suffix.envelope.sequence, 42);
        assert_eq!(suffix.payload, [9, 8, 7]);
        assert_eq!(worker.state.next_event, 43);
        assert!(worker.effects.is_empty());
        assert_eq!(mailbox.try_take(), Poll::Empty);
        drop(original_mailbox);
    }
}

#[test]
fn reply_publications_freeze_routing_payload_and_id_before_final_failure() {
    let reply = reply();
    let ingress = Address::from_str(&reply.ingress_agent).unwrap();
    let output = ApprovedOutputPayload {
        outcome: OutcomePayload {
            load_generation: 1,
            session_id: "session".into(),
            request_id: "request".into(),
            sequence_id: 3,
            token: 11,
            text: "normal UTF-8 response".into(),
            position: 18,
            stop: None,
        },
        submission_event_id: "submission".into(),
        incarnation: 1,
        release_operation_id: None,
        issued_work: None,
    };
    output.validate().unwrap();
    let receipt = ReleaseReceipt {
        load_generation: 1,
        session_id: "session".into(),
        members: vec![ReleaseMember {
            request_id: "request".into(),
            submission_event_id: "submission".into(),
            sequence_id: 3,
            incarnation: 1,
            operation_id: 7,
        }],
    };
    receipt.validate().unwrap();
    let mut observation = telemetry(&reply.correlation_id);
    observation.forwarded_at(123);
    let TelemetryPayload::Span(span) = &observation.payload else {
        panic!()
    };
    let cases = [
        (
            CommittedEffect::Output {
                base: cause().envelope,
                reply: reply.clone(),
                ingress: ingress.clone(),
                payload: output.clone(),
            },
            serde_json::to_vec(&output).unwrap(),
            OUTPUT_CONTENT_TYPE,
            EventClass::Output,
        ),
        (
            CommittedEffect::ReleaseReceipt {
                base: cause().envelope,
                reply: reply.clone(),
                ingress: ingress.clone(),
                payload: receipt.clone(),
            },
            serde_json::to_vec(&receipt).unwrap(),
            RELEASE_RECEIPT_CONTENT_TYPE,
            EventClass::Telemetry,
        ),
        (
            CommittedEffect::Telemetry(observation.clone()),
            serde_json::to_vec(span).unwrap(),
            STAGE_SPAN_CONTENT_TYPE,
            EventClass::Telemetry,
        ),
    ];
    for (effect, expected_payload, content_type, class) in cases {
        let (mut worker, mailbox) = worker(1);
        drop(mailbox);
        worker.state.next_event = 27;
        worker.effects.push_back(effect);
        assert!(worker.flush_effects().is_err());
        let (event, _) = publication(&worker);
        assert_eq!(event.payload, expected_payload);
        assert_eq!(event.envelope.event_id, "fixed-cause:llamacpp:27");
        assert_eq!(event.envelope.sequence, 27);
        assert_eq!(event.envelope.class, class);
        assert_eq!(event.envelope.payload_content_type, content_type);
        assert_eq!(event.envelope.source, endpoint("head"));
        let target = Endpoint::outer(
            ingress.clone(),
            reply.channel.clone(),
            reply.connection_generation,
        );
        assert_eq!(event.envelope.target, target);
        assert_eq!(event.envelope.correlation_id, reply.correlation_id);
        assert_eq!(event.envelope.deadline_unix_ms, reply.deadline_unix_ms);
        let Endpoint::Outer(route) = target else {
            panic!()
        };
        assert_eq!(event.envelope.return_route.as_ref(), Some(&route));
        let pointer = event.payload.as_ptr();
        let wire = p4_protocol::event::encode(event).unwrap();
        assert_eq!(worker.effects[0].event_count().unwrap(), 0);
        assert_eq!(worker.state.next_event, 28);
        // No unallocated ID is required to send an already frozen Event.
        worker.state.next_event = u64::MAX;
        worker.ensure_event_id_obligations(0, 0).unwrap();
        let mailbox = reconnect_for_test(&mut worker, 1);
        worker.flush_effects().unwrap();
        let retried = take(&mailbox);
        assert_eq!(p4_protocol::event::encode(&retried).unwrap(), wire);
        assert_eq!(retried.payload.as_ptr(), pointer);
        assert_eq!(worker.state.next_event, u64::MAX);
        assert!(worker.effects.is_empty());
    }
}

#[test]
fn unallocated_id_failure_preserves_intent_but_frozen_forward_owes_only_its_observers() {
    let (mut worker, mailbox) = worker(1);
    worker.state.next_event = u64::MAX;
    worker.effects.push_back(observed());
    let original = format!("{:?}", worker.effects);
    assert!(
        worker
            .flush_effects()
            .unwrap_err()
            .contains("physical result")
    );
    assert_eq!(format!("{:?}", worker.effects), original);
    assert_eq!(worker.state.next_event, u64::MAX);
    assert_eq!(worker.effects[0].event_count().unwrap(), 3);
    assert_eq!(mailbox.try_take(), Poll::Empty);
    worker.effects_fenced = false;
    worker.state.next_event = u64::MAX - 3;
    drop(mailbox);
    assert!(worker.flush_effects().is_err());
    assert!(matches!(
        publication(&worker).1,
        PublicationAfter::Observed(_)
    ));
    assert_eq!(worker.state.next_event, u64::MAX - 2);
    assert_eq!(worker.effects[0].event_count().unwrap(), 2);
    worker.ensure_event_id_obligations(0, 0).unwrap();
    assert!(worker.ensure_event_id_obligations(1, 0).is_err());
}

#[test]
fn active_frozen_forward_keeps_every_observation_id_share_while_full_services_an_ack() {
    let (mut worker, mailbox) = worker(1);
    worker.publisher.try_publish(cause()).unwrap();
    let (sender, receiver) = mpsc::channel();
    worker.receiver = receiver;
    let mut invalid_ack = cause();
    invalid_ack.envelope.payload_content_type = RELEASED_CONTENT_TYPE.into();
    invalid_ack.payload = b"not-json".to_vec();
    sender
        .send(WorkerInput::Event(invalid_ack.clone()))
        .unwrap();
    let held_mailbox = Arc::new(Mutex::new(Some(mailbox)));
    let close = Arc::clone(&held_mailbox);
    worker.issue_observer = Some(Arc::new(move |point, _| {
        if point == "publication_full_after_ack" {
            let mailbox = close.lock().unwrap().take();
            drop(mailbox);
        }
    }));
    worker.state.next_event = u64::MAX - 3;
    worker.effects.push_back(observed());
    assert!(
        guarded_flush(&mut worker)
            .unwrap_err()
            .contains("physical result")
    );
    assert!(
        held_mailbox.lock().unwrap().is_none(),
        "the actual Full/ACK consumer must run"
    );
    assert_eq!(worker.held_input.as_ref().map(WorkerInput::event), Some(&invalid_ack));
    assert!(
        worker.deferred_ack_error.is_none(),
        "no ID remains for a diagnostic after both observers"
    );
    assert_eq!(worker.state.next_event, u64::MAX - 2);
    assert_eq!(worker.effects[0].event_count().unwrap(), 2);
    assert_eq!(worker.active_effect_ids, 0);
    assert_eq!(worker.active_publications, 0);
}

#[test]
fn accepted_forward_is_not_repeated_when_a_frozen_observation_fails() {
    let (mut worker, mailbox) = worker(1);
    let held_mailbox = Arc::new(Mutex::new(Some(mailbox)));
    let close = Arc::clone(&held_mailbox);
    let forwarded = Arc::new(Mutex::new(None));
    let accepted = Arc::clone(&forwarded);
    worker.issue_observer = Some(Arc::new(move |point, _| {
        if point == "publication_full_after_ack" {
            let mailbox = close
                .lock()
                .unwrap()
                .take()
                .expect("one deterministic Full boundary");
            *accepted.lock().unwrap() = Some(take(&mailbox));
            drop(mailbox);
        }
    }));
    worker.state.next_event = 37;
    worker.effects.push_back(observed());
    worker.effects.push_back(forward(vec![9]));
    assert!(
        guarded_flush(&mut worker)
            .unwrap_err()
            .contains("observation")
    );
    let accepted = forwarded
        .lock()
        .unwrap()
        .take()
        .expect("forward must be accepted first");
    assert_eq!(accepted.envelope.sequence, 37);
    assert_eq!(
        accepted.envelope.payload_content_type,
        PHYSICAL_BATCH_CONTENT_TYPE
    );
    assert_eq!(worker.state.next_event, 39);
    assert_eq!(worker.effects.len(), 3);
    let (failed, after) = publication(&worker);
    assert!(matches!(after, PublicationAfter::Telemetry));
    assert_eq!(failed.envelope.sequence, 38);
    let first_span: StageSpan = serde_json::from_slice(&failed.payload).unwrap();
    assert_ne!(first_span.forward_unix_ms, 0);
    let wire = p4_protocol::event::encode(failed).unwrap();
    let allocation = failed.payload.as_ptr();
    let CommittedEffect::Telemetry(second) = &worker.effects[1] else {
        panic!("second observer order changed")
    };
    let TelemetryPayload::Span(second_span) = &second.payload else {
        panic!()
    };
    assert_eq!(first_span.forward_unix_ms, second_span.forward_unix_ms);
    let mailbox = reconnect_for_test(&mut worker, 3);
    worker.flush_effects().unwrap();
    let first = take(&mailbox);
    assert_eq!(p4_protocol::event::encode(&first).unwrap(), wire);
    assert_eq!(first.payload.as_ptr(), allocation);
    let second = take(&mailbox);
    assert_eq!(second.envelope.sequence, 39);
    assert_eq!(second.envelope.correlation_id, "second");
    let second_span: StageSpan = serde_json::from_slice(&second.payload).unwrap();
    assert_eq!(second_span.forward_unix_ms, first_span.forward_unix_ms);
    let suffix = take(&mailbox);
    assert_eq!(suffix.envelope.sequence, 40);
    assert_eq!(suffix.payload, [9]);
    assert!(worker.effects.is_empty());
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn native_precondition_failure_retains_native_intent_and_does_not_materialize_its_suffix() {
    let (mut worker, mailbox) = worker(2);
    worker.effects.push_back(CommittedEffect::Release {
        load_generation: 1,
        session_id: "missing-session".into(),
        sequence: ReleaseSequence {
            key: "missing-session\0request".into(),
            id: 3,
            incarnation: 1,
            operation_id: 7,
        },
    });
    worker.effects.push_back(forward(vec![1, 2]));
    let original = format!("{:?}", worker.effects);
    let next_event = worker.state.next_event;
    assert_eq!(
        worker.flush_effects().unwrap_err(),
        "head control session is missing"
    );
    assert!(worker.effects_fenced);
    assert_eq!(format!("{:?}", worker.effects), original);
    assert_eq!(worker.state.next_event, next_event);
    assert_eq!(worker.effects[0].event_count().unwrap(), 0);
    assert_eq!(worker.active_effect_ids, 0);
    assert_eq!(mailbox.try_take(), Poll::Empty);
    assert!(worker.flush_effects().unwrap_err().contains("fenced"));
    assert_eq!(format!("{:?}", worker.effects), original);
}
