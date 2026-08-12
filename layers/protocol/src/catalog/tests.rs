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
    assert!(health.allows_direction(TaskDirection::NodeController));
}
