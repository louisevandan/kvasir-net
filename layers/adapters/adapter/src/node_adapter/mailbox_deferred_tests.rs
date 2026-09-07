//! Deferred callbacks over the actual store. These are not broker, remote
//! delivery, causal-return reservation or native/KV settlement tests.
use super::*;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, EventClass};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Wake;

fn event(id: &str) -> Event {
    let mut payload = Vec::with_capacity(64);
    payload.extend_from_slice(&[0, 255, 128, 3]);
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "deferred-storage".into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.1", 62001)),
            target: Endpoint::node(Address::tcp("127.0.0.1", 62001), "mock", 1),
            return_route: None,
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: None,
            payload_content_type: "test".into(),
        },
        payload,
    }
}

fn cost(event: &Event) -> usize {
    retained_event_bytes(event).unwrap()
}

fn owned(mailbox: &CompletionMailbox) -> RetainedCompletion {
    match mailbox.try_take_owned() {
        OwnedPoll::Event(value) => value,
        other => panic!("expected owned Event: {other:?}"),
    }
}

fn register_reader(mailbox: &CompletionMailbox, waker: &Waker) {
    assert!(matches!(
        mailbox.poll_take_owned(&mut Context::from_waker(waker)),
        TaskPoll::Pending
    ));
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

struct ReentrantReader {
    mailbox: Weak<CompletionMailbox>,
    caller: Weak<Mutex<()>>,
    calls: AtomicUsize,
    locked: AtomicUsize,
    observed: Mutex<Vec<(String, usize)>>,
}

impl Wake for ReentrantReader {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mailbox = self.mailbox.upgrade().unwrap();
        let caller = self.caller.upgrade().unwrap();
        if caller.try_lock().is_err()
            || mailbox.receiver.try_lock().is_err()
            || mailbox.budget.try_lock().is_err()
            || mailbox.waker.try_lock().is_err()
            || mailbox.capacity.try_lock().is_err()
        {
            self.locked.fetch_add(1, Ordering::SeqCst);
            return;
        }
        let value = owned(&mailbox);
        self.observed.lock().unwrap().push((
            value.event().envelope.event_id.clone(),
            value.event().payload.as_ptr() as usize,
        ));
        value.retire();
    }
}

#[test]
fn ordinary_and_reserved_enqueue_defer_one_reentrant_reader_callback() {
    for reserved in [false, true] {
        let original = event("original-allocation");
        let pointer = original.payload.as_ptr() as usize;
        let (publisher, mailbox) = completion_mailbox_with_budget(1, 8192).unwrap();
        let caller = Arc::new(Mutex::new(()));
        let probe = Arc::new(ReentrantReader {
            mailbox: Arc::downgrade(&mailbox),
            caller: Arc::downgrade(&caller),
            calls: AtomicUsize::new(0),
            locked: AtomicUsize::new(0),
            observed: Mutex::new(Vec::new()),
        });
        let waker = Waker::from(Arc::clone(&probe));
        register_reader(&mailbox, &waker);
        let caller_guard = caller.lock().unwrap();
        let notification = if reserved {
            let reservation = publisher.try_reserve(1, cost(&original)).unwrap();
            publisher
                .publish_reserved_deferred(original, reservation)
                .unwrap()
        } else {
            publisher.try_publish_deferred(original).unwrap()
        };
        assert_eq!(mailbox.storage_snapshot().queued_count, 1);
        assert_eq!(probe.calls.load(Ordering::SeqCst), 0);
        drop(caller_guard);
        notification.notify();
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
        assert_eq!(probe.locked.load(Ordering::SeqCst), 0);
        assert_eq!(
            *probe.observed.lock().unwrap(),
            vec![("original-allocation".to_owned(), pointer)]
        );
        assert_eq!(mailbox.storage_snapshot().retained_count, 0);
        assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
    }
}

