use super::*;

fn item(route: &str, lane: QueueClass) -> Waiting {
    Waiting {
        route: route.into(),
        lane,
        deadline_unix_ms: 0,
    }
}

#[test]
fn nothing_waiting_composes_no_hop() {
    assert_eq!(compose(&[], 8, 0), None);
}

#[test]
fn a_window_is_bounded_by_the_declared_ceiling() {
    // The ceiling is a ceiling. Measurement showed concurrency above it is not
    // safe on the native runtime, so nothing here may exceed it.
    let waiting: Vec<Waiting> = (0..30)
        .map(|i| item(&format!("r{i}"), QueueClass::Prefill))
        .collect();
    let window = compose(&waiting, 10, 0).expect("a window");
    assert_eq!(window.width(), 10);
}

#[test]
fn a_ceiling_of_zero_admits_nothing() {
    assert_eq!(compose(&[item("r", QueueClass::Prefill)], 0, 0), None);
}

#[test]
fn a_ready_decode_lap_goes_before_fresh_prefill_once_the_deployment_is_full() {
    // The lap belongs to a request already holding KV on every node of its
    // chain; prefill is the long phase and has not started. This holds only
    // when there is no room, which is when it matters.
    let waiting = vec![
        item("prefill-1", QueueClass::Prefill),
        item("prefill-2", QueueClass::Prefill),
        item("decode-1", QueueClass::Decode),
        item("decode-2", QueueClass::Decode),
    ];
    let window = compose(&waiting, 2, 0).expect("a window");
    assert_eq!(window.lane, QueueClass::Decode);
    assert_eq!(window.width(), 2);
}

/// The other half of the same rule, and the one that was missing.
///
/// One sequence decoding always has a lap ready. Preferring it unconditionally
/// meant a burst of arrivals behind it never got in, so a node never reached
/// the width it had declared and both cards sat mostly idle. While there is
/// room, new work goes first.
#[test]
fn fresh_prefill_goes_first_while_the_deployment_has_room() {
    let waiting = vec![
        item("decode-1", QueueClass::Decode),
        item("prefill-1", QueueClass::Prefill),
        item("prefill-2", QueueClass::Prefill),
    ];
    let window = compose(&waiting, 8, 0).expect("a window");
    assert_eq!(window.lane, QueueClass::Prefill);
    assert_eq!(window.width(), 2);
}

/// Admission is not a trickle. A node that let one in per window would reach
/// its ceiling no faster than the sequences it is already carrying complete.
#[test]
fn admission_fills_the_room_that_is_left_in_one_window() {
    let mut waiting = vec![item("decode-1", QueueClass::Decode)];
    for index in 0..40 {
        waiting.push(item(&format!("prefill-{index}"), QueueClass::Prefill));
    }
    let window = compose(&waiting, 16, 0).expect("a window");
    assert_eq!(window.lane, QueueClass::Prefill);
    assert_eq!(window.width(), 16, "a whole window, not one at a time");
}

#[test]
fn a_window_never_mixes_lanes() {
    // Prefill and decode are different passes and a backend batches them
    // separately, so a mixed window would misrepresent both. Held at the
    // ceiling, where decode wins, and there is prefill waiting to mix in.
    let waiting = vec![
        item("d1", QueueClass::Decode),
        item("p1", QueueClass::Prefill),
        item("d2", QueueClass::Decode),
    ];
    let window = compose(&waiting, 2, 0).expect("a window");
    assert!(window.items.iter().all(|i| i.lane == QueueClass::Decode));
    assert_eq!(window.width(), 2);
}

#[test]
fn prefill_runs_when_no_lap_is_ready() {
    let waiting = vec![
        item("p1", QueueClass::Prefill),
        item("p2", QueueClass::Prefill),
    ];
    let window = compose(&waiting, 8, 0).expect("a window");
    assert_eq!(window.lane, QueueClass::Prefill);
    assert_eq!(window.width(), 2);
}

#[test]
fn expired_work_is_left_out_of_the_window() {
    let mut stale = item("late", QueueClass::Prefill);
    stale.deadline_unix_ms = 100;
    let waiting = vec![stale, item("live", QueueClass::Prefill)];
    let window = compose(&waiting, 8, 200).expect("a window");
    assert_eq!(window.width(), 1);
    assert_eq!(window.items[0].route, "live");
}

#[test]
fn expired_work_is_reported_rather_than_dropped() {
    // Silently dropping it would leave a caller waiting for a terminal that
    // never comes, which is the failure a route leak looks like.
    let mut stale = item("late", QueueClass::Decode);
    stale.deadline_unix_ms = 100;
    let waiting = vec![stale, item("live", QueueClass::Decode)];
    let expired = expired_items(&waiting, 200);
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].route, "late");
}

#[test]
fn a_zero_deadline_never_expires() {
    let waiting = vec![item("forever", QueueClass::Prefill)];
    assert!(expired_items(&waiting, u64::MAX).is_empty());
    assert_eq!(compose(&waiting, 1, u64::MAX).unwrap().width(), 1);
}

#[test]
fn everything_expired_composes_no_hop() {
    let mut stale = item("late", QueueClass::Prefill);
    stale.deadline_unix_ms = 100;
    assert_eq!(compose(&[stale], 8, 200), None);
}
