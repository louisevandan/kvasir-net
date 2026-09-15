//! The actual synchronous completion-front reservation and commit API.
//! These are destination-slot/identity tests, not retained-byte or causal
//! return-credit proofs. No adapter payload or SESSION-specific path is used.
use super::*;

fn input(f: &Fixture, id: &str, target: Endpoint, sequence: u64) -> Event {
    let mut value = event(id, Endpoint::agent(f.remote.clone()), target, sequence);
    value.payload = Vec::with_capacity(8192);
    value.payload.extend_from_slice(&[0, 255, 128, 7]);
    value.envelope.event_id.reserve(1024);
    value
}

fn ledger_snapshot(broker: &EventBroker) -> String {
    format!("{:?}", *broker.ledger.lock().unwrap())
}

fn allocations(value: &Event) -> (*const u8, usize, *const u8, usize) {
    (
        value.payload.as_ptr(),
        value.payload.capacity(),
        value.envelope.event_id.as_ptr(),
        value.envelope.event_id.capacity(),
    )
}

fn refuse(
    broker: &EventBroker,
    ticket: CompletionDispatch,
    value: Event,
    reason: DispatchError,
) -> Event {
    let before = ledger_snapshot(broker);
    let expected = value.clone();
    let storage = allocations(&value);
    let failure = broker
        .dispatch_completion(ticket, value)
        .expect_err("a refused completion must return its original");
    assert_eq!(failure.error, reason);
    assert_eq!(*failure.event, expected);
    assert_eq!(allocations(&failure.event), storage);
    assert_eq!(
        ledger_snapshot(broker),
        before,
        "refusal must not record an Event or advance its stream"
    );
    *failure.event
}

#[test]
fn full_front_reservation_does_not_take_the_callers_event_or_consume_identity() {
    let mut f = fixture(1);
    let target = Endpoint::agent(f.own.clone());
    let prefix = input(&f, "prefix", target.clone(), 1);
    f.broker.dispatch(prefix.clone()).unwrap();
    let pending = input(&f, "pending", target, 2);
    let expected = pending.clone();
    let storage = allocations(&pending);
    let before = ledger_snapshot(&f.broker);
    for _ in 0..3 {
        assert!(matches!(
            f.broker.reserve_completion(&pending.envelope),
            Err(DispatchError::Full(Delivery::Agent))
        ));
        assert_eq!(pending, expected);
        assert_eq!(allocations(&pending), storage);
        assert_eq!(ledger_snapshot(&f.broker), before);
    }
    assert_eq!(f.agent.try_recv().unwrap(), prefix);
    let ticket = f.broker.reserve_completion(&pending.envelope).unwrap();
    assert_eq!(f.broker.agent.capacity(), 0, "an actual slot is held");
    assert_eq!(
        f.broker.dispatch_completion(ticket, pending),
        Ok(DispatchOutcome::Enqueued(Delivery::Agent))
    );
    let delivered = f.agent.try_recv().unwrap();
    assert_eq!(delivered, expected);
    assert_eq!(allocations(&delivered), storage);
    assert_eq!(f.broker.dispatch(expected), Ok(DispatchOutcome::Duplicate));
    assert!(f.agent.try_recv().is_err());
}

#[test]
fn cancelling_a_front_ticket_returns_its_slot_without_receipt_or_sequence() {
    let mut f = fixture(1);
    let pending = input(&f, "cancel-ticket", Endpoint::agent(f.own.clone()), 1);
    let expected = pending.clone();
    let before = ledger_snapshot(&f.broker);
    let ticket = f.broker.reserve_completion(&pending.envelope).unwrap();
    assert_eq!(f.broker.agent.capacity(), 0);
    assert!(matches!(
        f.broker.reserve_completion(&pending.envelope),
        Err(DispatchError::Full(Delivery::Agent))
    ));
    drop(ticket);
    assert_eq!(f.broker.agent.capacity(), 1);
    assert_eq!(ledger_snapshot(&f.broker), before);
    assert_eq!(
        f.broker.dispatch(pending),
        Ok(DispatchOutcome::Enqueued(Delivery::Agent))
    );
    assert_eq!(f.agent.try_recv().unwrap(), expected);
}

