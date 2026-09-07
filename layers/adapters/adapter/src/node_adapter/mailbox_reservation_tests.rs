//! Actual storage transitions; no wire/source authority or pipeline claim.
use super::*;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, EventClass};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Wake;

const HANDSHAKE_LIMIT: std::time::Duration = std::time::Duration::from_secs(5);

fn finish<T>(handle: std::thread::JoinHandle<T>) -> T {
    let deadline = std::time::Instant::now() + HANDSHAKE_LIMIT;
    while !handle.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "reservation participant must terminate"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    handle.join().unwrap()
}

fn event(id: &str) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "reservation".into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.1", 61001)),
            target: Endpoint::node(Address::tcp("127.0.0.1", 61001), "mock", 1),
            return_route: None,
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: None,
            payload_content_type: "test".into(),
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
        OwnedPoll::Event(value) => value,
        other => panic!("expected owned completion: {other:?}"),
    }
}

#[derive(Default)]
struct CountWake(AtomicUsize);
impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn construction_and_reservation_overflow_fail_without_large_allocations() {
    assert!(matches!(
        completion_mailbox_with_budget(0, 0),
        Err(MailboxBuildError::InvalidCapacity)
    ));
    assert!(matches!(
        completion_mailbox_with_budget(usize::MAX, usize::MAX),
        Err(MailboxBuildError::StorageOverflow)
    ));
    let (publisher, mailbox) = completion_mailbox_with_budget(1, usize::MAX).unwrap();
    assert!(mailbox.storage_snapshot().queue_backing_bytes >= std::mem::size_of::<Entry>());
    assert!(matches!(
        publisher.try_reserve(0, 0),
        Err(ReserveError::InvalidCount)
    ));
    assert!(matches!(
        publisher.try_reserve(2, 0),
        Err(ReserveError::InvalidCount)
    ));
    assert!(matches!(
        publisher.try_reserve(1, usize::MAX),
        Err(ReserveError::CostOverflow)
    ));
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn reserved_space_is_unavailable_to_ordinary_publication_and_cancel_restores_it() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    assert_eq!(mailbox.storage_snapshot().queued_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(
        publisher.try_publish(original.clone()),
        Err(PublishError::Full(original.clone()))
    );
    drop(permit);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    publisher.try_publish(original.clone()).unwrap();
    assert_eq!(mailbox.try_take(), Poll::Event(original));
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn byte_capacity_and_event_capacity_are_independent_exact_bounds() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(2, charged(&original)).unwrap();
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    assert!(
        matches!(publisher.try_reserve(1, 0), Err(ReserveError::Full)),
        "a free count slot cannot spend used bytes"
    );
    publisher
        .publish_reserved(original.clone(), permit)
        .unwrap();
    assert_eq!(
        mailbox.storage_snapshot().retained_bytes,
        charged(&original)
    );
    let completion = owned(&mailbox);
    assert_eq!(completion.event(), &original);
    completion.retire();
    let too_small = completion_mailbox_with_budget(2, charged(&original) - 1).unwrap();
    assert_eq!(
        too_small.0.try_reserve(1, cost(&original)).unwrap_err(),
        ReserveError::TooLarge {
            required: charged(&original),
            limit: charged(&original) - 1,
        }
    );
    assert_eq!(
        too_small.0.try_publish(original.clone()),
        Err(PublishError::TooLarge {
            required: charged(&original),
            limit: charged(&original) - 1,
            event: original,
        })
    );
}

#[test]
fn a_reserved_publish_does_not_recheck_ordinary_full_and_legacy_cannot_strip_it() {
    let first = event("one");
    let next = event("two");
    let (publisher, mailbox) =
        completion_mailbox_with_budget(2, charged(&first) + charged(&next)).unwrap();
    let permit = publisher.try_reserve(1, cost(&next)).unwrap();
    publisher.try_publish(first.clone()).unwrap();
    assert_eq!(
        publisher.try_publish(next.clone()),
        Err(PublishError::Full(next.clone()))
    );
    publisher.publish_reserved(next.clone(), permit).unwrap();
    assert_eq!(mailbox.try_take(), Poll::Event(first));
    assert_eq!(mailbox.try_take(), Poll::Empty);
    let waker = Waker::from(Arc::new(CountWake::default()));
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    assert_eq!(mailbox.storage_snapshot().queued_count, 1);
    let completion = owned(&mailbox);
    assert_eq!(completion.event(), &next);
    assert_eq!(mailbox.storage_snapshot().queued_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    completion.retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
}

#[test]
fn owned_dequeue_keeps_capacity_until_event_retirement() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let count = Arc::new(CountWake::default());
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&count)))
        .unwrap();
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    publisher
        .publish_reserved(original.clone(), permit)
        .unwrap();
    let completion = owned(&mailbox);
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    assert_eq!(completion.retained_bytes(), charged(&original));
    assert!(matches!(
        publisher.try_reserve(1, cost(&original)),
        Err(ReserveError::Full)
    ));
    completion.retire();
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
    drop(publisher.try_reserve(1, cost(&original)).unwrap());
}

