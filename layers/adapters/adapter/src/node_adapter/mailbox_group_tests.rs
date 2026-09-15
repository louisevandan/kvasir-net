//! Known fan-out against the actual bounded store. No engine, transport grant,
//! actor liveness or native bound is synthesized by these tests.
use super::*;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, EventClass};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Wake;

fn event(id: &str) -> Event {
    let mut payload = Vec::with_capacity(2048);
    payload.extend_from_slice(&[0, 255, 128, 3]);
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "fan-out".into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.1", 61101)),
            target: Endpoint::node(Address::tcp("127.0.0.1", 61101), "mock", 1),
            return_route: Some(p4_protocol::event::OuterEndpoint {
                ingress_agent: p4_protocol::Address::tcp("127.0.0.1", 52001),
                channel: "outer".into(),
                connection_generation: 1,
            }),
            class: EventClass::Data,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: None,
            payload_content_type: "opaque".into(),
        },
        payload,
    }
}
fn cost(event: &Event) -> usize {
    retained_event_bytes(event).unwrap()
}
fn owned(mailbox: &CompletionMailbox) -> RetainedCompletion {
    let OwnedPoll::Event(value) = mailbox.try_take_owned() else {
        panic!("missing owned Event")
    };
    value
}

#[test]
fn a_three_result_group_reserves_atomically_and_drains_through_one_delivery_slot() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 3, usize::MAX).unwrap();
    let first = event("first");
    let second = event("second");
    let third = event("third");
    let first_pointer = first.payload.as_ptr();
    let second_pointer = second.payload.as_ptr();
    let third_pointer = third.payload.as_ptr();
    let bounds = [cost(&first), cost(&second), cost(&third)];
    let item_total = bounds.iter().sum::<usize>() + 3 * COMPLETION_ENTRY_OVERHEAD_BYTES;
    let mut group = publisher.try_reserve_group(&bounds).unwrap();
    let backing = group.items.capacity() * std::mem::size_of::<CompletionReservation>();
    assert_eq!(group.backing_bytes(), backing);
    assert_eq!(
        mailbox.storage_snapshot().retained_bytes,
        item_total + backing
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 3);
    assert_eq!(mailbox.storage_snapshot().queue_capacity, 1);
    assert_eq!(mailbox.storage_snapshot().queued_count, 0);
    publisher
        .publish_reserved(first, group.take_next().unwrap())
        .unwrap();
    let before = mailbox.storage_snapshot();
    let expected = second.clone();
    let failure = publisher
        .publish_reserved(second, group.take_next().unwrap())
        .unwrap_err();
    assert_eq!(failure.reason, ReservedPublishReason::Full);
    assert_eq!(failure.event, expected);
    assert_eq!(failure.event.payload.as_ptr(), second_pointer);
    assert_eq!(mailbox.storage_snapshot(), before);
    let first = owned(&mailbox);
    assert_eq!(first.event().payload.as_ptr(), first_pointer);
    assert_eq!(mailbox.storage_snapshot().retained_count, 3);
    publisher
        .publish_reserved(failure.event, failure.reservation)
        .unwrap();
    first.retire();
    let second = owned(&mailbox);
    publisher
        .publish_reserved(third, group.take_next().unwrap())
        .unwrap();
    assert!(group.is_empty());
    assert_eq!(group.take_next().map(|p| p.retained_bytes()), None);
    assert_eq!(second.event().payload.as_ptr(), second_pointer);
    second.retire();
    let third = owned(&mailbox);
    assert_eq!(third.event().payload.as_ptr(), third_pointer);
    third.retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(
        mailbox.storage_snapshot().retained_bytes,
        backing,
        "the empty group's real array still exists"
    );
    drop(group);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
    assert_eq!(mailbox.storage_snapshot().queued_count, 0);
}

