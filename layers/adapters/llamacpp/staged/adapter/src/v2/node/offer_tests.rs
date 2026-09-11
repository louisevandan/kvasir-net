//! The production try_offer implementation with no native worker thread.
//! This verifies ownership at its actual std-channel boundary, not KV work.
use super::*;

fn adapter(sender: Option<mpsc::SyncSender<WorkerInput>>) -> LlamaNodeAdapter {
    let (_publisher, mailbox) = completion_mailbox(1);
    LlamaNodeAdapter {
        sender,
        mailbox,
        snapshot: Arc::new(Mutex::new("offer-test".into())),
        shutting_down: Arc::new(AtomicBool::new(false)),
        worker: Mutex::new(None),
    }
}

fn input() -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template.clone();
    event.payload.reserve(4096);
    event.payload.extend_from_slice(&[0, 255, 128, 7]);
    event.envelope.event_id.reserve(512);
    event.validate().unwrap();
    event
}

fn allocation(event: &Event) -> (usize, usize, usize, usize) {
    (
        event.payload.as_ptr() as usize,
        event.payload.capacity(),
        event.envelope.event_id.as_ptr() as usize,
        event.envelope.event_id.capacity(),
    )
}

#[test]
fn missing_and_disconnected_senders_return_the_original_input() {
    for missing in [false, true] {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        let adapter = adapter((!missing).then_some(sender));
        let event = input();
        let expected = event.clone();
        let owned_allocation = allocation(&event);
        let Err(OfferError::Closed(returned)) = adapter.try_offer(event) else {
            panic!("the actual closed input must return its original Event");
        };
        assert_eq!(returned, expected);
        assert_eq!(allocation(&returned), owned_allocation);
        assert_eq!(adapter.snapshot(), "offer-test");
    }
}

#[test]
fn full_keeps_the_same_input_for_exactly_one_later_acceptance() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let adapter = adapter(Some(sender));
    let prefix = input();
    adapter.try_offer(prefix.clone()).unwrap();
    let mut event = input();
    event.envelope.event_id.push_str("-pending");
    let expected = event.clone();
    let owned_allocation = allocation(&event);
    let Err(OfferError::Full(returned)) = adapter.try_offer(event) else {
        panic!("a full input must return the rejected Event");
    };
    assert_eq!(returned, expected);
    assert_eq!(allocation(&returned), owned_allocation);
    let WorkerInput::Event(accepted_prefix) = receiver.try_recv().unwrap() else { panic!("raw offer changed ownership mode"); };
    assert_eq!(accepted_prefix, prefix);
    adapter.try_offer(returned).unwrap();
    let WorkerInput::Event(accepted) = receiver.try_recv().unwrap() else { panic!("raw offer changed ownership mode"); };
    assert_eq!(accepted, expected);
    assert_eq!(allocation(&accepted), owned_allocation);
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}
