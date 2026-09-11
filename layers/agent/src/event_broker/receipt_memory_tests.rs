//! Consume both real dispatch paths, including pinned exact duplicates.
use super::*;
use p4_adapter::node_adapter::retained_event_bytes;

fn payload_event(f: &Fixture, id: &str, sequence: u64) -> Event {
    let mut value = event(
        id,
        Endpoint::agent(f.remote.clone()),
        Endpoint::agent(f.own.clone()),
        sequence,
    );
    value.payload = Vec::with_capacity(16_384);
    value.payload.extend_from_slice(&[0, 255, 128, 7]);
    value.envelope.event_id.reserve(2048);
    value
}

#[test]
fn receipt_memory_measures_independent_copies_on_both_dispatch_paths() {
    let mut f = fixture(1);
    let mut expected_bytes = 0;
    for (i, class) in [
        EventClass::Data,
        EventClass::Output,
        EventClass::Control,
        EventClass::Telemetry,
    ]
    .into_iter()
    .enumerate()
    {
        let mut value = payload_event(&f, &format!("owned-{i}"), i as u64 + 1);
        value.envelope.class = class;
        let original_capacity = value.payload.capacity();
        let original_ptr = value.payload.as_ptr();
        let copy = value.clone();
        let cost = retained_event_bytes(&copy).unwrap();
        assert!(cost < retained_event_bytes(&value).unwrap());
        expected_bytes += cost;
        if i % 2 == 0 {
            f.broker.dispatch(value).unwrap();
        } else {
            let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
            f.broker.dispatch_completion(ticket, value).unwrap();
        }
        let mut delivered = f.agent.try_recv().unwrap();
        assert_eq!(delivered.payload.as_ptr(), original_ptr);
        assert_eq!(delivered.payload.capacity(), original_capacity);
        let before_dequeue = f.broker.receipt_snapshot().unwrap();
        assert_eq!(before_dequeue.indexed.events, i + 1);
        assert_eq!(before_dequeue.indexed.event_bytes, Some(expected_bytes));
        assert_eq!(before_dequeue.allocated, before_dequeue.indexed);
        assert_eq!(
            before_dequeue.indexed.payload_capacity_bytes,
            Some((i + 1) * 4)
        );
        delivered.payload[0] ^= 42;
        let rejected = f.broker.dispatch(delivered).unwrap_err();
        assert_eq!(rejected.error, DispatchError::ConflictingDuplicate);
        drop(rejected);
        assert_eq!(f.broker.dispatch(copy).unwrap(), DispatchOutcome::Duplicate);
        assert_eq!(
            f.broker.receipt_snapshot().unwrap(),
            before_dequeue,
            "receiver mutation/drop and duplicate inspection do not free or allocate a receipt"
        );
    }
}

#[test]
fn receipt_memory_retired_bytes_survive_until_the_last_real_front_ticket() {
    let mut f = fixture(1);
    // Same production constructor, a one-entry count window for forced eviction.
    f.broker = EventBroker::new(
        f.own.clone(),
        f.broker.agent.clone(),
        f.broker.outer.clone(),
        f.broker.outbound.clone(),
        1,
    );
    let first = payload_event(&f, "pinned", 1);
    let first_cost = retained_event_bytes(&first.clone()).unwrap();
    f.broker.dispatch(first.clone()).unwrap();
    let exact = f.broker.reserve_completion(&first.envelope).unwrap();
    let conflict = f.broker.reserve_completion(&first.envelope).unwrap();
    drop(f.agent.try_recv().unwrap());
    let mut second = payload_event(&f, "replacement", 2);
    second.payload = vec![3; 8192];
    let second_cost = retained_event_bytes(&second.clone()).unwrap();
    f.broker.dispatch(second.clone()).unwrap();
    let evicted = f.broker.receipt_snapshot().unwrap();
    assert_eq!(evicted.indexed.events, 1);
    assert_eq!(evicted.indexed.event_bytes, Some(second_cost));
    assert_eq!(evicted.retired.events, 1);
    assert_eq!(evicted.retired.event_bytes, Some(first_cost));
    assert_eq!(
        evicted.allocated.event_bytes,
        Some(first_cost + second_cost)
    );
    assert_eq!(
        evicted.peak_allocated_event_bytes,
        Some(first_cost + second_cost)
    );
    assert_eq!(evicted.committed_events, Some(2));
    assert_eq!(evicted.evicted_events, Some(1));
    assert_eq!(evicted.freed_events, Some(0));
    assert_eq!(
        f.broker.dispatch_completion(exact, first.clone()).unwrap(),
        DispatchOutcome::Duplicate
    );
    assert_eq!(
        f.broker.receipt_snapshot().unwrap(),
        evicted,
        "another ticket still pins the exact Event"
    );
    let mut changed = first;
    changed.payload[0] ^= 1;
    let failure = f.broker.dispatch_completion(conflict, changed).unwrap_err();
    assert_eq!(failure.error, DispatchError::ConflictingDuplicate);
    let freed = f.broker.receipt_snapshot().unwrap();
    assert_eq!(freed.retired.events, 0);
    assert_eq!(freed.retired.event_bytes, Some(0));
    assert_eq!(freed.allocated.event_bytes, Some(second_cost));
    assert_eq!(freed.freed_events, Some(1));
    assert_eq!(
        freed.peak_allocated_event_bytes,
        evicted.peak_allocated_event_bytes
    );
    assert_eq!(f.agent.try_recv().unwrap(), second);
    assert!(
        f.agent.try_recv().is_err(),
        "pinned duplicates never re-deliver an evicted event"
    );
}

