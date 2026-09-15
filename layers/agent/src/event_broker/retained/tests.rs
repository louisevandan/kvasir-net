use super::*;
use p4_adapter::node_adapter::{
    COMPLETION_ENTRY_OVERHEAD_BYTES, CompletionMailbox, OwnedPoll, Poll, PublishError,
    completion_mailbox_with_limits,
};
use p4_protocol::event::EventClass;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll as TaskPoll, Wake, Waker};

fn queue(bytes: usize) -> (CompletionPublisher, Arc<CompletionMailbox>) {
    completion_mailbox_with_limits(1, 8, bytes).unwrap()
}
fn own() -> Address {
    Address::tcp("127.0.0.1", 52001)
}
fn event(id: &str) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: id.into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.2", 52001)),
            target: Endpoint::agent(own()),
            return_route: Some(p4_protocol::event::OuterEndpoint { ingress_agent: p4_protocol::Address::tcp("127.0.0.1", 52001), channel: "outer".into(), connection_generation: 1 }),
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("opaque-test".into()),
            payload_content_type: "application/test".into(),
        },
        payload: {
            let mut payload = Vec::with_capacity(4096);
            payload.extend([0, 255, 128, 7]);
            payload
        },
    }
}
fn take(mailbox: &CompletionMailbox) -> RetainedCompletion {
    match mailbox.try_take_owned() {
        OwnedPoll::Event(event) => event,
        other => panic!("{other:?}"),
    }
}
fn make_source(
    event: Event,
) -> (
    CompletionPublisher,
    Arc<CompletionMailbox>,
    RetainedCompletion,
) {
    let (sender, mailbox) = queue(1 << 20);
    let reservation = sender
        .try_reserve(1, retained_event_bytes(&event).unwrap())
        .unwrap();
    sender.publish_reserved(event, reservation).unwrap();
    let completion = take(&mailbox);
    (sender, mailbox, completion)
}
fn broker(bytes: usize, window: usize) -> (Arc<RetainedEventBroker>, Arc<CompletionMailbox>) {
    let (sender, mailbox) = queue(bytes);
    (
        Arc::new(RetainedEventBroker::new(
            own(),
            sender.clone(),
            sender.clone(),
            sender,
            window,
        )),
        mailbox,
    )
}

#[test]
fn outer_output_is_routed_to_reception_agent_and_full_preserves_original() {
    let ingress = Address::tcp("192.0.2.1", 52001);
    let worker = own();
    let (agent_tx, agent_rx) = queue(1 << 20);
    let (outer_tx, outer_rx) = queue(1 << 20);
    let (out_tx, out_rx) = queue(1 << 20);
    let remote = RetainedEventBroker::new(worker.clone(), agent_tx, outer_tx, out_tx, 8);
    let route = Endpoint::outer(ingress.clone(), "private-client-channel", 7);
    let mut first = event("first-output");
    first.envelope.source = Endpoint::node(worker, "backend-neutral", 4);
    first.envelope.target = route.clone();
    first.envelope.return_route = match route { Endpoint::Outer(route) => Some(route), _ => unreachable!() };
    first.envelope.class = EventClass::Output;
    let mut second = first.clone();
    second.envelope.event_id = "second-output".into(); second.envelope.sequence = 2;
    let expected = second.clone();
    remote.dispatch_ingress(first).unwrap();
    let (_, source, pending) = make_source(second);
    let pointer = pending.event().payload.as_ptr();
    let charge = source.storage_snapshot().retained_bytes;
    let before = remote.receipt_snapshot().unwrap().committed_events;
    let failure = remote.dispatch_retained(pending).unwrap_err();
    assert_eq!(failure.error, DispatchError::Full(Delivery::Outbound(ingress.clone())));
    assert_eq!(failure.completion.event(), &expected);
    assert_eq!(failure.completion.event().payload.as_ptr(), pointer);
    assert_eq!(source.storage_snapshot().retained_bytes, charge);
    assert_eq!(remote.receipt_snapshot().unwrap().committed_events, before);
    assert_eq!(outer_rx.storage_snapshot().retained_count, 0);
    assert_eq!(agent_rx.storage_snapshot().retained_count, 0);
    drop(take(&out_rx));
    assert_eq!(remote.dispatch_retained(*failure.completion).unwrap(), DispatchOutcome::Enqueued(Delivery::Outbound(ingress.clone())));
    let forwarded = take(&out_rx);
    assert_eq!(forwarded.event(), &expected);
    assert_eq!(forwarded.event().payload.as_ptr(), pointer);
    assert_eq!(source.storage_snapshot().retained_bytes, 0);

    let (agent_tx, agent_rx) = queue(1 << 20);
    let (outer_tx, outer_rx) = queue(1 << 20);
    let (out_tx, out_rx) = queue(1 << 20);
    let reception = RetainedEventBroker::new(ingress, agent_tx, outer_tx, out_tx, 8);
    assert_eq!(reception.dispatch_retained(forwarded).unwrap(), DispatchOutcome::Enqueued(Delivery::Outer));
    let delivered = take(&outer_rx);
    assert_eq!(delivered.event(), &expected);
    assert_eq!(delivered.event().payload.as_ptr(), pointer);
    assert_eq!(agent_rx.storage_snapshot().retained_count, 0);
    assert_eq!(out_rx.storage_snapshot().retained_count, 0);
}

