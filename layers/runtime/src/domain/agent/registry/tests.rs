use super::adapter::Adapter;
use super::node::NodeSlot;
use super::{DanglingAdapter, Registry};
use crate::foundation::transport::tcp;

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
    registry.nodes.insert(
        "node-a".into(),
        NodeSlot::new("controller-a".into(), "adapter-a".into(), 1),
    );
    registry
}

#[test]
fn resolving_joins_a_slot_with_its_current_adapter() {
    let registry = registry();
    let resolved = registry.resolve("node-a").unwrap().unwrap();
    assert_eq!(resolved.slot.controller_id, "controller-a");
    assert_eq!(resolved.adapter.endpoint.as_deref(), Some("127.0.0.1:19203"));
}

#[test]
fn resolving_reads_the_adapter_live_so_a_re_registration_is_observed() {
    let mut registry = registry();
    registry.adapters.insert(
        "adapter-a".into(),
        Adapter {
            kind: "test".into(),
            transport: tcp("127.0.0.1:19999"),
            endpoint: Some("127.0.0.1:19999".into()),
            descriptor: "{}".into(),
        },
    );
    let resolved = registry.resolve("node-a").unwrap().unwrap();
    assert_eq!(
        resolved.adapter.endpoint.as_deref(),
        Some("127.0.0.1:19999"),
        "the slot must not cache a stale adapter handle"
    );
}

#[test]
fn an_unknown_node_resolves_to_nothing() {
    assert!(registry().resolve("node-missing").is_none());
}

#[test]
fn a_slot_outliving_its_adapter_is_reported_as_dangling() {
    let mut registry = registry();
    registry.adapters.remove("adapter-a");
    let dangling = registry.resolve("node-a").unwrap().unwrap_err();
    assert_eq!(
        dangling,
        DanglingAdapter {
            node_id: "node-a".into(),
            adapter_id: "adapter-a".into(),
        }
    );
    assert!(dangling.detail().contains("no longer registered"));
}

#[test]
fn attached_nodes_counts_only_slots_of_that_adapter() {
    let mut registry = registry();
    registry.nodes.insert(
        "node-b".into(),
        NodeSlot::new("controller-a".into(), "adapter-a".into(), 1),
    );
    registry.nodes.insert(
        "node-c".into(),
        NodeSlot::new("controller-a".into(), "adapter-b".into(), 1),
    );
    assert_eq!(registry.attached_nodes("adapter-a"), 2);
    assert_eq!(registry.attached_nodes("adapter-b"), 1);
    assert_eq!(registry.attached_nodes("adapter-z"), 0);
}
