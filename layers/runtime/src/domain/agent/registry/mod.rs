//! Agent-owned mutable state: which adapters are routable and which
//! NodeSlots exist. Resolution joins the two slices so a slot never carries
//! its own copy of a concrete runtime handle.

pub(crate) mod adapter;
pub(crate) mod node;

use adapter::Adapter;
use node::NodeSlot;
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct Registry {
    pub(crate) adapters: HashMap<String, Adapter>,
    pub(crate) nodes: HashMap<String, NodeSlot>,
}

/// A slot joined with the adapter that currently serves it. Both are cloned
/// so callers can release the registry lock before performing transport I/O.
#[derive(Debug)]
pub(crate) struct ResolvedNode {
    pub(crate) slot: NodeSlot,
    pub(crate) adapter: Adapter,
}

/// The adapter a slot names is gone. A slot outliving its adapter is a
/// registry inconsistency, not a caller error.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DanglingAdapter {
    pub(crate) node_id: String,
    pub(crate) adapter_id: String,
}

impl Registry {
    pub(crate) fn attached_nodes(&self, adapter_id: &str) -> usize {
        self.nodes
            .values()
            .filter(|slot| slot.adapter_id == adapter_id)
            .count()
    }

    /// Joins a slot with its adapter. The adapter handle is read here, never
    /// cached on the slot, so a re-registration cannot leave a slot pointing
    /// at a runtime the registry no longer advertises.
    pub(crate) fn resolve(&self, node_id: &str) -> Option<Result<ResolvedNode, DanglingAdapter>> {
        let slot = self.nodes.get(node_id)?;
        let Some(adapter) = self.adapters.get(&slot.adapter_id) else {
            return Some(Err(DanglingAdapter {
                node_id: node_id.into(),
                adapter_id: slot.adapter_id.clone(),
            }));
        };
        Some(Ok(ResolvedNode {
            slot: slot.clone(),
            adapter: adapter.clone(),
        }))
    }
}

impl DanglingAdapter {
    pub(crate) fn detail(&self) -> String {
        format!(
            "node {} names adapter {} which is no longer registered",
            self.node_id, self.adapter_id
        )
    }
}

#[cfg(test)]
mod tests;
