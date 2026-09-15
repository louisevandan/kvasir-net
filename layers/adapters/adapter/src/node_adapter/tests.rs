use super::PublishError;
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
            target: Endpoint::node(Address::tcp("127.0.0.1", 52001), "node", 1),
            return_route: Some(p4_protocol::event::OuterEndpoint {
                ingress_agent: p4_protocol::Address::tcp("127.0.0.1", 52001),
                channel: "outer".into(),
                connection_generation: 1,
            }),
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

/// A refused completion comes back, and says whether waiting would help.
///
/// Both cases used to be one `Err(Event)`, and the only caller answered it by
/// dropping the completion and failing the worker - losing a token that had
/// already been computed, for a queue that was about to drain. Telling a full
/// mailbox from a closed one is what lets it wait for the first and give up on
/// the second.
#[test]
fn a_refused_completion_is_returned_with_its_reason() {
    let (publisher, mailbox) = completion_mailbox(1);
    publisher.try_publish(event("one")).unwrap();

    match publisher.try_publish(event("two")) {
        Err(PublishError::Full(returned)) => {
            assert_eq!(
                returned.envelope.event_id, "two",
                "the event comes back whole"
            );
        }
        other => panic!("a full mailbox should say so and return the event: {other:?}"),
    }

    // Draining makes room, and the same event goes in.
    assert!(matches!(mailbox.try_take(), Poll::Event(_)));
    publisher.try_publish(event("two")).unwrap();
    assert!(matches!(mailbox.try_take(), Poll::Event(_)));

    // A mailbox nobody will read again is a different answer, and waiting on
    // it would be waiting forever.
    drop(mailbox);
    match publisher.try_publish(event("three")) {
        Err(PublishError::Closed(returned)) => {
            assert_eq!(returned.envelope.event_id, "three");
        }
        other => panic!("a closed mailbox should say so: {other:?}"),
    }
}

#[test]
fn node_load_lifecycle_typed_completion_never_infers_success_from_snapshot_or_errors() {
    use p4_protocol::event::lifecycle::{LifecycleOperation, LifecycleStatus, ResourceState};

    let mut completion = AdapterLifecycleCompletion {
        operation: LifecycleOperation::Load,
        status: LifecycleStatus::Succeeded,
        resource_state: ResourceState::Present,
        first_error: None,
        cleanup_error: None,
    };
    assert!(completion.validate().is_ok());
    assert!(completion.succeeded());

    completion.resource_state = ResourceState::Unknown;
    assert_eq!(
        completion.validate(),
        Err("successful LOAD completion must prove present resources")
    );
    completion.status = LifecycleStatus::Failed;
    assert_eq!(
        completion.validate(),
        Err("rejected or failed lifecycle completion requires first_error")
    );
    completion.first_error = Some("native cleanup uncertain".into());
    assert!(completion.validate().is_ok());
    assert!(!completion.succeeded());

    completion.operation = LifecycleOperation::Unload;
    completion.status = LifecycleStatus::Succeeded;
    completion.resource_state = ResourceState::Absent;
    completion.first_error = None;
    assert!(completion.succeeded());
    completion.cleanup_error = Some("late child".into());
    assert_eq!(
        completion.validate(),
        Err("successful lifecycle completion cannot contain an error")
    );
}