#[test]
fn invalid_or_impossible_group_has_no_partial_claims_or_allocation_side_effects() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 2, usize::MAX).unwrap();
    let bytes = cost(&event("valid"));
    let before = mailbox.storage_snapshot();
    assert_eq!(
        publisher.try_reserve_group(&[]).unwrap_err(),
        GroupReserveError::Empty
    );
    assert!(matches!(
        publisher.try_reserve_group(&[bytes, 0]),
        Err(GroupReserveError::InvalidFootprint { index: 1, .. })
    ));
    assert_eq!(
        publisher
            .try_reserve_group(&[bytes, usize::MAX])
            .unwrap_err(),
        GroupReserveError::CostOverflow
    );
    assert!(matches!(
        publisher.try_reserve_group(&[bytes; 3]),
        Err(GroupReserveError::TooLarge {
            required_count: 3,
            count_limit: 2,
            ..
        })
    ));
    assert_eq!(mailbox.storage_snapshot(), before);
    let (limited, limited_mailbox) = completion_mailbox_with_limits(1, 2, bytes).unwrap();
    assert!(matches!(limited.try_reserve_group(&[bytes]),
        Err(GroupReserveError::TooLarge { byte_limit: Some(limit), .. }) if limit == bytes));
    assert_eq!(limited_mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(limited_mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn count_only_compatibility_cannot_accumulate_uncharged_empty_group_backings() {
    let (publisher, mailbox) = completion_mailbox(2);
    assert_eq!(
        publisher
            .try_reserve_group(&[cost(&event("one"))])
            .unwrap_err(),
        GroupReserveError::MissingByteLimit
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn temporary_group_shortage_is_full_and_the_identical_group_succeeds_after_retirement() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 2, usize::MAX).unwrap();
    let bounds = [cost(&event("one")); 2];
    let existing = publisher.try_reserve(1, bounds[0]).unwrap();
    let before = mailbox.storage_snapshot();
    assert_eq!(
        publisher.try_reserve_group(&bounds).unwrap_err(),
        GroupReserveError::Full
    );
    assert_eq!(mailbox.storage_snapshot(), before);
    drop(existing);
    let group = publisher.try_reserve_group(&bounds).unwrap();
    assert_eq!(group.len(), 2);
    assert_eq!(mailbox.storage_snapshot().retained_count, 2);
    drop(group);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn receiver_close_during_group_preparation_is_rechecked_before_any_claim_commit() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 2, usize::MAX).unwrap();
    let result = publisher.reserve_group_before_commit(&[cost(&event("one")); 2], || drop(mailbox));
    assert_eq!(result.unwrap_err(), GroupReserveError::Closed);
    let budget = publisher.budget.lock().unwrap();
    assert_eq!(budget.used_count, 0);
    assert_eq!(budget.used_bytes, 0);
}

#[test]
fn competing_admission_during_group_preparation_cannot_overcommit_the_final_store() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 2, usize::MAX).unwrap();
    let competitor = event("competitor");
    let expected = competitor.clone();
    let bounds = [cost(&competitor); 2];
    let mut competitor_snapshot = None;
    let result = publisher.reserve_group_before_commit(&bounds, || {
        publisher.try_publish(competitor).unwrap();
        competitor_snapshot = Some(mailbox.storage_snapshot());
    });
    assert_eq!(result.unwrap_err(), GroupReserveError::Full);
    assert_eq!(mailbox.storage_snapshot(), competitor_snapshot.unwrap());
    let retained = owned(&mailbox);
    assert_eq!(retained.event(), &expected);
    retained.retire();
    let group = publisher.try_reserve_group(&bounds).unwrap();
    assert_eq!(group.len(), 2);
    drop(group);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

struct LockProbe {
    mailbox: Weak<CompletionMailbox>,
    calls: AtomicUsize,
    locked: AtomicUsize,
}
impl Wake for LockProbe {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(mailbox) = self.mailbox.upgrade() {
            if mailbox.receiver.try_lock().is_err() || mailbox.budget.try_lock().is_err() {
                self.locked.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

#[test]
fn cancelling_unused_group_claims_and_backing_never_calls_wakers_under_storage_locks() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 2, usize::MAX).unwrap();
    let probe = Arc::new(LockProbe {
        mailbox: Arc::downgrade(&mailbox),
        calls: AtomicUsize::new(0),
        locked: AtomicUsize::new(0),
    });
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    let group = publisher
        .try_reserve_group(&[cost(&event("one")); 2])
        .unwrap();
    assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
    drop(group);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

struct PanicProbe(AtomicUsize);
impl Wake for PanicProbe {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        // Subsequent calls are observable rather than another panic: removing
        // cleanup suppression must fail the oracle, not abort the test process.
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("caller capacity callback failed");
        }
    }
}

