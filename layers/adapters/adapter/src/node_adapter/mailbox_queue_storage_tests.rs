//! Delivery slots and retained claims are separate bounds. These are actual
//! mailbox transitions, not proof of downstream admission or actor progress.
use super::*;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, EventClass};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Wake;

fn event(id: &str) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "queue-storage".into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.1", 61001)),
            target: Endpoint::node(Address::tcp("127.0.0.1", 61001), "mock", 1),
            return_route: Some(p4_protocol::event::OuterEndpoint { ingress_agent: p4_protocol::Address::tcp("127.0.0.1", 52001), channel: "outer".into(), connection_generation: 1 }),
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: None,
            payload_content_type: "application/test".into(),
        },
        payload: vec![0, 255, 128, 3],
    }
}

fn cost(event: &Event) -> usize {
    retained_event_bytes(event).unwrap()
}

fn charged(event: &Event) -> usize {
    cost(event) + COMPLETION_ENTRY_OVERHEAD_BYTES
}

fn owned(mailbox: &CompletionMailbox) -> RetainedCompletion {
    match mailbox.try_take_owned() {
        OwnedPoll::Event(completion) => completion,
        other => panic!("expected an owned completion: {other:?}"),
    }
}

struct CapacityProbe {
    mailbox: Weak<CompletionMailbox>,
    calls: AtomicUsize,
    locked: AtomicUsize,
}

impl CapacityProbe {
    fn new(mailbox: &Arc<CompletionMailbox>) -> Arc<Self> {
        Arc::new(Self {
            mailbox: Arc::downgrade(mailbox),
            calls: AtomicUsize::new(0),
            locked: AtomicUsize::new(0),
        })
    }
}

