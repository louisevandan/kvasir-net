//! Materialising and releasing this node's share of a model.
//!
//! Lifecycle, which changes on its own schedule — a new backend brings new
//! plan keys, and none of that should reach the execution path.

use crate::work::DeploymentId;

/// This node's share of a distributed model.
///
/// A model is split across devices, but how is not stated here: a node does
/// not know its own placement, and the plan that does is opaque text the
/// adapter interprets. What the node knows is that a load is in progress and
/// that it will hear about it per stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Load {
    pub deployment: DeploymentId,
    /// Opaque above the adapter. Carries the placement, the declared
    /// concurrency ceiling, and whatever else the backend needs.
    pub plan: String,
    /// What the adapter should materialise, named the way the plan names it.
    pub artifact: String,
    /// Discovery snapshot selected by OUTER. The adapter may use this to
    /// reject a plan whose capability evidence is not the one it inspected.
    pub capability_snapshot_id: String,
    pub capability_expires_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unload {
    pub deployment: DeploymentId,
}