#[test]
fn group_cleanup_retires_all_storage_before_a_single_panicking_notification() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 3, usize::MAX).unwrap();
    let mut group = publisher
        .try_reserve_group(&[cost(&event("one")); 3])
        .unwrap();
    // One independently owned claim must not be cancelled with its old group.
    let independent = group.take_next().unwrap();
    let remaining_bytes = independent.retained_bytes();
    let probe = Arc::new(PanicProbe(AtomicUsize::new(0)));
    let registration = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(group)));
    assert!(result.is_err(), "caller panic is not silently swallowed");
    assert_eq!(
        probe.0.load(Ordering::SeqCst),
        1,
        "no callback from an unwinding field Drop"
    );
    drop(registration);
    assert!(mailbox.receiver.lock().is_ok());
    assert!(mailbox.budget.lock().is_ok());
    assert!(mailbox.capacity.lock().is_ok());
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, remaining_bytes);
    drop(independent);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn owned_dequeue_callback_panic_returns_its_claim_without_a_second_unwind_callback() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 2, usize::MAX).unwrap();
    let first = event("one");
    let second = event("two");
    let first_permit = publisher.try_reserve(1, cost(&first)).unwrap();
    let second_permit = publisher.try_reserve(1, cost(&second)).unwrap();
    publisher.publish_reserved(first, first_permit).unwrap();
    let probe = Arc::new(PanicProbe(AtomicUsize::new(0)));
    let registration = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mailbox.try_take_owned()));
    assert!(result.is_err());
    assert_eq!(probe.0.load(Ordering::SeqCst), 1);
    drop(registration);
    // A panicking dequeue did not deliver its Event; only the second reservation
    // is still live. This does not turn callback failure into reliable delivery.
    assert_eq!(mailbox.storage_snapshot().queued_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(
        mailbox.storage_snapshot().retained_bytes,
        second_permit.retained_bytes()
    );
    publisher.publish_reserved(second, second_permit).unwrap();
    owned(&mailbox).retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn concurrent_groups_cannot_both_consume_the_same_storage_count() {
    let (publisher, mailbox) = completion_mailbox_with_limits(1, 2, usize::MAX).unwrap();
    let bytes = cost(&event("one"));
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let (sender, receiver) = std::sync::mpsc::channel();
    let handles = (0..2)
        .map(|_| {
            let publisher = publisher.clone();
            let barrier = Arc::clone(&barrier);
            let sender = sender.clone();
            std::thread::spawn(move || {
                barrier.wait();
                sender
                    .send(publisher.try_reserve_group(&[bytes; 2]))
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = (0..2)
        .map(|_| {
            receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(GroupReserveError::Full)))
            .count(),
        1
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 2);
    drop(results);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn aggregate_pool_is_charged_once_and_split_without_changing_total_usage() {
    let first = event("first");
    let mut second = event("second");
    second.payload.reserve_exact(16 * 1024);
    let first_cost = cost(&first);
    let second_cost = cost(&second);
    let count = 2;
    let backing = count * std::mem::size_of::<CompletionReservation>();
    let total = first_cost + second_cost + count * COMPLETION_ENTRY_OVERHEAD_BYTES + backing + 4096;
    let (publisher, mailbox) = completion_mailbox_with_limits(1, count, total).unwrap();
    let mut group = publisher.try_reserve_pool(count, total).unwrap();
    assert_eq!(mailbox.storage_snapshot().retained_count, count);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, total);
    let before = mailbox.storage_snapshot();
    let first_reservation = group.take_for(first_cost).unwrap();
    let second_reservation = group.take_for(second_cost).unwrap();
    assert_eq!(mailbox.storage_snapshot(), before);
    assert!(group.is_empty());
    drop(group);
    assert_eq!(
        mailbox.storage_snapshot().retained_bytes,
        first_reservation.retained_bytes() + second_reservation.retained_bytes()
    );
    publisher
        .publish_reserved(first, first_reservation)
        .unwrap();
    let first = owned(&mailbox);
    publisher
        .publish_reserved(second, second_reservation)
        .unwrap();
    first.retire();
    owned(&mailbox).retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn aggregate_pool_refusal_and_oversized_item_preserve_the_whole_unassigned_pool() {
    let count = 2;
    let minimum = count * (std::mem::size_of::<Event>() + COMPLETION_ENTRY_OVERHEAD_BYTES)
        + count * std::mem::size_of::<CompletionReservation>();
    let (publisher, mailbox) = completion_mailbox_with_limits(1, count, minimum + 1024).unwrap();
    assert_eq!(
        publisher.try_reserve_pool(count, minimum - 1).unwrap_err(),
        GroupReserveError::PoolTooSmall {
            required_bytes: minimum,
            provided_bytes: minimum - 1,
        }
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    let mut group = publisher.try_reserve_pool(count, minimum + 1024).unwrap();
    let before = mailbox.storage_snapshot();
    assert!(matches!(
        group.take_for(std::mem::size_of::<Event>() + 1025),
        Err(GroupReserveError::PoolItemTooLarge { .. })
    ));
    assert_eq!(group.len(), count);
    assert_eq!(mailbox.storage_snapshot(), before);
    drop(group);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}