#[test]
fn existing_duplicate_and_conflict_precede_full_and_closed_destinations() {
    for closed in [false, true] {
        let mut f = fixture(1);
        let original = input(&f, "existing", Endpoint::agent(f.own.clone()), 1);
        f.broker.dispatch(original.clone()).unwrap();
        let newer = input(
            &f,
            "later-in-stream",
            Endpoint::outer(f.own.clone(), "sink", 1),
            2,
        );
        f.broker.dispatch(newer.clone()).unwrap();
        if closed {
            f.agent.close();
        }
        let mut fresh = original.clone();
        fresh.envelope.event_id = "fresh-positive-control".into();
        fresh.envelope.sequence = 3;
        let refusal = f.broker.reserve_completion(&fresh.envelope);
        assert!(matches!(
            (closed, refusal),
            (false, Err(DispatchError::Full(Delivery::Agent)))
                | (true, Err(DispatchError::Closed(Delivery::Agent)))
        ));
        let before = ledger_snapshot(&f.broker);
        let ticket = f.broker.reserve_completion(&original.envelope).unwrap();
        assert_eq!(
            f.broker.dispatch_completion(ticket, original.clone()),
            Ok(DispatchOutcome::Duplicate)
        );
        assert_eq!(ledger_snapshot(&f.broker), before);
        let mut conflict = input(&f, "existing", Endpoint::agent(f.own.clone()), 1);
        conflict.payload.push(9);
        let ticket = f.broker.reserve_completion(&conflict.envelope).unwrap();
        let returned = refuse(
            &f.broker,
            ticket,
            conflict,
            DispatchError::ConflictingDuplicate,
        );
        assert_eq!(returned.payload.last(), Some(&9));
        assert_eq!(f.agent.try_recv().unwrap(), original);
        assert_eq!(f.outer.try_recv().unwrap(), newer);
        assert!(f.agent.try_recv().is_err(), "neither retry is enqueued");
    }
}

#[test]
fn existing_ticket_pins_exact_receipt_through_window_eviction() {
    let mut f = fixture(1);
    let target = Endpoint::agent(f.own.clone());
    let original = input(&f, "evicted-original", target.clone(), 1);
    f.broker.dispatch(original.clone()).unwrap();
    let duplicate_ticket = f.broker.reserve_completion(&original.envelope).unwrap();
    let mut conflict = input(&f, "evicted-original", target.clone(), 1);
    conflict.payload.push(9);
    let conflict_ticket = f.broker.reserve_completion(&conflict.envelope).unwrap();
    assert_eq!(f.agent.try_recv().unwrap(), original);
    // Public dispatches perform real bounded-window eviction while the two
    // completion operations retain their prepare-time exact receipt.
    for sequence in 2..=10 {
        let newer = input(&f, &format!("newer-{sequence}"), target.clone(), sequence);
        f.broker.dispatch(newer.clone()).unwrap();
        assert_eq!(f.agent.try_recv().unwrap(), newer);
    }
    assert!(
        matches!(
            f.broker
                .ledger
                .lock()
                .unwrap()
                .inspect_completion_header(&original.envelope),
            Err(DispatchError::SequenceRegression {
                previous: 10,
                incoming: 1
            })
        ),
        "the test must actually cross the receipt window"
    );
    f.agent.close();
    let before = ledger_snapshot(&f.broker);
    assert_eq!(
        f.broker
            .dispatch_completion(duplicate_ticket, original.clone()),
        Ok(DispatchOutcome::Duplicate)
    );
    assert_eq!(ledger_snapshot(&f.broker), before);
    refuse(
        &f.broker,
        conflict_ticket,
        conflict,
        DispatchError::ConflictingDuplicate,
    );
    let raw_failure = f.broker.dispatch(original).unwrap_err();
    assert_eq!(
        raw_failure.error,
        DispatchError::SequenceRegression {
            previous: 10,
            incoming: 1,
        },
        "without the in-progress pin this really is no longer a duplicate"
    );
    assert!(f.agent.try_recv().is_err());
}

#[test]
fn front_probe_rejects_invalid_input_before_existing_receipt_or_full_destination() {
    let mut f = fixture(1);
    let target = Endpoint::agent(f.own.clone());
    let original = input(&f, "invalid-existing", target.clone(), 1);
    f.broker.dispatch(original.clone()).unwrap();
    let mut invalid = input(&f, "invalid-existing", target, 1);
    invalid.envelope.protocol_version = 0;
    let expected_error = DispatchError::Invalid(invalid.validate().unwrap_err().to_string());
    let before = ledger_snapshot(&f.broker);
    let expected = invalid.clone();
    let storage = allocations(&invalid);
    match f.broker.reserve_completion(&invalid.envelope) {
        Err(error) => assert_eq!(error, expected_error),
        Ok(_) => panic!("a malformed existing-ID probe cannot gain a ticket"),
    }
    assert_eq!(invalid, expected);
    assert_eq!(allocations(&invalid), storage);
    assert_eq!(ledger_snapshot(&f.broker), before);
    invalid.envelope.protocol_version = Envelope::VERSION;
    let ticket = f.broker.reserve_completion(&invalid.envelope).unwrap();
    assert_eq!(
        f.broker.dispatch_completion(ticket, invalid),
        Ok(DispatchOutcome::Duplicate)
    );
    assert_eq!(f.agent.try_recv().unwrap(), original);
    assert!(f.agent.try_recv().is_err());
}

