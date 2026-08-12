use super::{NodeSlot, UnbindRefusal};

fn slot() -> NodeSlot {
    NodeSlot::new("controller-a".into(), "adapter-a".into(), 2)
}

#[test]
fn a_new_slot_carries_capacity_and_no_binding() {
    let slot = slot();
    assert_eq!(slot.max_inflight, 2);
    assert_eq!(slot.admission.available_permits(), 2);
    assert!(slot.bindings.is_empty());
}

#[test]
fn a_binding_is_ready_only_for_its_own_deployment_and_generation() {
    let mut slot = slot();
    slot.bind("binding-a".into(), "deployment-a".into(), 3);

    assert!(slot.binding_is_ready("binding-a", "deployment-a", 3));
    assert!(!slot.binding_is_ready("binding-a", "deployment-a", 2));
    assert!(!slot.binding_is_ready("binding-a", "deployment-b", 3));
    assert!(!slot.binding_is_ready("binding-b", "deployment-a", 3));
}

#[test]
fn reloading_a_binding_replaces_its_generation() {
    let mut slot = slot();
    slot.bind("binding-a".into(), "deployment-a".into(), 1);
    slot.bind("binding-a".into(), "deployment-a".into(), 2);

    assert!(!slot.binding_is_ready("binding-a", "deployment-a", 1));
    assert!(slot.binding_is_ready("binding-a", "deployment-a", 2));
}

#[test]
fn unbinding_requires_the_recorded_deployment() {
    let mut slot = slot();
    slot.bind("binding-a".into(), "deployment-a".into(), 1);

    assert_eq!(
        slot.unbind("binding-a", "deployment-b"),
        Err(UnbindRefusal::DeploymentMismatch {
            recorded: "deployment-a".into()
        })
    );
    assert!(
        slot.binding_is_ready("binding-a", "deployment-a", 1),
        "a refused unbind must leave the live binding in place"
    );

    let removed = slot.unbind("binding-a", "deployment-a").unwrap();
    assert_eq!(removed.deployment_id, "deployment-a");
    assert!(slot.bindings.is_empty());
}

#[test]
fn unbinding_an_unknown_binding_is_refused() {
    let mut slot = slot();
    assert_eq!(
        slot.unbind("binding-a", "deployment-a"),
        Err(UnbindRefusal::UnknownBinding)
    );
}
