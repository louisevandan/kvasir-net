//! The canonical dispatch failure, not a parallel test-only routing path.
use super::*;

fn ledger_snapshot(broker: &EventBroker) -> String {
    format!(
        "{:?}",
        *broker
            .ledger
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    )
}

fn incoming(f: &Fixture, name: &str) -> Event {
    let mut input = event(
        name,
        Endpoint::agent(f.remote.clone()),
        Endpoint::node(f.own.clone(), "n1", 1),
        1,
    );
    input.payload = Vec::with_capacity(8192);
    input.payload.extend_from_slice(&[0, 255, 128, 7]);
    input.envelope.event_id.reserve(1024);
    input
}

fn refuse(broker: &EventBroker, input: Event, reason: DispatchError) -> Event {
    let before = ledger_snapshot(broker);
    let expected = input.clone();
    let allocation = (
        input.payload.as_ptr(),
        input.payload.capacity(),
        input.envelope.event_id.as_ptr(),
        input.envelope.event_id.capacity(),
    );
    let footprint = p4_adapter::node_adapter::retained_event_bytes(&input).unwrap();
    let failure = broker
        .dispatch(input)
        .expect_err("this input must be refused");
    assert_eq!(failure.error, reason);
    assert_eq!(
        *failure.event, expected,
        "refusal returns the whole original envelope/body"
    );
    assert_eq!(
        (
            failure.event.payload.as_ptr(),
            failure.event.payload.capacity(),
            failure.event.envelope.event_id.as_ptr(),
            failure.event.envelope.event_id.capacity(),
        ),
        allocation,
        "returning a cloned substitute loses the original allocation"
    );
    assert_eq!(
        p4_adapter::node_adapter::retained_event_bytes(&failure.event).unwrap(),
        footprint
    );
    assert_eq!(
        ledger_snapshot(broker),
        before,
        "no refusal may commit a receipt or sequence"
    );
    assert_eq!(
        failure.to_string(),
        reason.to_string(),
        "operational errors do not print payloads"
    );
    *failure.event
}

#[test]
fn invalid_envelope_returns_its_original_storage_and_corrected_input_is_accepted() {
    let mut f = fixture(1);
    let mut input = incoming(&f, "invalid-envelope");
    input.envelope.protocol_version = 0;
    let reason = DispatchError::Invalid(input.validate().unwrap_err().to_string());
    let mut returned = refuse(&f.broker, input, reason);
    assert!(f.node.try_recv().is_err());
    returned.envelope.protocol_version = Envelope::VERSION;
    let expected = returned.clone();
    assert_eq!(
        f.broker.dispatch(returned),
        Ok(DispatchOutcome::Enqueued(Delivery::Node {
            node: "n1".into(),
            generation: 1,
        }))
    );
    assert_eq!(f.node.try_recv().unwrap(), expected);
    assert_eq!(f.broker.dispatch(expected), Ok(DispatchOutcome::Duplicate));
}

#[test]
fn missing_stale_and_closed_routes_all_return_the_original_event_without_recording_it() {
    let mut f = fixture(1);
    let mut missing = incoming(&f, "missing");
    missing.envelope.target = Endpoint::node(f.own.clone(), "missing", 1);
    let returned = refuse(
        &f.broker,
        missing,
        DispatchError::UnknownNode("missing".into()),
    );
    let (sender, mut receiver) = bounded_queue(1);
    f.broker.register_node("missing", 1, sender).unwrap();
    let expected = returned.clone();
    f.broker.dispatch(returned).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), expected);

    let mut stale = incoming(&f, "stale");
    stale.envelope.correlation_id = "stale-order".into();
    stale.envelope.target = Endpoint::node(f.own.clone(), "n1", 2);
    let mut returned = refuse(
        &f.broker,
        stale,
        DispatchError::StaleNode {
            node: "n1".into(),
            current_generation: 1,
            incoming_generation: 2,
        },
    );
    assert!(f.node.try_recv().is_err());
    returned.envelope.target = Endpoint::node(f.own.clone(), "n1", 1);
    let expected = returned.clone();
    f.broker.dispatch(returned).unwrap();
    assert_eq!(f.node.try_recv().unwrap(), expected);

    f.node.close();
    let mut closed = incoming(&f, "closed");
    closed.envelope.correlation_id = "closed-order".into();
    let returned = refuse(
        &f.broker,
        closed,
        DispatchError::Closed(Delivery::Node {
            node: "n1".into(),
            generation: 1,
        }),
    );
    // Closed is terminal for this route, not a wait-until-space promise.
    let again = refuse(
        &f.broker,
        returned,
        DispatchError::Closed(Delivery::Node {
            node: "n1".into(),
            generation: 1,
        }),
    );
    assert_eq!(again.envelope.event_id, "closed");
    assert!(f.node.try_recv().is_err());
}

#[test]
fn identity_refusals_return_original_bytes_and_leave_exact_duplicate_receipts_unchanged() {
    let mut f = fixture(2);
    let mut committed = incoming(&f, "already-accepted");
    committed.envelope.sequence = 4;
    f.broker.dispatch(committed.clone()).unwrap();
    let mut conflict = incoming(&f, "already-accepted");
    conflict.envelope.sequence = 4;
    conflict.payload.push(9);
    let returned = refuse(&f.broker, conflict, DispatchError::ConflictingDuplicate);
    assert_eq!(returned.payload.last(), Some(&9));
    assert_eq!(
        f.broker.dispatch(committed.clone()),
        Ok(DispatchOutcome::Duplicate)
    );
    assert_eq!(f.node.try_recv().unwrap(), committed);
    assert!(f.node.try_recv().is_err());

    let regressing = incoming(&f, "regressing");
    let mut returned = refuse(
        &f.broker,
        regressing,
        DispatchError::SequenceRegression {
            previous: 4,
            incoming: 1,
        },
    );
    returned.envelope.sequence = 5;
    let expected = returned.clone();
    f.broker.dispatch(returned).unwrap();
    assert_eq!(f.node.try_recv().unwrap(), expected);
    assert!(f.node.try_recv().is_err());
}

#[test]
fn poisoned_locks_refuse_without_consuming_the_event_or_rewriting_the_ledger() {
    for ledger_lock in [false, true] {
        let mut f = fixture(1);
        let injected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if ledger_lock {
                let _guard = f.broker.ledger.lock().unwrap();
                panic!("test-only ledger poison before dispatch");
            } else {
                let _guard = f.broker.nodes.write().unwrap();
                panic!("test-only route poison before dispatch");
            }
        }));
        assert!(injected.is_err());
        let returned = refuse(&f.broker, incoming(&f, "poisoned"), DispatchError::Poisoned);
        assert!(f.node.try_recv().is_err());
        // Test-only cause correction: no production poison recovery is added.
        f.broker.ledger.clear_poison();
        f.broker.nodes.clear_poison();
        let expected = returned.clone();
        f.broker.dispatch(returned).unwrap();
        assert_eq!(f.node.try_recv().unwrap(), expected);
    }
}