#[test]
fn front_probe_rejects_sequence_regression_before_destination_full() {
    let mut f = fixture(1);
    let target = Endpoint::agent(f.own.clone());
    let previous = input(&f, "previous", target.clone(), 4);
    f.broker.dispatch(previous.clone()).unwrap();
    let mut regressing = input(&f, "regressing-front", target, 3);
    let expected = regressing.clone();
    let storage = allocations(&regressing);
    let before = ledger_snapshot(&f.broker);
    match f.broker.reserve_completion(&regressing.envelope) {
        Err(error) => assert_eq!(
            error,
            DispatchError::SequenceRegression {
                previous: 4,
                incoming: 3
            },
        ),
        Ok(_) => panic!("a regressing front cannot gain a destination ticket"),
    }
    assert_eq!(regressing, expected);
    assert_eq!(allocations(&regressing), storage);
    assert_eq!(ledger_snapshot(&f.broker), before);
    regressing.envelope.sequence = 5;
    assert!(matches!(
        f.broker.reserve_completion(&regressing.envelope),
        Err(DispatchError::Full(Delivery::Agent))
    ));
    assert_eq!(f.agent.try_recv().unwrap(), previous);
    let expected = regressing.clone();
    let ticket = f.broker.reserve_completion(&regressing.envelope).unwrap();
    f.broker.dispatch_completion(ticket, regressing).unwrap();
    assert_eq!(f.agent.try_recv().unwrap(), expected);
}

#[test]
fn changed_completion_envelope_returns_original_and_releases_reserved_slot() {
    let mut f = fixture(1);
    let mut value = input(&f, "changed-front", Endpoint::agent(f.own.clone()), 1);
    let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
    value.envelope.correlation_id = "replacement-stream".into();
    let returned = refuse(
        &f.broker,
        ticket,
        value,
        DispatchError::Invalid("completion front changed after reservation".into()),
    );
    assert_eq!(f.broker.agent.capacity(), 1);
    assert!(f.agent.try_recv().is_err());
    let expected = returned.clone();
    let ticket = f.broker.reserve_completion(&returned.envelope).unwrap();
    f.broker.dispatch_completion(ticket, returned).unwrap();
    assert_eq!(f.agent.try_recv().unwrap(), expected);
}

#[test]
fn recreated_route_cannot_use_a_previous_generations_reserved_slot() {
    let mut f = fixture(1);
    let value = input(
        &f,
        "route-generation",
        Endpoint::node(f.own.clone(), "n1", 1),
        1,
    );
    let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
    assert!(f.broker.unregister_node("n1", 1).unwrap());
    let (sender, mut receiver) = bounded_queue(1);
    f.broker.register_node("n1", 2, sender).unwrap();
    let mut returned = refuse(
        &f.broker,
        ticket,
        value,
        DispatchError::StaleNode {
            node: "n1".into(),
            current_generation: 2,
            incoming_generation: 1,
        },
    );
    assert!(f.node.try_recv().is_err());
    assert!(receiver.try_recv().is_err());
    returned.envelope.target = Endpoint::node(f.own.clone(), "n1", 2);
    let expected = returned.clone();
    let ticket = f.broker.reserve_completion(&returned.envelope).unwrap();
    assert_eq!(
        f.broker.dispatch_completion(ticket, returned),
        Ok(DispatchOutcome::Enqueued(Delivery::Node {
            node: "n1".into(),
            generation: 2,
        }))
    );
    assert_eq!(receiver.try_recv().unwrap(), expected);
}

#[test]
fn reserved_slot_is_bound_to_the_actual_channel_not_only_route_labels() {
    let mut f = fixture(1);
    let value = input(
        &f,
        "route-channel",
        Endpoint::node(f.own.clone(), "n1", 1),
        1,
    );
    let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
    let (sender, mut receiver) = bounded_queue(1);
    // Public registration forbids replacing a live generation. This explicit
    // test-only substitution isolates the additional same-channel guard from
    // the real generation-change test above; it is not a production rebind API.
    f.broker
        .nodes
        .write()
        .unwrap()
        .get_mut("n1")
        .unwrap()
        .sender = sender;
    let returned = refuse(
        &f.broker,
        ticket,
        value,
        DispatchError::Invalid("completion destination changed after reservation".into()),
    );
    assert!(f.node.try_recv().is_err());
    assert!(receiver.try_recv().is_err());
    let expected = returned.clone();
    let ticket = f.broker.reserve_completion(&returned.envelope).unwrap();
    f.broker.dispatch_completion(ticket, returned).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), expected);
}

