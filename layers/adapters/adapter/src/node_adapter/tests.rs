use super::*;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, EventClass};

fn event(id: &str) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "request".into(),
            causation_id: None,
            source: Endpoint::agent(Address::tcp("127.0.0.1", 52001)),
            target: Endpoint::node(Address::tcp("127.0.0.1", 52001), "node"),
            return_route: None,
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("test".into()),
            payload_content_type: "application/test".into(),
        },
        payload: vec![],
    }
}

#[test]
fn completion_mailbox_never_waits_for_capacity() {
    let (publisher, mailbox) = completion_mailbox(1);
    publisher.try_publish(event("one")).unwrap();
    assert!(publisher.try_publish(event("two")).is_err());
    assert!(matches!(mailbox.try_take(), Poll::Event(_)));
    assert_eq!(mailbox.try_take(), Poll::Empty);
}
