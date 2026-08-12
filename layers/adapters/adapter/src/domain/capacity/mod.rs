//! Per-deployment execution capacity.
//!
//! The adapter used to gate every execution behind one process-wide credit
//! constant. That put the throttle three tiers upstream of the GPU: the native
//! scheduler would report `limit=50 capacity=50` while only 16 sequences ever
//! arrived. Capacity is a property of the deployment the controller loaded, so
//! it is read from `stage_plan.load_options.batching.max_sequences` and the
//! gate is sized from it.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::Semaphore;

pub(crate) struct CapacityRegistry {
    declared: RwLock<HashMap<String, usize>>,
    gates: Mutex<HashMap<String, Arc<Semaphore>>>,
    /// Used when a deployment declared nothing the adapter understands.
    fallback: usize,
    /// Adapter-owned upper bound; a controller cannot ask for more.
    ceiling: usize,
}

pub(crate) const DEFAULT_FALLBACK_CREDITS: usize = 16;
pub(crate) const DEFAULT_CEILING: usize = 256;

impl CapacityRegistry {
    /// One reader for every knob so the ceiling the listener advertises and
    /// the ceiling a deployment is clamped to cannot drift apart.
    ///
    /// The controller's declared limit is the deployment's gate. The native
    /// scheduler keeps a draining cohort in its normal physical batch, so the
    /// earlier conservative 16-credit fallback is only for malformed plans.
    pub(crate) fn from_env() -> Self {
        Self::new(
            bounded("P4_ADAPTER_PREFILL_CREDITS", DEFAULT_FALLBACK_CREDITS),
            bounded("P4_ADAPTER_MAX_INFLIGHT", DEFAULT_CEILING),
        )
    }

    pub(crate) fn new(fallback: usize, ceiling: usize) -> Self {
        Self {
            declared: RwLock::new(HashMap::new()),
            gates: Mutex::new(HashMap::new()),
            fallback: fallback.clamp(1, ceiling.max(1)),
            ceiling: ceiling.max(1),
        }
    }

    /// Records what the controller measured for this deployment. Called at
    /// MODEL_LOAD, before any execution can reference the binding.
    pub(crate) fn declare(&self, deployment_id: &str, stage_plan: &str) -> usize {
        let capacity = declared_max_sequences(stage_plan)
            .map(|value| value.clamp(1, self.ceiling))
            .unwrap_or(self.fallback);
        if let Ok(mut declared) = self.declared.write() {
            declared.insert(deployment_id.into(), capacity);
        }
        // A reload may change capacity, so the old gate must not survive it.
        if let Ok(mut gates) = self.gates.lock() {
            gates.remove(deployment_id);
        }
        capacity
    }

    pub(crate) fn forget(&self, deployment_id: &str) {
        if let Ok(mut declared) = self.declared.write() {
            declared.remove(deployment_id);
        }
        if let Ok(mut gates) = self.gates.lock() {
            gates.remove(deployment_id);
        }
    }

    pub(crate) fn fallback(&self) -> usize {
        self.fallback
    }

    pub(crate) fn ceiling(&self) -> usize {
        self.ceiling
    }

    pub(crate) fn capacity(&self, deployment_id: &str) -> usize {
        self.declared
            .read()
            .ok()
            .and_then(|declared| declared.get(deployment_id).copied())
            .unwrap_or(self.fallback)
    }

    /// The gate for one deployment, created on first use at its declared size.
    pub(crate) fn gate(&self, deployment_id: &str) -> Arc<Semaphore> {
        let capacity = self.capacity(deployment_id);
        let mut gates = match self.gates.lock() {
            Ok(gates) => gates,
            Err(poisoned) => poisoned.into_inner(),
        };
        Arc::clone(
            gates
                .entry(deployment_id.into())
                .or_insert_with(|| Arc::new(Semaphore::new(capacity))),
        )
    }
}

fn bounded(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=4096).contains(value))
        .unwrap_or(default)
}

/// `load_options.batching.max_sequences` is the controller's minimum across
/// every stage; the adapter serves the whole group, so that is the figure that
/// bounds it. Anything malformed is ignored rather than guessed at.
pub(crate) fn declared_max_sequences(stage_plan: &str) -> Option<usize> {
    serde_json::from_str::<Value>(stage_plan)
        .ok()?
        .pointer("/load_options/batching/max_sequences")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests;
