use super::*;
use p4_protocol::event::{Envelope, EventClass};

fn event(id: &str, source: Endpoint, target: Endpoint, sequence: u64) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: "correlation".into(),
            causation_id: None,
            source,
            target,
            return_route: None,
            class: EventClass::Control,
            sequence,
            deadline_unix_ms: None,
            adapter_kind: None,
            payload_content_type: "application/test".into(),
        },
        payload: vec![7],
    }
}

struct Fixture {
    broker: EventBroker,
    own: Address,
    remote: Address,
    agent: EventReceiver,
    outer: EventReceiver,
    outbound: EventReceiver,
    node: EventReceiver,
}

fn fixture(capacity: usize) -> Fixture {
    let own = Address::tcp("127.0.0.1", 52001);
    let remote = Address::tcp("127.0.0.2", 52001);
    let (agent_tx, agent) = bounded_queue(capacity);
    let (outer_tx, outer) = bounded_queue(capacity);
    let (outbound_tx, outbound) = bounded_queue(capacity);
    let (node_tx, node) = bounded_queue(capacity);
    let broker = EventBroker::new(own.clone(), agent_tx, outer_tx, outbound_tx, 8);
    broker.register_node("n1", 1, node_tx).unwrap();
    Fixture {
        broker,
        own,
        remote,
        agent,
        outer,
        outbound,
        node,
    }
}

#[test]
fn target_alone_selects_all_four_destinations() {
    let mut f = fixture(4);
    let source = Endpoint::agent(f.remote.clone());
    let targets = [
        Endpoint::agent(f.own.clone()),
        Endpoint::node(f.own.clone(), "n1", 1),
        Endpoint::outer(f.own.clone(), "outer", 1),
        Endpoint::node(f.remote.clone(), "n2", 1),
    ];
    for (index, target) in targets.into_iter().enumerate() {
        f.broker
            .dispatch(event(
                &format!("e{index}"),
                source.clone(),
                target,
                index as u64 + 1,
            ))
            .unwrap();
    }
    assert!(f.agent.try_recv().is_ok());
    assert!(f.node.try_recv().is_ok());
    assert!(f.outer.try_recv().is_ok());
    let forwarded = f.outbound.try_recv().unwrap();
    assert_eq!(forwarded.envelope.target.agent_address(), &f.remote);
}

#[test]
fn a_full_queue_is_reported_without_a_blocking_fallback() {
    let f = fixture(1);
    let source = Endpoint::agent(f.remote.clone());
    let target = Endpoint::agent(f.own.clone());
    f.broker
        .dispatch(event("e1", source.clone(), target.clone(), 1))
        .unwrap();
    // The event comes back with the refusal so the caller can retry it.
    let refused = f.broker.dispatch(event("e2", source, target, 2));
    let Err(DispatchError::Full(delivery, returned)) = refused else {
        panic!("a full destination should return the event: {refused:?}");
    };
    assert_eq!(delivery, Delivery::Agent);
    assert_eq!(returned.envelope.event_id, "e2");
}

#[test]
fn a_full_offer_does_not_consume_identity_or_sequence() {
    let mut f = fixture(1);
    let source = Endpoint::agent(f.remote.clone());
    let target = Endpoint::agent(f.own.clone());
    f.broker
        .dispatch(event("e1", source.clone(), target.clone(), 1))
        .unwrap();
    let pending = event("e2", source, target, 2);
    assert!(matches!(
        f.broker.dispatch(pending.clone()),
        Err(DispatchError::Full(..))
    ));
    f.agent.try_recv().unwrap();
    assert!(matches!(
        f.broker.dispatch(pending),
        Ok(DispatchOutcome::Enqueued(Delivery::Agent))
    ));
}

#[test]
fn exact_duplicates_are_idempotent_but_changed_bytes_are_rejected() {
    let f = fixture(2);
    let original = event(
        "e1",
        Endpoint::agent(f.remote.clone()),
        Endpoint::agent(f.own.clone()),
        1,
    );
    f.broker.dispatch(original.clone()).unwrap();
    assert_eq!(
        f.broker.dispatch(original.clone()),
        Ok(DispatchOutcome::Duplicate)
    );
    let mut conflict = original;
    conflict.payload.push(9);
    assert_eq!(
        f.broker.dispatch(conflict),
        Err(DispatchError::ConflictingDuplicate)
    );
}

#[test]
fn a_new_event_cannot_move_a_source_sequence_backwards() {
    let f = fixture(2);
    let source = Endpoint::agent(f.remote.clone());
    let target = Endpoint::agent(f.own.clone());
    f.broker
        .dispatch(event("e2", source.clone(), target.clone(), 2))
        .unwrap();
    assert_eq!(
        f.broker.dispatch(event("e1", source, target, 1)),
        Err(DispatchError::SequenceRegression {
            previous: 2,
            incoming: 1,
        })
    );
}