#[test]
fn successful_deferred_transfer_keeps_source_until_explicit_notification() {
    let original = event("transfer");
    let expected = original.clone();
    let pointer = original.payload.as_ptr();
    let (source, source_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    let (destination, destination_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    source.try_publish(original).unwrap();
    let held = owned(&source_box);
    let source_wake = Arc::new(CountWake::default());
    let _source_listener = source
        .capacity_listener(&Waker::from(Arc::clone(&source_wake)))
        .unwrap();
    let reader_wake = Arc::new(CountWake::default());
    register_reader(&destination_box, &Waker::from(Arc::clone(&reader_wake)));
    let reservation = destination.try_reserve(1, cost(held.event())).unwrap();
    let notification = held
        .transfer_to_deferred(&destination, reservation)
        .unwrap();
    assert_eq!(source_box.storage_snapshot().retained_count, 1);
    assert_eq!(destination_box.storage_snapshot().retained_count, 1);
    assert_eq!(reader_wake.0.load(Ordering::SeqCst), 0);
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 0);

    notification.notify();
    assert_eq!(source_box.storage_snapshot().retained_count, 0);
    assert_eq!(source_box.storage_snapshot().retained_bytes, 0);
    assert_eq!(reader_wake.0.load(Ordering::SeqCst), 1);
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 1);
    let received = owned(&destination_box);
    assert_eq!(received.event(), &expected);
    assert_eq!(received.event().payload.as_ptr(), pointer);
    received.retire();
    assert_eq!(destination_box.storage_snapshot().retained_count, 0);
}

#[test]
fn a_failed_deferred_transfer_preserves_both_claims_and_original_for_retry() {
    let original = event("retry");
    let expected = original.clone();
    let pointer = original.payload.as_ptr();
    let original_capacity = original.payload.capacity();
    let (source, source_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    let (destination, destination_box) = completion_mailbox_with_limits(1, 2, 16384).unwrap();
    source.try_publish(original).unwrap();
    let held = owned(&source_box);
    destination.try_publish(event("occupant")).unwrap();
    let reservation = destination.try_reserve(1, cost(held.event())).unwrap();
    let reservation_bytes = reservation.retained_bytes();
    let before_source = source_box.storage_snapshot();
    let before_destination = destination_box.storage_snapshot();
    let source_wake = Arc::new(CountWake::default());
    let destination_wake = Arc::new(CountWake::default());
    let _source_listener = source
        .capacity_listener(&Waker::from(Arc::clone(&source_wake)))
        .unwrap();
    let _destination_listener = destination
        .capacity_listener(&Waker::from(Arc::clone(&destination_wake)))
        .unwrap();

    let refusal = held
        .transfer_to_deferred(&destination, reservation)
        .unwrap_err();
    assert_eq!(refusal.reason, ReservedPublishReason::Full);
    assert_eq!(refusal.completion.event(), &expected);
    assert_eq!(refusal.completion.event().payload.as_ptr(), pointer);
    assert_eq!(
        refusal.completion.event().payload.capacity(),
        original_capacity
    );
    assert_eq!(refusal.reservation.retained_bytes(), reservation_bytes);
    assert_eq!(source_box.storage_snapshot(), before_source);
    assert_eq!(destination_box.storage_snapshot(), before_destination);
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 0);
    assert_eq!(destination_wake.0.load(Ordering::SeqCst), 0);

    owned(&destination_box).retire();
    let notification = refusal
        .completion
        .transfer_to_deferred(&destination, refusal.reservation)
        .unwrap();
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 0);
    notification.notify();
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 1);
    let received = owned(&destination_box);
    assert_eq!(received.event().payload.as_ptr(), pointer);
    received.retire();
}