#[test]
fn owned_runtime_admission_pause_rechecks_reserved_front_and_preserves_exact_duplicate() {
    let (broker, _) = broker(1 << 20, 8);
    let (sender, inbound) = queue(1 << 20);
    broker.register_node("paused", 1, sender).unwrap();
    let mut original = event("paused-event");
    original.envelope.target = Endpoint::node(own(), "paused", 1);
    let replay = original.clone();
    let (_, source, completion) = make_source(original);
    let pointer = completion.event().payload.as_ptr();
    let front = CompletionFront { envelope: completion.event().envelope.clone(),
        event_bytes: retained_event_bytes(completion.event()).unwrap() };
    let ticket = broker.reserve_retained_completion(&front).unwrap();
    let pause = broker.pause_node_admission("paused", 1).unwrap();
    let failed = broker.dispatch_retained_completion(ticket, completion).unwrap_err();
    assert!(matches!(failed.error, DispatchError::Full(_)));
    assert_eq!(failed.completion.event().payload.as_ptr(), pointer);
    assert_eq!(source.storage_snapshot().retained_count, 1);
    assert_eq!(inbound.storage_snapshot().retained_count, 0);
    assert_eq!(inbound.storage_snapshot().reserved_queue_slots, 0);
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(0));
    assert!(matches!(broker.dispatch_ingress(replay.clone()).unwrap_err().error, DispatchError::Full(_)));
    drop(pause);
    broker.dispatch_retained(*failed.completion).unwrap();
    let pause = broker.pause_node_admission("paused", 1).unwrap();
    assert_eq!(broker.dispatch_ingress(replay).unwrap(), DispatchOutcome::Duplicate);
    assert_eq!(take(&inbound).event().payload.as_ptr(), pointer);
    drop(pause);
    assert_eq!(source.storage_snapshot().retained_count, 0);
}

