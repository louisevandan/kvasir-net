//! Ordinary-front inspection and conditional dequeue, not retained handoff.
use super::*;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, EventClass};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Wake;

fn event(id: &str) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "matching-request".into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.1", 52201)),
            target: Endpoint::node(Address::tcp("127.0.0.1", 52201), "mock", 1),
            return_route: Some(p4_protocol::event::OuterEndpoint {
                ingress_agent: p4_protocol::Address::tcp("127.0.0.1", 52001),
                channel: "outer".into(),
                connection_generation: 1,
            }),
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("neutral-matching-test".into()),
            payload_content_type: "application/test".into(),
        },
        payload: vec![0, 128, 255, 3],
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
fn peek_and_mismatched_envelopes_preserve_queue_claim_and_notifications() {
    let (publisher, mailbox) = completion_mailbox(1);
    let reader = Arc::new(CountWake::default());
    let capacity = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&reader));
    assert_eq!(
        mailbox.poll_take(&mut Context::from_waker(&waker)),
        TaskPoll::Pending
    );
    let _registration = publisher
        .capacity_listener(&Waker::from(Arc::clone(&capacity)))
        .unwrap();
    let original = event("original");
    let expected = original.envelope.clone();
    let notification = publisher.try_publish_deferred(original).unwrap();
    let before = mailbox.storage_snapshot();
    let mut mismatches = Vec::new();
    let mut changed = expected.clone();
    changed.event_id.push_str("-different");
    mismatches.push(changed);
    let mut changed = expected.clone();
    changed.correlation_id.push_str("-different");
    mismatches.push(changed);
    let mut changed = expected.clone();
    changed.source = Endpoint::agent(Address::tcp("127.0.0.1", 52202));
    mismatches.push(changed);
    let mut changed = expected.clone();
    changed.target = Endpoint::node(Address::tcp("127.0.0.1", 52201), "other", 1);
    mismatches.push(changed);
    let mut changed = expected.clone();
    changed.sequence += 1;
    mismatches.push(changed);
    for mismatch in mismatches {
        assert_eq!(mailbox.peek_completion(), Some(expected.clone()));
        assert_eq!(mailbox.try_take_completion_matching(&mismatch), Poll::Empty);
        assert_eq!(mailbox.storage_snapshot(), before);
        assert_eq!(mailbox.peek_completion(), Some(expected.clone()));
        assert_eq!(reader.0.load(Ordering::SeqCst), 0);
        assert_eq!(capacity.0.load(Ordering::SeqCst), 0);
        assert!(mailbox.waker.lock().unwrap().is_some());
    }
    notification.notify();
    assert_eq!(reader.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        mailbox.try_take_completion_matching(&expected),
        Poll::Event(_)
    ));
    assert_eq!(capacity.0.load(Ordering::SeqCst), 1);
}

struct UnlockedCapacityWake {
    mailbox: Weak<CompletionMailbox>,
    calls: AtomicUsize,
    saw_locked: AtomicBool,
}

impl Wake for UnlockedCapacityWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let mailbox = self.mailbox.upgrade().unwrap();
        self.saw_locked.store(
            mailbox.receiver.try_lock().is_err() || mailbox.budget.try_lock().is_err(),
            Ordering::SeqCst,
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn matching_returns_the_original_allocation_and_releases_after_unlock() {
    let (publisher, mailbox) = completion_mailbox(1);
    let wake = Arc::new(UnlockedCapacityWake {
        mailbox: Arc::downgrade(&mailbox),
        calls: AtomicUsize::new(0),
        saw_locked: AtomicBool::new(false),
    });
    let _registration = publisher
        .capacity_listener(&Waker::from(Arc::clone(&wake)))
        .unwrap();
    let original = event("allocation");
    let expected = original.clone();
    let pointer = original.payload.as_ptr();
    publisher.try_publish(original).unwrap();
    let peeked = mailbox.peek_completion().unwrap();
    let Poll::Event(returned) = mailbox.try_take_completion_matching(&peeked) else {
        panic!("matching ordinary completion must be returned");
    };
    assert_eq!(returned, expected);
    assert_eq!(returned.payload.as_ptr(), pointer);
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(mailbox.storage_snapshot().retained_bytes, 0);
    assert_eq!(mailbox.storage_snapshot().queued_count, 0);
    assert_eq!(wake.calls.load(Ordering::SeqCst), 1);
    assert!(!wake.saw_locked.load(Ordering::SeqCst));
    assert_eq!(mailbox.peek_completion(), None);
    assert_eq!(mailbox.try_take_completion_matching(&peeked), Poll::Empty);
    assert_eq!(wake.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn matching_neither_consumes_reserved_front_nor_skips_to_ordinary_suffix() {
    let (publisher, mailbox) = completion_mailbox(2);
    let reserved = event("reserved");
    let expected_reserved = reserved.clone();
    let reservation = publisher
        .try_reserve(1, retained_event_bytes(&reserved).unwrap())
        .unwrap();
    publisher.publish_reserved(reserved, reservation).unwrap();
    let ordinary = event("ordinary");
    let expected_ordinary = ordinary.clone();
    publisher.try_publish(ordinary).unwrap();
    let before = mailbox.storage_snapshot();
    assert_eq!(mailbox.peek_completion(), None);
    for envelope in [&expected_reserved.envelope, &expected_ordinary.envelope] {
        assert_eq!(mailbox.try_take_completion_matching(envelope), Poll::Empty);
        assert_eq!(mailbox.storage_snapshot(), before);
    }
    let OwnedPoll::Event(held) = mailbox.try_take_owned() else {
        panic!("reserved front is still present for the owned reader");
    };
    assert_eq!(held.event(), &expected_reserved);
    assert_eq!(mailbox.storage_snapshot().retained_count, 2);
    assert_eq!(
        mailbox.peek_completion(),
        Some(expected_ordinary.envelope.clone())
    );
    assert_eq!(
        mailbox.try_take_completion_matching(&expected_ordinary.envelope),
        Poll::Event(expected_ordinary)
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 1);
    held.retire();
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
}

#[test]
fn matching_drains_buffered_ordinary_events_before_last_publisher_closure() {
    let (publisher, mailbox) = completion_mailbox(2);
    let first = event("first");
    let second = event("second");
    assert_eq!(mailbox.peek_completion(), None);
    assert_eq!(
        mailbox.try_take_completion_matching(&first.envelope),
        Poll::Empty
    );
    publisher.try_publish(first.clone()).unwrap();
    publisher.try_publish(second.clone()).unwrap();
    drop(publisher);
    assert_eq!(mailbox.peek_completion(), Some(first.envelope.clone()));
    assert_eq!(
        mailbox.try_take_completion_matching(&second.envelope),
        Poll::Empty
    );
    assert_eq!(
        mailbox.try_take_completion_matching(&first.envelope),
        Poll::Event(first)
    );
    assert_eq!(mailbox.peek_completion(), Some(second.envelope.clone()));
    let second_envelope = second.envelope.clone();
    assert_eq!(
        mailbox.try_take_completion_matching(&second_envelope),
        Poll::Event(second)
    );
    assert_eq!(mailbox.peek_completion(), None);
    assert_eq!(
        mailbox.try_take_completion_matching(&second_envelope),
        Poll::Closed
    );
    assert_eq!(mailbox.storage_snapshot().retained_count, 0);
}