#[test]
fn reserved_deferred_permanent_refusals_do_not_notify_or_replace_ownership() {
    for case in ["wrong", "small", "closed"] {
        let original = event(case);
        let pointer = original.payload.as_ptr();
        let expected = original.clone();
        let (publisher, mailbox) = completion_mailbox_with_budget(1, 8192).unwrap();
        let (other, other_box) = completion_mailbox_with_budget(1, 8192).unwrap();
        let reservation = match case {
            "wrong" => other.try_reserve(1, cost(&original)).unwrap(),
            "small" => publisher.try_reserve(1, 0).unwrap(),
            _ => publisher.try_reserve(1, cost(&original)).unwrap(),
        };
        let reservation_bytes = reservation.retained_bytes();
        let claim_budget = Arc::clone(&reservation.claim.budget);
        let wake = Arc::new(CountWake::default());
        let _listener = publisher
            .capacity_listener(&Waker::from(Arc::clone(&wake)))
            .unwrap();
        let mailbox = if case == "closed" {
            drop(mailbox);
            None
        } else {
            Some(mailbox)
        };
        let before_wakes = wake.0.load(Ordering::SeqCst);
        let error = publisher
            .publish_reserved_deferred(original, reservation)
            .unwrap_err();
        match case {
            "wrong" => assert_eq!(error.reason, ReservedPublishReason::WrongMailbox),
            "small" => assert!(matches!(
                error.reason,
                ReservedPublishReason::TooSmall { .. }
            )),
            _ => assert_eq!(error.reason, ReservedPublishReason::Closed),
        }
        assert_eq!(error.event, expected);
        assert_eq!(error.event.payload.as_ptr(), pointer);
        assert_eq!(error.reservation.retained_bytes(), reservation_bytes);
        assert!(Arc::ptr_eq(&error.reservation.claim.budget, &claim_budget));
        assert_eq!(claim_budget.lock().unwrap().used_count, 1);
        assert_eq!(wake.0.load(Ordering::SeqCst), before_wakes);
        if let Some(mailbox) = &mailbox {
            assert_eq!(mailbox.storage_snapshot().queued_count, 0);
        }
        assert_eq!(other_box.storage_snapshot().queued_count, 0);
    }
}

#[test]
fn dropping_a_transfer_receipt_is_quiet_but_retires_source_not_destination() {
    let original = event("drop-receipt");
    let pointer = original.payload.as_ptr();
    let (source, source_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    let (destination, destination_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    source.try_publish(original).unwrap();
    let held = owned(&source_box);
    let source_wake = Arc::new(CountWake::default());
    let _source_listener = source
        .capacity_listener(&Waker::from(Arc::clone(&source_wake)))
        .unwrap();
    let reader_wake = Arc::new(CountWake::default());
    register_reader(&destination_box, &Waker::from(Arc::clone(&reader_wake)));
    let reservation = destination.try_reserve(1, cost(held.event())).unwrap();
    let receipt = held
        .transfer_to_deferred(&destination, reservation)
        .unwrap();
    drop(receipt);
    assert_eq!(source_box.storage_snapshot().retained_count, 0);
    assert_eq!(source_box.storage_snapshot().retained_bytes, 0);
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 0);
    assert_eq!(reader_wake.0.load(Ordering::SeqCst), 0);
    assert!(destination_box.waker.lock().unwrap().is_some());
    assert_eq!(destination_box.storage_snapshot().queued_count, 1);
    assert_eq!(destination_box.storage_snapshot().retained_count, 1);
    let received = owned(&destination_box);
    assert_eq!(received.event().payload.as_ptr(), pointer);
    received.retire();
    assert_eq!(destination_box.storage_snapshot().retained_count, 0);
    assert_eq!(destination_box.storage_snapshot().retained_bytes, 0);
}

struct PanicWake(AtomicUsize);

impl Wake for PanicWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("intentional deferred callback panic");
    }
}

#[test]
fn a_reader_callback_panic_does_not_repeat_source_release_or_next_callback() {
    let original = event("panic-receipt");
    let pointer = original.payload.as_ptr();
    let (source, source_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    let (destination, destination_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    source.try_publish(original).unwrap();
    let held = owned(&source_box);
    let source_wake = Arc::new(CountWake::default());
    let _source_listener = source
        .capacity_listener(&Waker::from(Arc::clone(&source_wake)))
        .unwrap();
    let reader_wake = Arc::new(PanicWake(AtomicUsize::new(0)));
    register_reader(&destination_box, &Waker::from(Arc::clone(&reader_wake)));
    let reservation = destination.try_reserve(1, cost(held.event())).unwrap();
    let receipt = held
        .transfer_to_deferred(&destination, reservation)
        .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| receipt.notify())).is_err());
    assert_eq!(reader_wake.0.load(Ordering::SeqCst), 1);
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 0);
    assert_eq!(source_box.storage_snapshot().retained_count, 0);
    assert_eq!(source_box.storage_snapshot().retained_bytes, 0);
    assert_eq!(destination_box.storage_snapshot().queued_count, 1);
    let received = owned(&destination_box);
    assert_eq!(received.event().payload.as_ptr(), pointer);
    received.retire();
    assert_eq!(destination_box.storage_snapshot().retained_count, 0);
}

