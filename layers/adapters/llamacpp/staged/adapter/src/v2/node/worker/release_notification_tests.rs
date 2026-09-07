//! Head ACK consumption and committed notification delivery with real mailboxes.
//! These tests call Worker::released; they do not run Worker::run or native KV.
use super::release_tests::{event, fixture, sequence};
use super::*;
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use std::sync::atomic::AtomicUsize;
use std::task::{Context, Wake, Waker};

struct NativeProbe(Arc<AtomicUsize>);

impl crate::process::ServerControl for NativeProbe {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn wait_ready(
        &mut self,
        _deadline: Instant,
    ) -> Result<Option<crate::process::ReadyInfo>, String> {
        Ok(Some(crate::process::ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "release-notification-no-model".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 128,
            n_batch: 8,
            n_ubatch: 8,
            n_seq_max: 4,
            upstream_commit: "fixture".into(),
            patch_set: "fixture".into(),
            backend_inventory: "no-engine".into(),
        }))
    }
    fn request(&mut self, _frame: Frame) -> Result<Frame, String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err("a release ACK must not execute native work".into())
    }
    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn owner(name: &str) -> ReleaseSequence {
    let mut value = sequence(name, if name == "a" { 0 } else { 1 });
    value.incarnation = if name == "a" { 41 } else { 42 };
    value.operation_id = if name == "a" { 101 } else { 102 };
    value
}

fn receipt(name: &str) -> ReleaseReceipt {
    ReleaseReceipt {
        load_generation: 1,
        session_id: "pipeline".into(),
        members: vec![ReleaseMember {
            request_id: name.into(),
            submission_event_id: format!("original-submission-{name}"),
            sequence_id: if name == "a" { 0 } else { 1 },
            incarnation: if name == "a" { 41 } else { 42 },
            operation_id: if name == "a" { 101 } else { 102 },
        }],
    }
}

fn route(name: &str) -> Endpoint {
    Endpoint::outer(
        Address::tcp("127.0.0.1", 42001),
        format!("owner-{name}"),
        if name == "a" { 7 } else { 9 },
    )
}