impl Wake for CapacityProbe {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let Some(mailbox) = self.mailbox.upgrade() else {
            return;
        };
        self.calls.fetch_add(1, Ordering::SeqCst);
        // Do not call a blocking mailbox method from a possibly locked callback.
        // Inspect each lock independently, so one failure cannot hide another.
        let storage_locked = mailbox.receiver.try_lock().is_err();
        let budget_locked = mailbox.budget.try_lock().is_err();
        let listener_locked = mailbox.capacity.try_lock().is_err();
        if storage_locked || budget_locked || listener_locked {
            self.locked.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[test]
fn three_retained_claims_pass_through_one_delivery_slot_without_early_retirement() {
    let first = event("one");
    let second = event("two");
    let third = event("end");
    let expected_second = second.clone();
    let second_pointer = second.payload.as_ptr();
    let total = charged(&first) + charged(&second) + charged(&third);
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 3, total).unwrap();
    let first_permit = publisher.try_reserve(1, cost(&first)).unwrap();
    let second_permit = publisher.try_reserve(1, cost(&second)).unwrap();
    let third_permit = publisher.try_reserve(1, cost(&third)).unwrap();
    let second_budget = Arc::as_ptr(&second_permit.claim.budget);
    let second_capacity = Arc::as_ptr(&second_permit.claim.capacity);
    let second_charge = second_permit.retained_bytes();
    let snapshot = mailbox.storage_snapshot();
    assert_eq!((snapshot.queue_capacity, snapshot.capacity), (1, 3));
    assert_eq!((snapshot.queued_count, snapshot.retained_count), (0, 3));
    assert_eq!(snapshot.retained_bytes, total);
    assert!(matches!(
        publisher.try_reserve(1, cost(&first)),
        Err(ReserveError::Full)
    ));

    publisher
        .publish_reserved(first.clone(), first_permit)
        .unwrap();
    let before_full = mailbox.storage_snapshot();
    let rejected = publisher
        .publish_reserved(second, second_permit)
        .unwrap_err();
    assert_eq!(rejected.reason, ReservedPublishReason::Full);
    assert_eq!(rejected.event, expected_second);
    assert_eq!(rejected.event.payload.as_ptr(), second_pointer);
    assert_eq!(
        Arc::as_ptr(&rejected.reservation.claim.budget),
        second_budget
    );
    assert_eq!(
        Arc::as_ptr(&rejected.reservation.claim.capacity),
        second_capacity
    );
    assert_eq!(rejected.reservation.retained_bytes(), second_charge);
    assert_eq!(mailbox.storage_snapshot(), before_full);

    let first_owned = owned(&mailbox);
    assert_eq!(first_owned.event(), &first);
    let after_dequeue = mailbox.storage_snapshot();
    assert_eq!(
        (after_dequeue.queued_count, after_dequeue.retained_count),
        (0, 3)
    );
    assert_eq!(after_dequeue.retained_bytes, total);
    publisher
        .publish_reserved(rejected.event, rejected.reservation)
        .unwrap();
    let second_owned = owned(&mailbox);
    assert_eq!(second_owned.event(), &expected_second);
    assert_eq!(second_owned.event().payload.as_ptr(), second_pointer);
    publisher
        .publish_reserved(third.clone(), third_permit)
        .unwrap();
    let third_owned = owned(&mailbox);
    assert_eq!(third_owned.event(), &third);
    assert!(matches!(mailbox.try_take_owned(), OwnedPoll::Empty));
    assert_eq!(mailbox.storage_snapshot(), after_dequeue);
    first_owned.retire();
    second_owned.retire();
    third_owned.retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn owned_dequeue_wakes_a_queue_waiter_outside_locks_but_equal_limits_wait_for_retirement() {
    let first = event("one");
    let second = event("two");
    let (publisher, mailbox) =
        completion_mailbox_with_limits(1, 2, charged(&first) + charged(&second)).unwrap();
    let probe = CapacityProbe::new(&mailbox);
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    let first_permit = publisher.try_reserve(1, cost(&first)).unwrap();
    let second_permit = publisher.try_reserve(1, cost(&second)).unwrap();
    publisher
        .publish_reserved(first.clone(), first_permit)
        .unwrap();
    let rejected = publisher
        .publish_reserved(second.clone(), second_permit)
        .unwrap_err();
    assert_eq!(rejected.reason, ReservedPublishReason::Full);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    let first_owned = owned(&mailbox);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
    assert_eq!(mailbox.storage_snapshot().retained_count, 2);
    publisher
        .publish_reserved(rejected.event, rejected.reservation)
        .unwrap();
    let second_owned = owned(&mailbox);
    assert_eq!(first_owned.event(), &first);
    assert_eq!(second_owned.event(), &second);
    first_owned.retire();
    second_owned.retire();
    assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);

    // The existing constructor keeps the old equal delivery/storage contract.
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&first)).unwrap();
    let probe = CapacityProbe::new(&mailbox);
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    let permit = publisher.try_reserve(1, cost(&first)).unwrap();
    publisher.publish_reserved(first, permit).unwrap();
    let completion = owned(&mailbox);
    assert_eq!(
        mailbox.storage_snapshot().queue_capacity,
        mailbox.storage_snapshot().capacity
    );
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    completion.retire();
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
}