struct UnlockedWake {
    broker: Arc<RetainedEventBroker>,
    calls: AtomicUsize,
}
impl Wake for UnlockedWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        assert!(
            self.broker.ledger.try_lock().is_ok(),
            "callback ran under receipt ledger lock"
        );
        assert!(
            self.broker.nodes.try_write().is_ok(),
            "callback ran under registration lock"
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn retained_handoff_moves_original_and_releases_source_outside_ledger() {
    let (broker, destination) = broker(1 << 20, 8);
    let original = event("move");
    let pointer = original.payload.as_ptr();
    let capacity = original.payload.capacity();
    let (publisher, source, completion) = make_source(original);
    let wake = Arc::new(UnlockedWake {
        broker: broker.clone(),
        calls: AtomicUsize::new(0),
    });
    let _listener = publisher
        .capacity_listener(&Waker::from(wake.clone()))
        .unwrap();
    let waker = Waker::from(wake.clone());
    assert!(matches!(
        destination.poll_take_owned(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    ));
    assert_eq!(
        broker.dispatch_retained(completion).unwrap(),
        DispatchOutcome::Enqueued(Delivery::Agent)
    );
    assert!(wake.calls.load(Ordering::SeqCst) >= 2);
    assert_eq!(source.storage_snapshot().retained_count, 0);
    assert!(
        matches!(destination.try_take(), Poll::Empty),
        "raw reader must not strip the destination claim"
    );
    let held = take(&destination);
    assert_eq!(held.event().payload.as_ptr(), pointer);
    assert_eq!(held.event().payload.capacity(), capacity);
    assert_eq!(destination.storage_snapshot().queued_count, 0);
    assert_eq!(destination.storage_snapshot().retained_count, 1);
    assert_eq!(broker.receipt_snapshot().unwrap().indexed.events, 1);
    held.retire();
    assert_eq!(destination.storage_snapshot().retained_bytes, 0);
    assert_eq!(
        broker.receipt_snapshot().unwrap().indexed.events,
        1,
        "receipt is independent of both delivery claims"
    );
}

#[test]
fn retained_full_and_permanent_limits_preserve_original_claim_and_ledger() {
    let bytes = retained_event_bytes(&event("aaaa")).unwrap() + COMPLETION_ENTRY_OVERHEAD_BYTES;
    let (broker, destination) = broker(bytes, 8);
    broker.dispatch_ingress(event("aaaa")).unwrap();
    let (_, source, completion) = make_source(event("bbbb"));
    let pointer = completion.event().payload.as_ptr();
    let before = source.storage_snapshot();
    let ledger = format!("{:?}", broker.ledger.lock().unwrap());
    let failure = broker.dispatch_retained(completion).unwrap_err();
    assert_eq!(failure.error, DispatchError::Full(Delivery::Agent));
    let occupied = take(&destination);
    let failure = broker.dispatch_retained(*failure.completion).unwrap_err();
    assert_eq!(
        failure.error,
        DispatchError::Full(Delivery::Agent),
        "dequeue alone does not release bytes"
    );
    assert_eq!(source.storage_snapshot(), before);
    assert_eq!(failure.completion.event().payload.as_ptr(), pointer);
    assert_eq!(format!("{:?}", broker.ledger.lock().unwrap()), ledger);
    assert_eq!(destination.storage_snapshot().reserved_queue_slots, 0);
    occupied.retire();
    broker.dispatch_retained(*failure.completion).unwrap();
    assert_eq!(take(&destination).event().payload.as_ptr(), pointer);
    let mut too_large = event("large");
    too_large.payload.reserve_exact(8192);
    let (_, source, completion) = make_source(too_large);
    let before = source.storage_snapshot();
    let failure = broker.dispatch_retained(completion).unwrap_err();
    assert!(matches!(
        failure.error,
        DispatchError::StorageTooLarge { .. }
    ));
    assert_eq!(source.storage_snapshot(), before);
}

#[test]
fn retained_duplicate_and_conflict_precede_full_and_pinned_eviction() {
    let (broker, destination) = broker(1 << 20, 1);
    let first = event("first");
    broker.dispatch_ingress(first.clone()).unwrap();
    let (_, source, completion) = make_source(first.clone());
    assert_eq!(
        broker.dispatch_retained(completion).unwrap(),
        DispatchOutcome::Duplicate
    );
    assert_eq!(source.storage_snapshot().retained_count, 0);
    let mut conflict = first.clone();
    conflict.payload[0] = 42;
    let (_, source, completion) = make_source(conflict);
    let failure = broker.dispatch_retained(completion).unwrap_err();
    assert_eq!(failure.error, DispatchError::ConflictingDuplicate);
    assert_eq!(source.storage_snapshot().retained_count, 1);
    let (_, pinned_source, completion) = make_source(first);
    let front = CompletionFront {
        envelope: completion.event().envelope.clone(),
        event_bytes: retained_event_bytes(completion.event()).unwrap(),
    };
    let ticket = broker.reserve_retained_completion(&front).unwrap();
    take(&destination).retire();
    broker.dispatch_ingress(event("evict")).unwrap();
    assert_eq!(broker.receipt_snapshot().unwrap().retired.events, 1);
    assert_eq!(
        broker
            .dispatch_retained_completion(ticket, completion)
            .unwrap(),
        DispatchOutcome::Duplicate
    );
    assert_eq!(pinned_source.storage_snapshot().retained_count, 0);
    assert_eq!(broker.receipt_snapshot().unwrap().retired.events, 0);
    assert_eq!(take(&destination).event().envelope.event_id, "evict");
}

#[test]
fn retained_front_reserves_real_slot_and_bytes_and_rechecks_route() {
    let (broker, _unused) = broker(1 << 20, 8);
    let (node_sender, destination) = queue(1 << 20);
    broker
        .register_node("node", 1, node_sender.clone())
        .unwrap();
    let mut original = event("front");
    original.envelope.target = Endpoint::node(own(), "node", 1);
    let (publisher, mailbox) = queue(1 << 20);
    let reservation = publisher
        .try_reserve(1, retained_event_bytes(&original).unwrap())
        .unwrap();
    publisher.publish_reserved(original, reservation).unwrap();
    let front = mailbox.peek_owned_front().unwrap();
    let ticket = broker.reserve_retained_completion(&front).unwrap();
    assert_eq!(destination.storage_snapshot().reserved_queue_slots, 1);
    assert_eq!(destination.storage_snapshot().retained_count, 1);
    assert!(matches!(
        node_sender.try_publish(event("intruder")),
        Err(PublishError::Full(_))
    ));
    let mut changed = front.clone();
    changed.event_bytes += 1;
    assert!(matches!(
        mailbox.try_take_owned_matching(&changed),
        OwnedPoll::Empty
    ));
    assert_eq!(mailbox.storage_snapshot().queued_count, 1);
    broker.unregister_node("node", 1).unwrap();
    let completion = match mailbox.try_take_owned_matching(&front) {
        OwnedPoll::Event(e) => e,
        p => panic!("{p:?}"),
    };
    let failure = broker
        .dispatch_retained_completion(ticket, completion)
        .unwrap_err();
    assert_eq!(failure.error, DispatchError::UnknownNode("node".into()));
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(destination.storage_snapshot().retained_count, 0);
    assert_eq!(destination.storage_snapshot().reserved_queue_slots, 0);
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(0));
}

#[test]
fn retained_closed_ticket_returns_claims_without_callback_under_ledger() {
    let (broker, destination) = broker(1 << 20, 8);
    let (_, source, completion) = make_source(event("close"));
    let front = CompletionFront {
        envelope: completion.event().envelope.clone(),
        event_bytes: retained_event_bytes(completion.event()).unwrap(),
    };
    let ticket = broker.reserve_retained_completion(&front).unwrap();
    let wake = Arc::new(UnlockedWake {
        broker: broker.clone(),
        calls: AtomicUsize::new(0),
    });
    let _listener = broker
        .agent
        .capacity_listener(&Waker::from(wake.clone()))
        .unwrap();
    drop(destination);
    let failure = broker
        .dispatch_retained_completion(ticket, completion)
        .unwrap_err();
    assert_eq!(failure.error, DispatchError::Closed(Delivery::Agent));
    assert_eq!(source.storage_snapshot().retained_count, 1);
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(0));
}

