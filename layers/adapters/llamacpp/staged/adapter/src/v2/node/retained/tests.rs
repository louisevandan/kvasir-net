use super::*;
use p4_adapter::node_adapter::{CompletionPublisher, retained_event_bytes};
use std::time::{Duration, Instant};

fn input(name: &str) -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template.clone();
    event.envelope.event_id = format!("owned-{name}");
    event.envelope.correlation_id = name.into();
    event.envelope.payload_content_type = "application/unsupported-owned-probe".into();
    event.payload.reserve_exact(8192);
    event.payload.extend([0, 128, 255, 7]);
    event
}
fn owned(
    publisher: &CompletionPublisher,
    mailbox: &CompletionMailbox,
    event: Event,
) -> RetainedCompletion {
    let claim = publisher
        .try_reserve(1, retained_event_bytes(&event).unwrap())
        .unwrap();
    publisher.publish_reserved(event, claim).unwrap();
    match mailbox.try_take_owned() {
        OwnedPoll::Event(event) => event,
        p => panic!("{p:?}"),
    }
}
fn until(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(
            Instant::now() < end,
            "actual owned worker did not reach the expected hold"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn offer(adapter: &RetainedLlamaNodeAdapter, completion: RetainedCompletion) {
    let mut pending = Some(completion);
    until(
        || match adapter.try_offer_retained(pending.take().unwrap()) {
            Ok(()) => true,
            Err(RetainedOfferError::Full(event)) => {
                pending = Some(event);
                false
            }
            other => panic!("{other:?}"),
        },
    );
}

#[test]
fn owned_worker_stop_retains_active_obstructing_and_queued_original_inputs() {
    let (publisher, source) = completion_mailbox_with_limits(1, 8, 1 << 20).unwrap();
    let first = input("first");
    let adapter =
        RetainedLlamaNodeAdapter::new(first.envelope.target.clone(), 1, 1, 8, 1 << 20).unwrap();
    offer(&adapter, owned(&publisher, &source, first));
    until(|| {
        adapter.inner.mailbox.storage_snapshot().queued_count == 1
            && source.storage_snapshot().retained_count == 0
    });
    assert!(
        matches!(adapter.inner.mailbox.try_take(), Poll::Empty),
        "raw consumer cannot strip an owned completion claim"
    );
    let active = input("active");
    let active_pointer = active.payload.as_ptr();
    offer(&adapter, owned(&publisher, &source, active));
    until(|| adapter.snapshot() == "completion_queue_full:waiting");
    assert_eq!(source.storage_snapshot().retained_count, 1);
    let mut bad_ack = input("bad-ack");
    bad_ack.envelope.payload_content_type = crate::v2::RELEASED_CONTENT_TYPE.into();
    let ack_pointer = bad_ack.payload.as_ptr();
    offer(&adapter, owned(&publisher, &source, bad_ack));
    let obstructing = input("obstructing");
    let obstructing_pointer = obstructing.payload.as_ptr();
    offer(&adapter, owned(&publisher, &source, obstructing));
    let queued = input("queued");
    let queued_pointer = queued.payload.as_ptr();
    // The second successful std-channel offer proves the first non-ACK left
    // that channel while native/recursive command service remained blocked.
    offer(&adapter, owned(&publisher, &source, queued));
    assert_eq!(source.storage_snapshot().retained_count, 4);
    adapter.inner.shutting_down.store(true, Ordering::Release);
    let thread = adapter.inner.worker.lock().unwrap().take().unwrap();
    thread.join().unwrap();
    assert!(adapter.stopped.load(Ordering::Acquire));
    assert_eq!(
        source.storage_snapshot().retained_count,
        4,
        "worker exit is not input retirement"
    );
    {
        let remainder = adapter._remainder.lock().unwrap();
        let remainder = remainder.as_ref().unwrap();
        assert_eq!(
            remainder
                .failed_input
                .as_ref()
                .unwrap()
                .event()
                .payload
                .as_ptr(),
            active_pointer
        );
        assert_eq!(
            remainder
                .held_input
                .as_ref()
                .unwrap()
                .event()
                .payload
                .as_ptr(),
            obstructing_pointer
        );
        assert_eq!(
            remainder
                .deferred_ack_error
                .as_ref()
                .unwrap()
                .0
                .event()
                .payload
                .as_ptr(),
            ack_pointer
        );
        let queued = remainder.receiver.try_recv().unwrap();
        assert_eq!(queued.event().payload.as_ptr(), queued_pointer);
        assert!(
            remainder.effect_count() > 0,
            "the exact failed publication remains owned"
        );
        assert_eq!(remainder.state.requests.len(), 0);
        drop(queued);
    }
    assert_eq!(source.storage_snapshot().retained_count, 3);
    let retry = owned(&publisher, &source, input("after-stop"));
    let pointer = retry.event().payload.as_ptr();
    let Err(RetainedOfferError::Closed(retry)) = adapter.try_offer_retained(retry) else {
        panic!("stopped worker accepted more input");
    };
    assert_eq!(retry.event().payload.as_ptr(), pointer);
    retry.retire();
    drop(adapter); // Explicit local abandonment retires values and claims together.
    assert_eq!(source.storage_snapshot().retained_count, 0);
    assert_eq!(source.storage_snapshot().retained_bytes, 0);
}
