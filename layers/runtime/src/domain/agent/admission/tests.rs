use super::{execution, lifecycle};
use crate::domain::agent::registry::node::NodeSlot;

fn slot(max_inflight: u32) -> NodeSlot {
    NodeSlot::new("controller-a".into(), "adapter-a".into(), max_inflight)
}

#[test]
fn executions_share_a_slot_up_to_its_sizing() {
    let slot = slot(3);
    let held: Vec<_> = (0..3).filter_map(|_| execution(&slot)).collect();
    assert_eq!(held.len(), 3);
    assert!(
        execution(&slot).is_none(),
        "a fourth execution must be refused rather than queued"
    );
}

#[test]
fn a_lifecycle_transition_needs_every_permit() {
    let slot = slot(4);
    let held = execution(&slot).expect("one execution");
    assert!(
        lifecycle(&slot).is_none(),
        "a binding must not be replaced under a live stream"
    );
    drop(held);
    assert!(lifecycle(&slot).is_some());
}

#[test]
fn an_execution_cannot_start_during_a_lifecycle_transition() {
    let slot = slot(4);
    let held = lifecycle(&slot).expect("lifecycle permit");
    assert!(execution(&slot).is_none());
    drop(held);
    assert!(execution(&slot).is_some());
}

/// Raising concurrency must not weaken the guarantee the gate exists for.
#[test]
fn exclusivity_is_independent_of_sizing() {
    for max_inflight in [1, 8, 64, 1024] {
        let slot = slot(max_inflight);
        let held = execution(&slot).expect("one execution");
        assert!(
            lifecycle(&slot).is_none(),
            "sizing {max_inflight} must still block a lifecycle transition"
        );
        drop(held);
        assert!(
            lifecycle(&slot).is_some(),
            "sizing {max_inflight} must admit a lifecycle transition once drained"
        );
    }
}