#[test]
fn retained_delivery_permissions_reject_foreign_or_undersized_owners_and_cancel() {
    let (a, a_rx) = queue(1 << 20);
    let (b, b_rx) = queue(1 << 20);
    let original = event("permission");
    let pointer = original.payload.as_ptr();
    let cost = retained_event_bytes(&original).unwrap();
    let (slot, claim) = a.try_reserve_delivery(cost).unwrap();
    let failure = b
        .publish_with_queue_deferred(original, claim, slot)
        .unwrap_err();
    assert_eq!(failure.reason, ReservedPublishReason::WrongMailbox);
    assert_eq!(failure.event.payload.as_ptr(), pointer);
    assert_eq!(a_rx.storage_snapshot().reserved_queue_slots, 1);
    assert_eq!(b_rx.storage_snapshot().retained_count, 0);
    let original = failure.event;
    drop((failure.slot, failure.reservation));
    assert_eq!(a_rx.storage_snapshot().reserved_queue_slots, 0);
    assert_eq!(a_rx.storage_snapshot().retained_count, 0);
    let (slot, claim) = a.try_reserve_delivery(cost - 1).unwrap();
    let failure = a
        .publish_with_queue_deferred(original, claim, slot)
        .unwrap_err();
    assert!(matches!(
        failure.reason,
        ReservedPublishReason::TooSmall { .. }
    ));
    assert_eq!(failure.event.payload.as_ptr(), pointer);
    assert_eq!(a_rx.storage_snapshot().queued_count, 0);
    let original = failure.event;
    drop((failure.slot, failure.reservation));
    let (slot, claim) = a.try_reserve_delivery(cost).unwrap();
    a.publish_with_queue_deferred(original, claim, slot)
        .unwrap()
        .notify();
    assert_eq!(take(&a_rx).event().payload.as_ptr(), pointer);
    for foreign_slot in [false, true] {
        let (a_slot, a_claim) = a.try_reserve_delivery(cost).unwrap();
        let (b_slot, b_claim) = b.try_reserve_delivery(cost).unwrap();
        let (slot, claim, unused_slot, unused_claim) = if foreign_slot {
            (b_slot, a_claim, a_slot, b_claim)
        } else {
            (a_slot, b_claim, b_slot, a_claim)
        };
        let failure = a
            .publish_with_queue_deferred(event("permission"), claim, slot)
            .unwrap_err();
        assert_eq!(failure.reason, ReservedPublishReason::WrongMailbox);
        assert_eq!(a_rx.storage_snapshot().queued_count, 0);
        assert_eq!(b_rx.storage_snapshot().queued_count, 0);
        drop((failure, unused_slot, unused_claim));
        assert_eq!(a_rx.storage_snapshot().retained_count, 0);
        assert_eq!(b_rx.storage_snapshot().retained_count, 0);
        assert_eq!(a_rx.storage_snapshot().reserved_queue_slots, 0);
        assert_eq!(b_rx.storage_snapshot().reserved_queue_slots, 0);
    }
}

