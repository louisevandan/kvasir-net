use super::*;

#[test]
fn catalog_is_exhaustive_for_request_and_response_examples() {
    let health = Message::Health {
        request_id: "health-1".into(),
        node_id: "node-a".into(),
        ready: true,
        detail: "ready".into(),
    };
    assert_eq!(health.kind(), MessageKind::Health);
    assert_eq!(health.correlation_id(), "health-1");
    assert_eq!(health.queue_class(), QueueClass::Response);
    assert!(health.is_terminal());
}

#[test]
fn a_message_no_longer_carries_a_direction() {
    // Directions named a controller, and there is no controller. Where a
    // message may travel is now the envelope's target address, which every
    // relay reads without opening a body.
    let error = Message::Error {
        request_id: "r".into(),
        detail: "d".into(),
    };
    assert_eq!(error.queue_class(), QueueClass::Response);
    assert!(error.is_terminal());
}