#[test]
fn queue_ticket_rechecks_sequence_changed_by_a_real_interleaved_dispatch() {
    let mut f = fixture(1);
    let value = input(&f, "older", Endpoint::node(f.own.clone(), "n1", 1), 1);
    let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
    let newer = input(&f, "newer", Endpoint::outer(f.own.clone(), "sink", 1), 2);
    f.broker.dispatch(newer.clone()).unwrap();
    let mut returned = refuse(
        &f.broker,
        ticket,
        value,
        DispatchError::SequenceRegression {
            previous: 2,
            incoming: 1,
        },
    );
    assert!(f.node.try_recv().is_err());
    assert_eq!(f.outer.try_recv().unwrap(), newer);
    returned.envelope.sequence = 3;
    let expected = returned.clone();
    let ticket = f.broker.reserve_completion(&returned.envelope).unwrap();
    f.broker.dispatch_completion(ticket, returned).unwrap();
    assert_eq!(f.node.try_recv().unwrap(), expected);
}

#[test]
fn queue_ticket_rechecks_duplicate_and_conflict_without_a_second_delivery() {
    for conflicting in [false, true] {
        let mut f = fixture(2);
        let value = input(&f, "interleaved", Endpoint::node(f.own.clone(), "n1", 1), 1);
        let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
        let mut accepted = value.clone();
        if conflicting {
            accepted.payload.push(9);
        }
        f.broker.dispatch(accepted.clone()).unwrap();
        if conflicting {
            refuse(
                &f.broker,
                ticket,
                value,
                DispatchError::ConflictingDuplicate,
            );
        } else {
            let before = ledger_snapshot(&f.broker);
            assert_eq!(
                f.broker.dispatch_completion(ticket, value),
                Ok(DispatchOutcome::Duplicate)
            );
            assert_eq!(ledger_snapshot(&f.broker), before);
        }
        assert_eq!(f.node.try_recv().unwrap(), accepted);
        assert!(
            f.node.try_recv().is_err(),
            "the ticket cannot duplicate delivery"
        );
        assert_eq!(
            f.broker
                .nodes
                .read()
                .unwrap()
                .get("n1")
                .unwrap()
                .sender
                .capacity(),
            2,
            "both the consumed delivery and unused reservation are released"
        );
    }
}

#[test]
fn completion_commit_moves_original_to_every_destination_and_dedupe_is_independent() {
    let mut f = fixture(1);
    let targets = [
        Endpoint::agent(f.own.clone()),
        Endpoint::node(f.own.clone(), "n1", 1),
        Endpoint::outer(f.own.clone(), "outer", 1),
        Endpoint::node(f.remote.clone(), "n2", 1),
    ];
    for (index, target) in targets.into_iter().enumerate() {
        let value = input(&f, &format!("original-{index}"), target, index as u64 + 1);
        let expected = value.clone();
        let storage = allocations(&value);
        let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
        assert!(matches!(
            f.broker.dispatch_completion(ticket, value),
            Ok(DispatchOutcome::Enqueued(_))
        ));
        let mut delivered = match index {
            0 => f.agent.try_recv().unwrap(),
            1 => f.node.try_recv().unwrap(),
            2 => f.outer.try_recv().unwrap(),
            3 => f.outbound.try_recv().unwrap(),
            _ => unreachable!(),
        };
        assert_eq!(delivered, expected);
        assert_eq!(allocations(&delivered), storage);
        delivered.payload[0] ^= 1;
        let ticket = f.broker.reserve_completion(&delivered.envelope).unwrap();
        let returned = refuse(
            &f.broker,
            ticket,
            delivered,
            DispatchError::ConflictingDuplicate,
        );
        assert_eq!(allocations(&returned), storage);
        let ticket = f.broker.reserve_completion(&expected.envelope).unwrap();
        assert_eq!(
            f.broker.dispatch_completion(ticket, expected),
            Ok(DispatchOutcome::Duplicate)
        );
    }
    for receiver in [&mut f.agent, &mut f.node, &mut f.outer, &mut f.outbound] {
        assert!(receiver.try_recv().is_err());
    }
}