#[test]
fn missing_or_conflicting_return_context_refuses_without_queue_receipt_or_claim_effects() {
    for malformed in 0..3 {
        let (broker, destination) = broker(1 << 20, 8);
        let good = event("same-id-after-refusal");
        let mut bad = good.clone();
        match malformed {
            0 => bad.envelope.return_route = None,
            1 => bad.envelope.source = Endpoint::outer(own(), "different", 2),
            _ => bad.envelope.target = Endpoint::outer(own(), "different", 2),
        }
        let expected = bad.clone();
        let pointer = bad.payload.as_ptr();
        let raw = broker.dispatch_ingress(bad).unwrap_err();
        assert!(matches!(raw.error, DispatchError::Invalid(_)));
        assert_eq!(raw.event.payload.as_ptr(), pointer);
        let (_, source, held) = make_source(*raw.event);
        let charge = source.storage_snapshot().retained_bytes;
        let failed = broker.dispatch_retained(held).unwrap_err();
        assert!(matches!(failed.error, DispatchError::Invalid(_)));
        assert_eq!(failed.completion.event(), &expected);
        assert_eq!(failed.completion.event().payload.as_ptr(), pointer);
        assert_eq!(source.storage_snapshot().retained_bytes, charge);
        assert_eq!(destination.storage_snapshot().retained_count, 0);
        assert_eq!(destination.storage_snapshot().reserved_queue_slots, 0);
        assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(0));
        drop(failed);
        assert_eq!(source.storage_snapshot().retained_bytes, 0);
        assert_eq!(broker.dispatch_ingress(good).unwrap(), DispatchOutcome::Enqueued(Delivery::Agent));
        drop(take(&destination));
    }
}
