use super::{CapacityRegistry, declared_max_sequences};

fn plan(max_sequences: u64) -> String {
    format!(r#"{{"load_options":{{"batching":{{"max_sequences":{max_sequences}}}}}}}"#)
}

#[test]
fn a_declared_capacity_is_read_from_the_stage_plan() {
    assert_eq!(declared_max_sequences(&plan(50)), Some(50));
    assert_eq!(declared_max_sequences(&plan(1)), Some(1));
}

#[test]
fn malformed_or_absent_declarations_are_ignored() {
    assert_eq!(declared_max_sequences("{}"), None);
    assert_eq!(declared_max_sequences("not json"), None);
    assert_eq!(declared_max_sequences(r#"{"load_options":{}}"#), None);
    assert_eq!(declared_max_sequences(&plan(0)), None);
    assert_eq!(
        declared_max_sequences(r#"{"load_options":{"batching":{"max_sequences":"50"}}}"#),
        None
    );
}

#[test]
fn an_undeclared_deployment_falls_back_rather_than_blocking() {
    let registry = CapacityRegistry::new(16, 256);
    assert_eq!(registry.capacity("deployment-unknown"), 16);
    assert_eq!(registry.gate("deployment-unknown").available_permits(), 16);
}

#[test]
fn a_declared_deployment_sizes_its_own_gate_without_an_opt_in() {
    let registry = CapacityRegistry::new(16, 256);
    assert_eq!(registry.declare("deployment-a", &plan(50)), 50);
    assert_eq!(registry.capacity("deployment-a"), 50);
    assert_eq!(registry.gate("deployment-a").available_permits(), 50);
}

#[test]
fn deployments_do_not_share_a_gate() {
    let registry = CapacityRegistry::new(16, 256);
    registry.declare("deployment-a", &plan(50));
    registry.declare("deployment-b", &plan(4));
    assert_eq!(registry.gate("deployment-a").available_permits(), 50);
    assert_eq!(registry.gate("deployment-b").available_permits(), 4);
}

#[test]
fn the_adapter_ceiling_bounds_a_controller_request() {
    let registry = CapacityRegistry::new(16, 64);
    assert_eq!(registry.declare("deployment-a", &plan(1000)), 64);
    assert_eq!(registry.gate("deployment-a").available_permits(), 64);
}

#[test]
fn a_malformed_declaration_uses_the_fallback_not_zero() {
    let registry = CapacityRegistry::new(16, 256);
    assert_eq!(registry.declare("deployment-a", "{}"), 16);
    assert_eq!(registry.gate("deployment-a").available_permits(), 16);
}

#[test]
fn reloading_a_deployment_replaces_its_gate() {
    let registry = CapacityRegistry::new(16, 256);
    registry.declare("deployment-a", &plan(4));
    let first = registry.gate("deployment-a");
    assert_eq!(first.available_permits(), 4);

    registry.declare("deployment-a", &plan(50));
    let second = registry.gate("deployment-a");
    assert_eq!(second.available_permits(), 50);
    assert!(
        !std::sync::Arc::ptr_eq(&first, &second),
        "a reload must not keep serving the old capacity"
    );
}

#[test]
fn unloading_a_deployment_forgets_its_capacity() {
    let registry = CapacityRegistry::new(16, 256);
    registry.declare("deployment-a", &plan(50));
    registry.forget("deployment-a");
    assert_eq!(registry.capacity("deployment-a"), 16);
}

#[test]
fn the_same_deployment_reuses_one_gate() {
    let registry = CapacityRegistry::new(16, 256);
    registry.declare("deployment-a", &plan(8));
    assert!(std::sync::Arc::ptr_eq(
        &registry.gate("deployment-a"),
        &registry.gate("deployment-a")
    ));
}