#[test]
fn recreated_node_has_a_new_source_sequence_domain_and_rejects_stale_targets() {
    let f = fixture(4);
    let target = Endpoint::agent(f.own.clone());
    f.broker
        .dispatch(event(
            "old-generation",
            Endpoint::node(f.own.clone(), "n1", 1),
            target.clone(),
            2,
        ))
        .unwrap();

    assert!(f.broker.unregister_node("n1", 1).unwrap());
    let (reused_sender, _reused_receiver) = bounded_queue(1);
    assert_eq!(
        f.broker.register_node("n1", 1, reused_sender),
        Err(DispatchError::StaleNode {
            node: "n1".into(),
            current_generation: 1,
            incoming_generation: 1,
        })
    );
    let (new_sender, mut new_receiver) = bounded_queue(1);
    f.broker.register_node("n1", 2, new_sender).unwrap();

    f.broker
        .dispatch(event(
            "new-generation",
            Endpoint::node(f.own.clone(), "n1", 2),
            target,
            1,
        ))
        .unwrap();
    assert_eq!(
        f.broker.dispatch(event(
            "stale-target",
            Endpoint::agent(f.remote.clone()),
            Endpoint::node(f.own.clone(), "n1", 1),
            1,
        )),
        Err(DispatchError::StaleNode {
            node: "n1".into(),
            current_generation: 2,
            incoming_generation: 1,
        })
    );
    assert!(new_receiver.try_recv().is_err());
}

#[test]
fn full_returns_the_original_allocations_on_every_retry_before_exact_acceptance() {
    let mut f = fixture(1);
    let source = Endpoint::agent(f.remote.clone());
    let target = Endpoint::agent(f.own.clone());
    f.broker
        .dispatch(event("prefix", source.clone(), target.clone(), 1))
        .unwrap();
    let mut pending = event("pending", source, target, 2);
    pending.payload = Vec::with_capacity(8192);
    pending.payload.extend_from_slice(&[0, 255, 128, 7]);
    pending.envelope.event_id.reserve(1024);
    let payload_ptr = pending.payload.as_ptr();
    let payload_capacity = pending.payload.capacity();
    let id_ptr = pending.envelope.event_id.as_ptr();
    let id_capacity = pending.envelope.event_id.capacity();
    let expected = pending.clone();
    for _ in 0..3 {
        let Err(DispatchError::Full(Delivery::Agent, returned)) = f.broker.dispatch(pending) else {
            panic!("the occupied destination must refuse without taking ownership");
        };
        assert_eq!(*returned, expected);
        assert_eq!(returned.payload.as_ptr(), payload_ptr);
        assert_eq!(returned.payload.capacity(), payload_capacity);
        assert_eq!(returned.envelope.event_id.as_ptr(), id_ptr);
        assert_eq!(returned.envelope.event_id.capacity(), id_capacity);
        pending = *returned;
    }
    assert_eq!(f.agent.try_recv().unwrap().envelope.event_id, "prefix");
    assert_eq!(
        f.broker.dispatch(pending),
        Ok(DispatchOutcome::Enqueued(Delivery::Agent))
    );
    assert_eq!(f.agent.try_recv().unwrap(), expected);
    assert_eq!(f.broker.dispatch(expected), Ok(DispatchOutcome::Duplicate));
    assert!(f.agent.try_recv().is_err());
}

#[test]
fn changing_destination_does_not_create_a_new_source_correlation_order_domain() {
    let mut f = fixture(1);
    let source = Endpoint::node(f.remote.clone(), "producer", 1);
    let mut physical = event(
        "physical",
        source.clone(),
        Endpoint::node(f.own.clone(), "n1", 1),
        1,
    );
    physical.envelope.class = EventClass::Data;
    let mut observation = event(
        "observation",
        source.clone(),
        Endpoint::outer(f.own.clone(), "sink", 1),
        2,
    );
    observation.envelope.class = EventClass::Telemetry;
    let ordered_first = physical.clone();
    let ordered_second = observation.clone();
    f.broker.dispatch(observation.clone()).unwrap();
    assert_eq!(
        f.broker.dispatch(physical.clone()),
        Err(DispatchError::SequenceRegression {
            previous: 2,
            incoming: 1,
        })
    );
    assert!(f.node.try_recv().is_err());
    assert_eq!(f.outer.try_recv().unwrap(), observation);

    // An independent correlation is a different order domain. It must not
    // be rejected merely because the source or destinations are shared.
    physical.envelope.correlation_id = "independent".into();
    f.broker.dispatch(physical.clone()).unwrap();
    assert_eq!(f.node.try_recv().unwrap(), physical);

    // Same exact inputs in their valid order remain a positive control.
    let mut control = fixture(1);
    control.broker.dispatch(ordered_first.clone()).unwrap();
    control.broker.dispatch(ordered_second.clone()).unwrap();
    assert_eq!(control.node.try_recv().unwrap(), ordered_first);
    assert_eq!(control.outer.try_recv().unwrap(), ordered_second);
}
