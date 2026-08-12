use super::{Denial, execution, node_create, owned_node};
use crate::domain::agent::registry::Registry;
use crate::domain::agent::registry::adapter::Adapter;
use crate::domain::agent::registry::node::NodeSlot;
use crate::foundation::transport::tcp;
use p4_protocol::{ExecutionRequest, Phase};

fn registry() -> Registry {
    let mut registry = Registry::default();
    registry.adapters.insert(
        "adapter-a".into(),
        Adapter {
            kind: "test".into(),
            transport: tcp("127.0.0.1:19203"),
            endpoint: Some("127.0.0.1:19203".into()),
            descriptor: "{}".into(),
        },
    );
    let mut slot = NodeSlot::new("controller-a".into(), "adapter-a".into(), 1);
    slot.bind("binding-a".into(), "deployment-a".into(), 1);
    registry.nodes.insert("node-a".into(), slot);
    registry
}

fn request() -> ExecutionRequest {
    ExecutionRequest {
        controller_id: "controller-a".into(),
        node_id: "node-a".into(),
        deployment_id: "deployment-a".into(),
        binding_id: "binding-a".into(),
        runtime_generation: 1,
        request_id: "request-a".into(),
        session_id: "session-a".into(),
        phase: Phase::Prefill,
        position: 0,
        max_tokens: 8,
        temperature: 0.0,
        prompt: "hello".into(),
        options: "{}".into(),
    }
}

#[test]
fn the_owning_controller_resolves_its_slot() {
    let resolved = owned_node(&registry(), "controller-a", "node-a", "request-a").unwrap();
    assert_eq!(resolved.slot.adapter_id, "adapter-a");
}

#[test]
fn another_controller_cannot_reach_the_slot() {
    let denial = owned_node(&registry(), "controller-b", "node-a", "request-a").unwrap_err();
    assert_eq!(
        denial,
        Denial::ForeignController {
            node_id: "node-a".into(),
            controller_id: "controller-b".into(),
        }
    );
}

#[test]
fn an_uncreated_node_is_denied() {
    let denial = owned_node(&registry(), "controller-a", "node-z", "request-a").unwrap_err();
    assert!(denial.detail().contains("is not created"));
}

#[test]
fn execution_requires_the_recorded_deployment_and_generation() {
    let registry = registry();
    let resolved = owned_node(&registry, "controller-a", "node-a", "request-a").unwrap();
    assert!(execution(&resolved, &request()).is_ok());

    let mut stale = request();
    stale.runtime_generation = 0;
    assert!(matches!(
        execution(&resolved, &stale),
        Err(Denial::BindingNotReady { .. })
    ));

    let mut foreign = request();
    foreign.deployment_id = "deployment-b".into();
    assert!(matches!(
        execution(&resolved, &foreign),
        Err(Denial::BindingNotReady { .. })
    ));
}

#[test]
fn node_create_is_idempotent_for_the_same_owner_and_adapter() {
    assert!(node_create(&registry(), "controller-a", "node-a", "adapter-a").is_ok());
}

#[test]
fn node_create_refuses_a_foreign_controller_and_a_moved_adapter() {
    let mut registry = registry();
    registry.adapters.insert(
        "adapter-b".into(),
        Adapter {
            kind: "test".into(),
            transport: tcp("127.0.0.1:19204"),
            endpoint: Some("127.0.0.1:19204".into()),
            descriptor: "{}".into(),
        },
    );
    assert!(matches!(
        node_create(&registry, "controller-b", "node-a", "adapter-a"),
        Err(Denial::ForeignController { .. })
    ));
    assert!(matches!(
        node_create(&registry, "controller-a", "node-a", "adapter-b"),
        Err(Denial::ForeignAdapter { .. })
    ));
}

#[test]
fn node_create_requires_a_registered_adapter() {
    assert!(matches!(
        node_create(&registry(), "controller-a", "node-new", "adapter-z"),
        Err(Denial::UnknownAdapter { .. })
    ));
}

#[test]
fn a_slot_whose_adapter_vanished_is_denied_rather_than_routed() {
    let mut registry = registry();
    registry.adapters.remove("adapter-a");
    assert!(matches!(
        owned_node(&registry, "controller-a", "node-a", "request-a"),
        Err(Denial::Dangling(_))
    ));
}