#[test]
fn receipt_memory_refusal_and_cancel_do_not_consume_storage_or_identity() {
    let mut f = fixture(1);
    let first = payload_event(&f, "full", 1);
    f.broker.dispatch(first).unwrap();
    let baseline = f.broker.receipt_snapshot().unwrap();
    let second = payload_event(&f, "retry", 2);
    let ptr = second.payload.as_ptr();
    let failed = f.broker.dispatch(second).unwrap_err();
    assert_eq!(failed.error, DispatchError::Full(Delivery::Agent));
    assert_eq!(failed.event.payload.as_ptr(), ptr);
    assert_eq!(f.broker.receipt_snapshot().unwrap(), baseline);
    assert!(matches!(
        f.broker.reserve_completion(&failed.event.envelope),
        Err(DispatchError::Full(_))
    ));
    drop(f.agent.try_recv().unwrap());
    let ticket = f.broker.reserve_completion(&failed.event.envelope).unwrap();
    drop(ticket);
    assert_eq!(f.broker.receipt_snapshot().unwrap(), baseline);
    f.broker.dispatch(*failed.event).unwrap();
    assert_eq!(
        f.broker.receipt_snapshot().unwrap().committed_events,
        Some(2)
    );
    assert_eq!(f.agent.try_recv().unwrap().payload.as_ptr(), ptr);
}

#[test]
fn receipt_memory_tracks_actual_window_rotation_without_retaining_evicted_values() {
    let mut f = fixture(1);
    let mut retained = std::collections::VecDeque::new();
    let mut peak = 0;
    for i in 0..128 {
        let mut value = payload_event(&f, &format!("rotation-{i}"), i + 1);
        value.payload = vec![(i % 256) as u8; 65_536];
        let cost = retained_event_bytes(&value.clone()).unwrap();
        retained.push_back(cost);
        peak = peak.max(retained.iter().sum::<usize>());
        if i % 2 == 0 {
            f.broker.dispatch(value).unwrap();
        } else {
            let ticket = f.broker.reserve_completion(&value.envelope).unwrap();
            f.broker.dispatch_completion(ticket, value).unwrap();
        }
        drop(f.agent.try_recv().unwrap());
        if retained.len() > 8 {
            retained.pop_front();
        }
        let snapshot = f.broker.receipt_snapshot().unwrap();
        assert_eq!(snapshot.allocated.events, retained.len());
        assert_eq!(snapshot.allocated.event_bytes, Some(retained.iter().sum()));
        assert_eq!(snapshot.retired.events, 0);
        assert_eq!(snapshot.peak_allocated_event_bytes, Some(peak));
        assert_eq!(snapshot.committed_events, Some(i as usize + 1));
        assert_eq!(
            snapshot.evicted_events,
            Some((i as usize + 1).saturating_sub(8))
        );
        assert_eq!(snapshot.freed_events, snapshot.evicted_events);
    }
}