#[test]
fn destination_queue_full_preserves_both_claims_before_original_allocation_transfer() {
    let original = event("one");
    let expected = original.clone();
    let pointer = original.payload.as_ptr();
    let blocker = event("two");
    let (source, source_mailbox) =
        completion_mailbox_with_limits(1, 1, charged(&original)).unwrap();
    let (destination, destination_mailbox) =
        completion_mailbox_with_limits(1, 2, charged(&original) + charged(&blocker)).unwrap();
    let source_permit = source.try_reserve(1, cost(&original)).unwrap();
    source.publish_reserved(original, source_permit).unwrap();
    let completion = owned(&source_mailbox);
    let destination_permit = destination.try_reserve(1, cost(&expected)).unwrap();
    let permit_budget = Arc::as_ptr(&destination_permit.claim.budget);
    let permit_charge = destination_permit.retained_bytes();
    destination.try_publish(blocker.clone()).unwrap();
    let source_before = source_mailbox.storage_snapshot();
    let destination_before = destination_mailbox.storage_snapshot();
    let rejected = completion
        .transfer_to(&destination, destination_permit)
        .unwrap_err();
    assert_eq!(rejected.reason, ReservedPublishReason::Full);
    assert_eq!(rejected.completion.event(), &expected);
    assert_eq!(rejected.completion.event().payload.as_ptr(), pointer);
    assert_eq!(
        Arc::as_ptr(&rejected.reservation.claim.budget),
        permit_budget
    );
    assert_eq!(rejected.reservation.retained_bytes(), permit_charge);
    assert_eq!(source_mailbox.storage_snapshot(), source_before);
    assert_eq!(destination_mailbox.storage_snapshot(), destination_before);

    let blocker_owned = owned(&destination_mailbox);
    assert_eq!(blocker_owned.event(), &blocker);
    assert_eq!(destination_mailbox.storage_snapshot().retained_count, 2);
    rejected
        .completion
        .transfer_to(&destination, rejected.reservation)
        .unwrap();
    assert_eq!(source_mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(source_mailbox.storage_snapshot().retained_bytes, 0);
    assert_eq!(destination_mailbox.storage_snapshot().retained_count, 2);
    let received = owned(&destination_mailbox);
    assert_eq!(received.event(), &expected);
    assert_eq!(received.event().payload.as_ptr(), pointer);
    blocker_owned.retire();
    received.retire();
    assert_eq!(destination_mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(destination_mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn ordinary_queue_full_preserves_the_event_without_temporary_claim_or_self_wake() {
    let blocker = event("one");
    let original = event("two");
    let expected = original.clone();
    let pointer = original.payload.as_ptr();
    let (publisher, mailbox) =
        completion_mailbox_with_limits(1, 3, charged(&blocker) + 2 * charged(&original)).unwrap();
    let probe = CapacityProbe::new(&mailbox);
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    publisher.try_publish(blocker.clone()).unwrap();
    let before = mailbox.storage_snapshot();
    let mut retry = original;
    for _ in 0..3 {
        retry = match publisher.try_publish(retry) {
            Err(PublishError::Full(event)) => event,
            result => panic!("delivery-full Event must be returned: {result:?}"),
        };
        assert_eq!(retry, expected);
        assert_eq!(retry.payload.as_ptr(), pointer);
        assert_eq!(mailbox.storage_snapshot(), before);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    }
    let blocker_owned = owned(&mailbox);
    assert_eq!(blocker_owned.event(), &blocker);
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    publisher.try_publish(retry).unwrap();
    let received = owned(&mailbox);
    assert_eq!(received.event(), &expected);
    assert_eq!(received.event().payload.as_ptr(), pointer);
    assert_eq!(mailbox.storage_snapshot().retained_count, 2);
    blocker_owned.retire();
    received.retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
    assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
}

#[test]
fn an_impossible_event_is_too_large_even_while_the_delivery_queue_is_full() {
    let blocker = event("one");
    let mut original = event("two");
    original.payload = vec![7; 64];
    let expected = original.clone();
    let pointer = original.payload.as_ptr();
    let required = charged(&original);
    let limit = charged(&blocker);
    assert!(required > limit);
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 3, limit).unwrap();
    let probe = CapacityProbe::new(&mailbox);
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    publisher.try_publish(blocker.clone()).unwrap();
    let before = mailbox.storage_snapshot();
    assert_eq!(before.queued_count, before.queue_capacity);

    let rejected = publisher.try_publish(original);
    let Err(PublishError::TooLarge {
        event,
        required: reported_required,
        limit: reported_limit,
    }) = rejected
    else {
        panic!("permanent byte rejection must take precedence over Full: {rejected:?}");
    };
    assert_eq!(event, expected);
    assert_eq!(event.payload.as_ptr(), pointer);
    assert_eq!(reported_required, required);
    assert_eq!(reported_limit, limit);
    assert_eq!(mailbox.storage_snapshot(), before);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
    let retained_blocker = owned(&mailbox);
    assert_eq!(retained_blocker.event(), &blocker);
    retained_blocker.retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn neither_delivery_nor_retained_capacity_may_be_zero() {
    let bytes = charged(&event("one"));
    for (queue_capacity, retained_capacity) in [(0, 0), (0, 1), (1, 0)] {
        assert!(matches!(
            completion_mailbox_with_limits(queue_capacity, retained_capacity, bytes),
            Err(MailboxBuildError::InvalidCapacity)
        ));
    }
    let (_, mailbox) = completion_mailbox_with_limits(1, 1, bytes).unwrap();
    let snapshot = mailbox.storage_snapshot();
    assert_eq!((snapshot.queue_capacity, snapshot.capacity), (1, 1));
    assert_eq!((snapshot.queued_count, snapshot.retained_count), (0, 0));
    assert_eq!(snapshot.retained_bytes, 0);
}