fn prepared() -> (Worker, Arc<AtomicUsize>) {
    let (mut worker, _mailbox) = fixture();
    for name in ["a", "b"] {
        let mut pending = super::release_tests::pending(owner(name));
        let Endpoint::Outer(outer) = route(name) else {
            unreachable!()
        };
        pending.original.source = Endpoint::Outer(outer.clone());
        pending.original.return_route = Some(outer.clone());
        pending.original.correlation_id = format!("correlation-{name}");
        pending.original.deadline_unix_ms = Some(if name == "a" { 1001 } else { 1002 });
        pending.reply.channel = outer.channel;
        pending.reply.connection_generation = outer.connection_generation;
        pending.reply.correlation_id = pending.original.correlation_id.clone();
        pending.reply.deadline_unix_ms = pending.original.deadline_unix_ms;
        worker
            .state
            .pending_releases
            .insert(pending.sequence.key.clone(), pending);
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let worker = worker
        .with_stage_for_test(Box::new(NativeProbe(calls.clone())))
        .unwrap();
    (worker, calls)
}

fn ack() -> Event {
    event(vec![owner("a"), owner("b")])
}

fn assert_committed(worker: &Worker, calls: &AtomicUsize) {
    assert!(worker.state.pending_releases.is_empty());
    assert_eq!(
        worker
            .state
            .free_sequences
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(!worker.state.verify_fenced());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "acknowledging release must not release native KV again"
    );
}

fn assert_received(event: &Event, name: &str) {
    assert_eq!(
        event.envelope.payload_content_type,
        RELEASE_RECEIPT_CONTENT_TYPE
    );
    assert_eq!(event.envelope.class, EventClass::Telemetry);
    assert_eq!(
        event.envelope.source,
        Endpoint::node(Address::tcp("127.0.0.1", 42001), "head", 1)
    );
    assert_eq!(event.envelope.target, route(name));
    assert_eq!(
        event
            .envelope
            .return_route
            .as_ref()
            .map(|outer| Endpoint::Outer(outer.clone())),
        Some(route(name))
    );
    assert_eq!(event.envelope.correlation_id, format!("correlation-{name}"));
    assert_eq!(
        event.envelope.causation_id.as_deref(),
        Some(format!("original-submission-{name}").as_str())
    );
    assert_eq!(
        event.envelope.deadline_unix_ms,
        Some(if name == "a" { 1001 } else { 1002 })
    );
    let actual: ReleaseReceipt = serde_json::from_slice(&event.payload).unwrap();
    assert_eq!(actual, receipt(name));
}

fn assert_retained(worker: &Worker, names: &[&str]) {
    assert_eq!(worker.effects.len(), names.len());
    for (effect, name) in worker.effects.iter().zip(names) {
        let super::effects::CommittedEffect::ReleaseReceipt {
            base,
            reply,
            ingress,
            payload,
        } = effect
        else {
            panic!("only exact release receipt intents may remain after ACK: {effect:?}");
        };
        // Provenance has no payload field: the type cannot retain prompt bytes.
        let _: &p4_protocol::event::Envelope = base;
        assert_eq!(base.event_id, format!("original-submission-{name}"));
        assert_eq!(base.source, route(name));
        assert_eq!(
            Endpoint::outer(
                ingress.clone(),
                reply.channel.clone(),
                reply.connection_generation
            ),
            route(name)
        );
        assert_eq!(reply.correlation_id, format!("correlation-{name}"));
        assert_eq!(
            reply.deadline_unix_ms,
            Some(if *name == "a" { 1001 } else { 1002 })
        );
        assert_eq!(*payload, receipt(name));
    }
}

fn assert_duplicate_preserves_commit(worker: &mut Worker, calls: &AtomicUsize) {
    let before = super::release_tests::snapshot(worker);
    assert!(
        worker.released(ack()).is_err(),
        "an ACK cannot return the same slots twice"
    );
    assert_eq!(super::release_tests::snapshot(worker), before);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn first_closed_receipt_retains_all_committed_member_intents_without_native_reexecution() {
    let (mut worker, calls) = prepared();
    let (publisher, mailbox) = completion_mailbox(1);
    worker.publisher = publisher;
    drop(mailbox);
    assert!(worker.released(ack()).is_err());
    assert_committed(&worker, &calls);
    assert!(worker.effects_fenced);
    assert_retained(&worker, &["a", "b"]);
    assert_duplicate_preserves_commit(&mut worker, &calls);
}

/// The real publisher calls this synchronously after enqueuing its first event.
/// The callback collects it and closes the final receiver before publish returns,
/// so the second event encounters Closed without a sleep/race or a fake publisher.
struct CloseAfterFirst {
    mailbox: Mutex<Option<Arc<CompletionMailbox>>>,
    observed: Arc<Mutex<Vec<Event>>>,
}

impl Wake for CloseAfterFirst {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let mailbox = self
            .mailbox
            .lock()
            .unwrap()
            .take()
            .expect("only the first publish wakes the reader");
        let Poll::Event(event) = mailbox.try_take() else {
            panic!("wake follows the first accepted event")
        };
        self.observed.lock().unwrap().push(event);
        drop(mailbox);
    }
}

#[test]
fn closing_after_one_receipt_keeps_only_the_unpublished_exact_intent() {
    let (mut worker, calls) = prepared();
    let (publisher, mailbox) = completion_mailbox(1);
    worker.publisher = publisher;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let closer = Arc::new(CloseAfterFirst {
        mailbox: Mutex::new(Some(mailbox.clone())),
        observed: observed.clone(),
    });
    let waker = Waker::from(closer);
    let mut context = Context::from_waker(&waker);
    assert!(mailbox.poll_take(&mut context).is_pending());
    drop(mailbox);
    assert!(worker.released(ack()).is_err());
    assert_committed(&worker, &calls);
    assert!(worker.effects_fenced);
    let observed = observed.lock().unwrap();
    assert_eq!(observed.len(), 1);
    assert_received(&observed[0], "a");
    assert_retained(&worker, &["b"]);
    assert_duplicate_preserves_commit(&mut worker, &calls);
}

#[test]
fn receipt_event_identity_exhaustion_preserves_every_unpublished_committed_intent() {
    for (next, emitted, pending) in [(u64::MAX, 0, vec!["a", "b"]), (u64::MAX - 1, 1, vec!["b"])] {
        let (mut worker, calls) = prepared();
        let (publisher, mailbox) = completion_mailbox(2);
        worker.publisher = publisher;
        // Inject loss of ID space after the actual ACK transaction committed.
        // A pre-commit shortage is a different, stronger refusal tested below.
        worker.released_without_flush(ack()).unwrap();
        worker.state.next_event = next;
        assert!(worker.flush_effects().is_err());
        assert_committed(&worker, &calls);
        assert!(worker.effects_fenced);
        let mut events = Vec::new();
        while let Poll::Event(event) = mailbox.try_take() {
            events.push(event);
        }
        assert_eq!(events.len(), emitted);
        if let Some(event) = events.first() {
            assert_received(event, "a");
        }
        assert_retained(&worker, &pending);
        assert_eq!(worker.state.next_event, u64::MAX);
        assert_duplicate_preserves_commit(&mut worker, &calls);
    }
}

#[test]
fn receipt_id_shortage_before_commit_preserves_ack_and_slot_authority() {
    for next in [u64::MAX, u64::MAX - 1] {
        let (mut worker, calls) = prepared();
        let (publisher, mailbox) = completion_mailbox(2);
        worker.publisher = publisher;
        worker.state.next_event = next;
        let pending = worker.state.pending_releases.clone();
        let free = worker.state.free_sequences.clone();
        let error = worker.released(ack()).unwrap_err();
        assert!(error.contains("event ID is exhausted"), "{error}");
        assert_eq!(worker.state.pending_releases, pending);
        assert_eq!(worker.state.free_sequences, free);
        assert_eq!(worker.state.next_event, next);
        assert!(worker.effects.is_empty() && !worker.effects_fenced);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(mailbox.try_take(), Poll::Empty);
    }
}

#[test]
fn direct_responses_cannot_spend_ids_owed_to_pending_receipts() {
    let (mut worker, calls) = prepared();
    let (publisher, mailbox) = completion_mailbox(2);
    worker.publisher = publisher;
    worker.state.next_event = u64::MAX - 2;
    let pending = worker.state.pending_releases.clone();
    assert!(
        worker
            .emit_error(&ack(), "EXTRA", "unreserved response".into())
            .is_err()
    );
    assert_eq!(worker.state.next_event, u64::MAX - 2);
    assert_eq!(worker.state.pending_releases, pending);
    assert_eq!(mailbox.try_take(), Poll::Empty);
    worker.released(ack()).unwrap();
    assert_committed(&worker, &calls);
    for (name, id) in [("a", u64::MAX - 2), ("b", u64::MAX - 1)] {
        let Poll::Event(event) = mailbox.try_take() else {
            panic!("reserved receipt missing")
        };
        assert_received(&event, name);
        assert_eq!(event.envelope.sequence, id);
    }
    assert_eq!(worker.state.next_event, u64::MAX);
    assert!(worker.effects.is_empty());
}

fn wait_until_waiting(snapshot: &Mutex<String>) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while snapshot.lock().unwrap().as_str() != "completion_queue_full:waiting" {
        assert!(
            Instant::now() < deadline,
            "worker never reached actual full-mailbox backpressure"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn receive(mailbox: &CompletionMailbox) -> Event {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Poll::Event(event) = mailbox.try_take() {
            return event;
        }
        assert!(
            Instant::now() < deadline,
            "committed receipt was not delivered after capacity returned"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_full_first_receipt_waits_for_capacity_then_delivers_both_exact_routes_once() {
    let (mut worker, calls) = prepared();
    let (publisher, mailbox) = completion_mailbox(1);
    publisher.try_publish(ack()).unwrap(); // Known sentinel owns the only slot.
    worker.publisher = publisher;
    let snapshot = worker.snapshot.clone();
    let (done, result) = mpsc::channel();
    let join = std::thread::spawn(move || {
        let outcome = worker.released(ack());
        assert!(done.send((worker, outcome)).is_ok());
    });
    wait_until_waiting(&snapshot);
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let sentinel = receive(&mailbox);
    assert_eq!(
        sentinel.envelope.payload_content_type,
        RELEASED_CONTENT_TYPE
    );
    let first = receive(&mailbox);
    let second = receive(&mailbox);
    assert_received(&first, "a");
    assert_received(&second, "b");
    assert_ne!(first.envelope.event_id, second.envelope.event_id);
    let (mut worker, outcome) = result.recv_timeout(Duration::from_secs(3)).unwrap();
    join.join().unwrap();
    outcome.unwrap();
    assert_committed(&worker, &calls);
    assert!(!worker.effects_fenced);
    assert!(worker.effects.is_empty());
    assert!(matches!(mailbox.try_take(), Poll::Empty));
    assert_duplicate_preserves_commit(&mut worker, &calls);
}

#[test]
fn same_outer_route_does_not_merge_distinct_correlations_or_deadlines() {
    let (mut worker, calls) = prepared();
    let pending = worker
        .state
        .pending_releases
        .get_mut(&owner("b").key)
        .unwrap();
    let Endpoint::Outer(outer) = route("a") else {
        unreachable!()
    };
    pending.original.source = Endpoint::Outer(outer.clone());
    pending.original.return_route = Some(outer.clone());
    pending.reply.channel = outer.channel;
    pending.reply.connection_generation = outer.connection_generation;
    let (publisher, mailbox) = completion_mailbox(2);
    worker.publisher = publisher;
    worker.released(ack()).unwrap();
    let first = receive(&mailbox);
    let second = receive(&mailbox);
    assert_received(&first, "a");
    assert_eq!(second.envelope.target, route("a"));
    assert_eq!(second.envelope.correlation_id, "correlation-b");
    assert_eq!(second.envelope.deadline_unix_ms, Some(1002));
    assert_eq!(
        second.envelope.causation_id.as_deref(),
        Some("original-submission-b")
    );
    assert_eq!(
        serde_json::from_slice::<ReleaseReceipt>(&second.payload).unwrap(),
        receipt("b")
    );
    assert_committed(&worker, &calls);
    assert!(worker.effects.is_empty());
    assert!(matches!(mailbox.try_take(), Poll::Empty));
}

#[test]
fn invalid_later_notification_provenance_cannot_partially_return_slots_or_emit_a_receipt() {
    for invalid_submission in [false, true] {
        for reverse in [false, true] {
            let (mut worker, calls) = prepared();
            let pending = worker
                .state
                .pending_releases
                .get_mut(&owner("b").key)
                .unwrap();
            if invalid_submission {
                pending.original.event_id.clear();
            } else {
                pending.reply.ingress_agent = "not-an-address".into();
            }
            let (publisher, mailbox) = completion_mailbox(2);
            worker.publisher = publisher;
            let before = super::release_tests::snapshot(&worker);
            let members = if reverse {
                vec![owner("b"), owner("a")]
            } else {
                vec![owner("a"), owner("b")]
            };
            assert!(worker.released(event(members)).is_err());
            assert_eq!(super::release_tests::snapshot(&worker), before);
            assert!(matches!(mailbox.try_take(), Poll::Empty));
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn valid_but_conflicting_original_and_reply_authority_rejects_the_whole_ack() {
    for field in [
        "ingress",
        "channel",
        "connection",
        "correlation",
        "deadline",
        "source",
        "target",
        "return_route",
    ] {
        for reverse in [false, true] {
            let (mut worker, calls) = prepared();
            let pending = worker
                .state
                .pending_releases
                .get_mut(&owner("b").key)
                .unwrap();
            match field {
                "ingress" => pending.reply.ingress_agent = "tcp://127.0.0.2:42002".into(),
                "channel" => pending.reply.channel = "another-valid-channel".into(),
                "connection" => pending.reply.connection_generation = 10,
                "correlation" => pending.reply.correlation_id = "another-valid-correlation".into(),
                "deadline" => pending.reply.deadline_unix_ms = Some(1003),
                "source" => pending.original.source = route("a"),
                "target" => {
                    pending.original.target =
                        Endpoint::node(Address::tcp("127.0.0.1", 42001), "middle", 1)
                }
                "return_route" => {
                    let Endpoint::Outer(outer) = route("a") else {
                        unreachable!()
                    };
                    pending.original.return_route = Some(outer);
                }
                _ => unreachable!(),
            }
            let (publisher, mailbox) = completion_mailbox(2);
            worker.publisher = publisher;
            let before = super::release_tests::snapshot(&worker);
            let members = if reverse {
                vec![owner("b"), owner("a")]
            } else {
                vec![owner("a"), owner("b")]
            };
            let result = worker.released(event(members));
            assert!(
                result.is_err(),
                "valid-but-conflicting {field}, reverse={reverse}: ACK must not consume either owner"
            );
            assert_eq!(
                super::release_tests::snapshot(&worker),
                before,
                "{field}, reverse={reverse}"
            );
            assert!(
                matches!(mailbox.try_take(), Poll::Empty),
                "{field}, reverse={reverse}"
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "{field}, reverse={reverse}"
            );
        }
    }
}
