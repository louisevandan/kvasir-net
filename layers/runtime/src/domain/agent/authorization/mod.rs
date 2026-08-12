//! The Agent rejection gate.
//!
//! Every controller-facing operation passes through here before it reaches an
//! adapter. Concrete adapters re-check binding generation, but none of them
//! key on `controller_id`, so ownership is enforced only at this boundary and
//! must hold until the binding is unloaded.

use super::registry::{DanglingAdapter, Registry, ResolvedNode};
use p4_protocol::ExecutionRequest;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Denial {
    UnknownNode {
        node_id: String,
        correlation_id: String,
    },
    ForeignController {
        node_id: String,
        controller_id: String,
    },
    ForeignAdapter {
        node_id: String,
    },
    UnknownAdapter {
        adapter_id: String,
    },
    Dangling(DanglingAdapter),
    BindingNotReady {
        node_id: String,
        binding_id: String,
        generation: u64,
    },
}

impl Denial {
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::UnknownNode {
                node_id,
                correlation_id,
            } => format!("node instance {node_id} is not created; request {correlation_id}"),
            Self::ForeignController {
                node_id,
                controller_id,
            } => format!("node instance {node_id} is not owned by controller {controller_id}"),
            Self::ForeignAdapter { node_id } => {
                format!("node {node_id} is already attached to another adapter")
            }
            Self::UnknownAdapter { adapter_id } => {
                format!("adapter {adapter_id} is not registered")
            }
            Self::Dangling(dangling) => dangling.detail(),
            Self::BindingNotReady {
                node_id,
                binding_id,
                generation,
            } => format!(
                "node {node_id} has no ready binding {binding_id} generation {generation}"
            ),
        }
    }
}

/// Resolves a slot only for the controller that owns it. Used by EXECUTE,
/// MODEL_LOAD, MODEL_UNLOAD and HEALTH_CHECK alike so that ownership cannot be
/// bypassed by choosing a different operation.
pub(crate) fn owned_node(
    registry: &Registry,
    controller_id: &str,
    node_id: &str,
    correlation_id: &str,
) -> Result<ResolvedNode, Denial> {
    let resolved = registry
        .resolve(node_id)
        .ok_or_else(|| Denial::UnknownNode {
            node_id: node_id.into(),
            correlation_id: correlation_id.into(),
        })?
        .map_err(Denial::Dangling)?;
    if resolved.slot.controller_id != controller_id {
        return Err(Denial::ForeignController {
            node_id: node_id.into(),
            controller_id: controller_id.into(),
        });
    }
    Ok(resolved)
}

/// A slot may execute only the exact deployment and runtime generation it
/// recorded at MODEL_BOUND. A reload bumps the generation, so an in-flight
/// controller cannot address the replacement runtime with a stale identity.
pub(crate) fn execution(
    resolved: &ResolvedNode,
    request: &ExecutionRequest,
) -> Result<(), Denial> {
    if resolved.slot.binding_is_ready(
        &request.binding_id,
        &request.deployment_id,
        request.runtime_generation,
    ) {
        return Ok(());
    }
    Err(Denial::BindingNotReady {
        node_id: request.node_id.clone(),
        binding_id: request.binding_id.clone(),
        generation: request.runtime_generation,
    })
}

/// NODE_CREATE is idempotent for its owner but never re-points an existing
/// slot: neither a different controller nor a different adapter may claim it.
pub(crate) fn node_create(
    registry: &Registry,
    controller_id: &str,
    node_id: &str,
    adapter_id: &str,
) -> Result<(), Denial> {
    if !registry.adapters.contains_key(adapter_id) {
        return Err(Denial::UnknownAdapter {
            adapter_id: adapter_id.into(),
        });
    }
    let Some(existing) = registry.nodes.get(node_id) else {
        return Ok(());
    };
    if existing.controller_id != controller_id {
        return Err(Denial::ForeignController {
            node_id: node_id.into(),
            controller_id: controller_id.into(),
        });
    }
    if existing.adapter_id != adapter_id {
        return Err(Denial::ForeignAdapter {
            node_id: node_id.into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