struct ClaimWake {
    mailbox: Weak<CompletionMailbox>,
    calls: AtomicUsize,
    locked: AtomicUsize,
}

impl Wake for ClaimWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let Some(mailbox) = self.mailbox.upgrade() else {
            return;
        };
        self.calls.fetch_add(1, Ordering::SeqCst);
        if mailbox.receiver.try_lock().is_err()
            || mailbox.budget.try_lock().is_err()
            || mailbox.capacity.try_lock().is_err()
        {
            self.locked.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[test]
fn reservation_cancellation_and_owned_retirement_wake_outside_every_storage_lock() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let probe = Arc::new(ClaimWake {
        mailbox: Arc::downgrade(&mailbox),
        calls: AtomicUsize::new(0),
        locked: AtomicUsize::new(0),
    });
    let _listener = publisher
        .capacity_listener(&Waker::from(Arc::clone(&probe)))
        .unwrap();
    drop(publisher.try_reserve(1, cost(&original)).unwrap());
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    publisher.publish_reserved(original, permit).unwrap();
    let completion = owned(&mailbox);
    assert_eq!(
        probe.calls.load(Ordering::SeqCst),
        1,
        "dequeue does not return capacity"
    );
    completion.retire();
    assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
    assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
}

#[test]
fn wrong_mailbox_and_too_small_preserve_the_exact_event_and_permission() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let (other, other_mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    let pointer = original.payload.as_ptr();
    let failure = other.publish_reserved(original, permit).unwrap_err();
    assert_eq!(failure.reason, ReservedPublishReason::WrongMailbox);
    assert_eq!(failure.event.payload.as_ptr(), pointer);
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(other_mailbox.storage_snapshot().retained_count, 0);
    publisher
        .publish_reserved(failure.event, failure.reservation)
        .unwrap();
    owned(&mailbox).retire();
    let original = event("one");
    let permit = publisher.try_reserve(1, cost(&original) - 1).unwrap();
    let failure = publisher
        .publish_reserved(original.clone(), permit)
        .unwrap_err();
    assert_eq!(failure.event, original);
    assert_eq!(
        failure.reason,
        ReservedPublishReason::TooSmall {
            required: charged(&original),
            reserved: charged(&original) - 1,
        }
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    drop(failure);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
}

#[test]
fn receiver_close_returns_original_event_and_permission_until_explicit_cancel() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    drop(mailbox);
    assert!(matches!(
        publisher.try_reserve(1, cost(&original)),
        Err(ReserveError::Closed)
    ));
    let failure = publisher
        .publish_reserved(original.clone(), permit)
        .unwrap_err();
    assert_eq!(failure.reason, ReservedPublishReason::Closed);
    assert_eq!(failure.event, original);
    assert_eq!(publisher.budget.lock().unwrap().used_count, 1);
    drop(failure);
    assert_eq!(publisher.budget.lock().unwrap().used_count, 0);
}

