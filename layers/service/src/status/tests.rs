use super::*;

#[test]
fn a_name_that_would_break_the_snapshot_is_escaped() {
    // A node id and a route both come from a caller. Left alone, one of them
    // could make a snapshot claim a node that is not there.
    assert_eq!(escape("n0\nnode=fake"), "n0_node=fake");
    assert_eq!(escape("r,1"), "r_1");
    assert_eq!(escape("[boxed]"), "_boxed_");
    assert_eq!(escape("ordinary-route-7"), "ordinary-route-7");
}

/// `From<QueueClass> for ActiveHopLane`, pinned per variant.
///
/// The only production caller is `typed_snapshot()`, which is exercised
/// end to end (through a real wire encode/decode) in
/// `layers/service/tests/active_hop_lane.rs`. These four are the narrow,
/// fast check on the mapping itself: every existing test that touches
/// `ActiveHopLane` before this one built an `ActiveHopSnapshot` directly and
/// so never ran this conversion at all -- inverting it (`Prefill ->
/// Decode`) previously passed the entire workspace suite.
#[test]
fn queue_class_decode_narrows_to_active_hop_lane_decode() {
    assert_eq!(
        ActiveHopLane::from(QueueClass::Decode),
        ActiveHopLane::Decode
    );
}

#[test]
fn queue_class_prefill_narrows_to_active_hop_lane_prefill() {
    assert_eq!(
        ActiveHopLane::from(QueueClass::Prefill),
        ActiveHopLane::Prefill
    );
}

#[test]
fn queue_class_control_narrows_to_active_hop_lane_prefill() {
    // Not reachable from a real active hop today, but the conversion is
    // total by construction (see its doc comment) and this pins the value
    // it must produce rather than leaving the not-Decode arm unchecked.
    assert_eq!(
        ActiveHopLane::from(QueueClass::Control),
        ActiveHopLane::Prefill
    );
}

#[test]
fn queue_class_response_narrows_to_active_hop_lane_prefill() {
    assert_eq!(
        ActiveHopLane::from(QueueClass::Response),
        ActiveHopLane::Prefill
    );
}
