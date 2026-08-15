use super::*;

#[test]
fn the_three_axes_are_independent() {
    // The recorded defect: one constant sized connection admission, request
    // admission and queue depth, so narrowing the release width narrowed all
    // three. Changing one here must not move the others.
    let narrowed = Budget {
        in_flight: 16,
        ..Budget::default()
    };
    assert_eq!(narrowed.in_flight, 16);
    assert_eq!(narrowed.connections, Budget::default().connections);
    assert_eq!(narrowed.depth, Budget::default().depth);
}

#[test]
fn a_zero_budget_is_refused_at_start_rather_than_clamped() {
    for budget in [
        Budget { connections: 0, ..Budget::default() },
        Budget { in_flight: 0, ..Budget::default() },
        Budget { depth: 0, ..Budget::default() },
    ] {
        assert!(budget.checked().is_err());
    }
    assert!(Budget::default().checked().is_ok());
}

#[test]
fn decode_is_the_deepest_lane() {
    // A queued decode lap belongs to a request already holding KV across its
    // whole chain, so refusing it throws away more work than refusing a new
    // arrival does.
    let lanes = Lanes::default();
    assert!(lanes.depth(QueueClass::Decode) > lanes.depth(QueueClass::Prefill));
    assert!(lanes.depth(QueueClass::Decode) > lanes.depth(QueueClass::Control));
}

#[test]
fn every_lane_has_its_own_depth() {
    let lanes = Lanes {
        control: 1,
        prefill: 2,
        decode: 3,
        response: 4,
    };
    assert_eq!(lanes.depth(QueueClass::Control), 1);
    assert_eq!(lanes.depth(QueueClass::Prefill), 2);
    assert_eq!(lanes.depth(QueueClass::Decode), 3);
    assert_eq!(lanes.depth(QueueClass::Response), 4);
}