#[test]
fn a_source_callback_panic_cannot_repeat_release_or_run_later_listeners() {
    let original = event("source-panic");
    let (source, source_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    let (destination, destination_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    source.try_publish(original).unwrap();
    let held = owned(&source_box);
    let first = Arc::new(PanicWake(AtomicUsize::new(0)));
    let later = Arc::new(CountWake::default());
    let _first_listener = source
        .capacity_listener(&Waker::from(Arc::clone(&first)))
        .unwrap();
    let _later_listener = source
        .capacity_listener(&Waker::from(Arc::clone(&later)))
        .unwrap();
    let reader_wake = Arc::new(CountWake::default());
    register_reader(&destination_box, &Waker::from(Arc::clone(&reader_wake)));
    let reservation = destination.try_reserve(1, cost(held.event())).unwrap();
    let receipt = held
        .transfer_to_deferred(&destination, reservation)
        .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| receipt.notify()));
    // A later assertion failure must not make fixture teardown invoke this
    // deliberate panicker during unwinding.
    drop(_first_listener);
    assert!(result.is_err());
    assert_eq!(reader_wake.0.load(Ordering::SeqCst), 1);
    assert_eq!(first.0.load(Ordering::SeqCst), 1);
    assert_eq!(later.0.load(Ordering::SeqCst), 0);
    assert_eq!(source_box.storage_snapshot().retained_count, 0);
    assert_eq!(source_box.storage_snapshot().retained_bytes, 0);
    assert_eq!(destination_box.storage_snapshot().queued_count, 1);
    owned(&destination_box).retire();
}

#[test]
fn a_deferred_receipt_does_not_keep_the_last_publisher_connected() {
    let (publisher, mailbox) = completion_mailbox_with_budget(1, 8192).unwrap();
    let reader = Arc::new(CountWake::default());
    register_reader(&mailbox, &Waker::from(Arc::clone(&reader)));
    let receipt = publisher.try_publish_deferred(event("disconnect")).unwrap();
    assert_eq!(reader.0.load(Ordering::SeqCst), 0);
    drop(publisher);
    assert_eq!(reader.0.load(Ordering::SeqCst), 1);
    owned(&mailbox).retire();
    assert!(matches!(mailbox.try_take_owned(), OwnedPoll::Closed));
    receipt.notify();
    assert_eq!(reader.0.load(Ordering::SeqCst), 1);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
}

#[test]
fn deferred_notification_is_not_an_event_visibility_or_retirement_barrier() {
    let original = event("already-readable");
    let pointer = original.payload.as_ptr();
    let (source, source_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    let (destination, destination_box) = completion_mailbox_with_budget(1, 8192).unwrap();
    source.try_publish(original).unwrap();
    let held = owned(&source_box);
    let source_wake = Arc::new(CountWake::default());
    let _source_listener = source
        .capacity_listener(&Waker::from(Arc::clone(&source_wake)))
        .unwrap();
    let reservation = destination.try_reserve(1, cost(held.event())).unwrap();
    let receipt = held
        .transfer_to_deferred(&destination, reservation)
        .unwrap();
    let received = owned(&destination_box);
    assert_eq!(received.event().payload.as_ptr(), pointer);
    received.retire();
    assert_eq!(destination_box.storage_snapshot().retained_count, 0);
    assert_eq!(source_box.storage_snapshot().retained_count, 1);
    receipt.notify();
    assert_eq!(source_box.storage_snapshot().retained_count, 0);
    assert_eq!(source_wake.0.load(Ordering::SeqCst), 1);
}