#[test]
fn transfer_failure_retains_both_claims_and_success_moves_the_original_allocation() {
    let original = event("one");
    let expected = original.clone();
    let pointer = original.payload.as_ptr();
    let (source, source_mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let (destination, destination_mailbox) =
        completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let (foreign, foreign_mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let permit = source.try_reserve(1, cost(&original)).unwrap();
    source.publish_reserved(original, permit).unwrap();
    let completion = owned(&source_mailbox);
    let wrong = foreign.try_reserve(1, cost(completion.event())).unwrap();
    let failure = completion.transfer_to(&destination, wrong).unwrap_err();
    assert_eq!(failure.reason, ReservedPublishReason::WrongMailbox);
    assert_eq!(failure.completion.event(), &expected);
    assert_eq!(source_mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(foreign_mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(destination_mailbox.storage_snapshot().retained_count, 0);
    drop(failure.reservation);
    let permit = destination
        .try_reserve(1, cost(failure.completion.event()))
        .unwrap();
    failure
        .completion
        .transfer_to(&destination, permit)
        .unwrap();
    assert_eq!(source_mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(foreign_mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(destination_mailbox.storage_snapshot().retained_count, 1);
    let completion = owned(&destination_mailbox);
    assert_eq!(completion.event(), &expected);
    assert_eq!(completion.event().payload.as_ptr(), pointer);
    completion.retire();
    assert_eq!(destination_mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn a_closed_transfer_keeps_the_old_storage_claim() {
    let original = event("one");
    let (source, source_mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let (destination, destination_mailbox) =
        completion_mailbox_with_budget(1, charged(&original)).unwrap();
    source.try_publish(original.clone()).unwrap();
    let completion = owned(&source_mailbox);
    let permit = destination.try_reserve(1, cost(&original)).unwrap();
    drop(destination_mailbox);
    let failure = completion.transfer_to(&destination, permit).unwrap_err();
    assert_eq!(failure.reason, ReservedPublishReason::Closed);
    assert_eq!(failure.completion.event(), &original);
    assert_eq!(source_mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(destination.budget.lock().unwrap().used_count, 1);
    drop(failure);
    assert_eq!(source_mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(destination.budget.lock().unwrap().used_count, 0);
}

#[test]
fn reserved_input_is_drained_owned_before_last_publisher_closure() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    publisher
        .publish_reserved(original.clone(), permit)
        .unwrap();
    drop(publisher);
    assert_eq!(mailbox.try_take(), Poll::Empty);
    let completion = owned(&mailbox);
    assert_eq!(completion.event(), &original);
    assert!(matches!(mailbox.try_take_owned(), OwnedPoll::Closed));
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    completion.retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
}

#[test]
fn owned_reader_rechecks_reserved_publish_between_empty_and_registration() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let permit = publisher.try_reserve(1, cost(&original)).unwrap();
    let waker = Waker::from(Arc::new(CountWake::default()));
    let result = mailbox.poll_take_owned_before_register(&mut Context::from_waker(&waker), || {
        publisher
            .publish_reserved(original.clone(), permit)
            .unwrap();
    });
    let TaskPoll::Ready(OwnedPoll::Event(completion)) = result else {
        panic!("lost owned arrival: {result:?}")
    };
    assert_eq!(completion.event(), &original);
    completion.retire();
    assert!(mailbox.waker.lock().unwrap().is_none());
}

#[test]
fn owned_reader_rechecks_last_publisher_close_between_empty_and_registration() {
    let (publisher, mailbox) = completion_mailbox_with_budget(1, 1024).unwrap();
    let waker = Waker::from(Arc::new(CountWake::default()));
    let result = mailbox
        .poll_take_owned_before_register(&mut Context::from_waker(&waker), || drop(publisher));
    assert!(matches!(result, TaskPoll::Ready(OwnedPoll::Closed)));
    assert!(mailbox.waker.lock().unwrap().is_none());
}

#[test]
fn two_simultaneous_reservations_cannot_both_claim_the_same_actual_slot() {
    let original = event("one");
    let (publisher, mailbox) = completion_mailbox_with_budget(1, charged(&original)).unwrap();
    let bytes = cost(&original);
    let (results_sender, results_receiver) = mpsc::channel();
    let mut starts = Vec::new();
    let threads = (0..2)
        .map(|_| {
            let publisher = publisher.clone();
            let (start_sender, start_receiver) = mpsc::channel();
            starts.push(start_sender);
            let results = results_sender.clone();
            std::thread::spawn(move || {
                start_receiver.recv_timeout(HANDSHAKE_LIMIT).unwrap();
                results.send(publisher.try_reserve(1, bytes)).unwrap();
            })
        })
        .collect::<Vec<_>>();
    for start in starts {
        start.send(()).unwrap();
    }
    let results = (0..2)
        .map(|_| {
            results_receiver
                .recv_timeout(HANDSHAKE_LIMIT)
                .expect("reservation must finish without waiting for consumer capacity")
        })
        .collect::<Vec<_>>();
    for thread in threads {
        finish(thread);
    }
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ReserveError::Full)))
            .count(),
        1
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    assert_eq!(
        mailbox.storage_snapshot().retained_bytes,
        charged(&original)
    );
    drop(results);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
}
