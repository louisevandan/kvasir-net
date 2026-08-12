use super::apply_agent_local_ipc_domain;
use serde_json::json;

#[test]
fn local_nodes_share_the_owning_agent_memory_domain() {
    let mut start = json!({
        "nodes": [
            { "id": "gpu-a", "kind": "local", "ipc_domain_id": "controller-claim" },
            { "id": "gpu-b", "kind": "local" },
            { "id": "remote", "kind": "external", "ipc_domain_id": "remote-agent" }
        ]
    })
    .as_object()
    .cloned()
    .unwrap();

    apply_agent_local_ipc_domain(&mut start, "127.0.0.1:29017");

    assert_eq!(
        start["nodes"][0]["ipc_domain_id"],
        "p4-agent-127_0_0_1_29017"
    );
    assert_eq!(
        start["nodes"][1]["ipc_domain_id"],
        "p4-agent-127_0_0_1_29017"
    );
    assert_eq!(start["nodes"][2]["ipc_domain_id"], "remote-agent");
}
