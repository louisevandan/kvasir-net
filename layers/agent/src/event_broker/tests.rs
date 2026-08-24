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
    broker.register_node("n1", node_tx).unwrap();
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
        Endpoint::node(f.own.clone(), "n1"),
        Endpoint::outer(f.own.clone(), "outer", 1),
        Endpoint::node(f.remote.clone(), "n2"),
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
    assert_eq!(
        f.broker.dispatch(event("e2", source, target, 2)),
        Err(DispatchError::Full(Delivery::Agent))
    );
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
        Err(DispatchError::Full(_))
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
