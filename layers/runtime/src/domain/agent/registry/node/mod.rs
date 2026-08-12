//! NodeSlot and its model bindings.
//!
//! A NodeSlot is model-independent: it records controller ownership, the
//! adapter responsible for it, and execution capacity. Concrete runtime
//! handles never live here; see `registry/adapter`.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// One versioned model binding on a slot. `generation` is issued by the
/// adapter and separates a reloaded runtime from the session that preceded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) deployment_id: String,
    pub(crate) generation: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct NodeSlot {
    pub(crate) controller_id: String,
    pub(crate) adapter_id: String,
    pub(crate) max_inflight: u32,
    pub(crate) admission: Arc<Semaphore>,
    pub(crate) bindings: HashMap<String, Binding>,
}

/// Why a binding removal was refused. Unbinding the wrong deployment would
/// silently drop a live binding, so the mismatch is reported rather than
/// applied.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnbindRefusal {
    UnknownBinding,
    DeploymentMismatch { recorded: String },
}

impl NodeSlot {
    pub(crate) fn new(controller_id: String, adapter_id: String, max_inflight: u32) -> Self {
        Self {
            controller_id,
            adapter_id,
            max_inflight,
            admission: Arc::new(Semaphore::new(max_inflight as usize)),
            bindings: HashMap::new(),
        }
    }

    /// True when the slot carries exactly this deployment and runtime
    /// generation for `binding_id`. A reloaded runtime bumps the generation,
    /// so a stale session cannot execute against its replacement.
    pub(crate) fn binding_is_ready(
        &self,
        binding_id: &str,
        deployment_id: &str,
        generation: u64,
    ) -> bool {
        matches!(
            self.bindings.get(binding_id),
            Some(binding)
                if binding.deployment_id == deployment_id && binding.generation == generation
        )
    }

    pub(crate) fn bind(&mut self, binding_id: String, deployment_id: String, generation: u64) {
        self.bindings.insert(
            binding_id,
            Binding {
                deployment_id,
                generation,
            },
        );
    }

    pub(crate) fn unbind(
        &mut self,
        binding_id: &str,
        deployment_id: &str,
    ) -> Result<Binding, UnbindRefusal> {
        let recorded = self
            .bindings
            .get(binding_id)
            .ok_or(UnbindRefusal::UnknownBinding)?;
        if recorded.deployment_id != deployment_id {
            return Err(UnbindRefusal::DeploymentMismatch {
                recorded: recorded.deployment_id.clone(),
            });
        }
        Ok(self
            .bindings
            .remove(binding_id)
            .expect("binding disappeared under an exclusive registry write"))
    }
}

#[cfg(test)]
mod tests;
