//! NodeSlot execution credit.
//!
//! The slot semaphore is a reader/writer lock written as a counting
//! semaphore: an execution is a reader and takes one permit, a binding
//! lifecycle transition is a writer and takes every permit. Sizing
//! (`max_inflight`) therefore sets concurrency only; it never weakens the
//! exclusivity that keeps a binding from being replaced under a live stream.
//! See `apps/p4/docs/internals.md#execution-credit`.

use super::registry::node::NodeSlot;
use std::sync::Arc;
use tokio::sync::OwnedSemaphorePermit;

/// One execution's share of a slot. Held until the adapter emits a terminal
/// response.
pub(crate) fn execution(slot: &NodeSlot) -> Option<OwnedSemaphorePermit> {
    Arc::clone(&slot.admission).try_acquire_owned().ok()
}

/// Every share of a slot, so MODEL_LOAD and MODEL_UNLOAD cannot overlap any
/// execution. Acquiring all permits is what makes the binding stable until it
/// is explicitly unloaded.
pub(crate) fn lifecycle(slot: &NodeSlot) -> Option<OwnedSemaphorePermit> {
    Arc::clone(&slot.admission)
        .try_acquire_many_owned(slot.max_inflight)
        .ok()
}

pub(crate) fn saturated_detail(node_id: &str) -> String {
    format!("node {node_id} admission is full; retry after an active stream completes")
}

#[cfg(test)]
mod tests;
