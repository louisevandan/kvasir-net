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
    /// Whether a declared capacity may widen the gate. Off by default; see
    /// `from_env`.
    derive: bool,
}

pub(crate) const DEFAULT_FALLBACK_CREDITS: usize = 16;
pub(crate) const DEFAULT_CEILING: usize = 256;

impl CapacityRegistry {
    /// One reader for every knob so the ceiling the listener advertises and
    /// the ceiling a deployment is clamped to cannot drift apart.
    ///
    /// Deriving the gate from the declared capacity is opt-in. Measured on
    /// Ornith-1.0-35B with 32 concurrent 1000-token requests, widening the
    /// gate from 16 to the declared 32 made the run 3.4x slower: 648.3s ->
    /// 2183.9s. Compute frames barely moved (42,327 -> 41,533) while each
    /// frame took about four times as long, because the native scheduler
    /// leaves the batched path as soon as one session finishes and then pays
    /// the full cohort-width graph cost for a single token — tokens per frame
    /// fell from 0.99 to 0.85. Until that scheduler guard is fixed, a wider
    /// gate is a regression, so the default stays conservative.
    pub(crate) fn from_env() -> Self {
        let mut registry = Self::new(
            bounded("P4_ADAPTER_PREFILL_CREDITS", DEFAULT_FALLBACK_CREDITS),
            bounded("P4_ADAPTER_MAX_INFLIGHT", DEFAULT_CEILING),
        );
        registry.derive = std::env::var("P4_ADAPTER_DERIVE_CAPACITY").as_deref() == Ok("1");
        registry
    }

    pub(crate) fn new(fallback: usize, ceiling: usize) -> Self {
        Self {
            declared: RwLock::new(HashMap::new()),
            gates: Mutex::new(HashMap::new()),
            fallback: fallback.clamp(1, ceiling.max(1)),
            ceiling: ceiling.max(1),
            derive: true,
        }
    }

    /// Records what the controller measured for this deployment. Called at
    /// MODEL_LOAD, before any execution can reference the binding.
    pub(crate) fn declare(&self, deployment_id: &str, stage_plan: &str) -> usize {
        let capacity = declared_max_sequences(stage_plan)
            .filter(|_| self.derive)
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
