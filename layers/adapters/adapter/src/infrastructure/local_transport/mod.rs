//! Agent-owned locality annotation for native Pipeline transport.
//! See `apps/p4/docs/internals.md#agent-local-native-transport`.

use serde_json::{Map, Value};

pub(crate) fn apply_agent_local_ipc_domain(start: &mut Map<String, Value>, agent_endpoint: &str) {
    let domain = format!(
        "p4-agent-{}",
        agent_endpoint
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>()
    );
    let Some(nodes) = start.get_mut("nodes").and_then(Value::as_array_mut) else {
        return;
    };
    for node in nodes {
        let Some(node) = node.as_object_mut() else {
            continue;
        };
        if node.get("kind").and_then(Value::as_str) == Some("local") {
            node.insert("ipc_domain_id".into(), Value::String(domain.clone()));
        }
    }
}

#[cfg(test)]
mod tests;
