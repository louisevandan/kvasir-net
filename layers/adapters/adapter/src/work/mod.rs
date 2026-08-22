//! The unit of work a node hands to its adapter.
//!
//! Four shapes, because the workload has four: materialise this node's share
//! of a distributed model, release it, run one hop, or let go of a sequence a
//! hop will not name again. Each lives in its own folder because each changes
//! for its own reason — a new backend kind touches `distribution`, a new plan
//! key touches `load`, throughput work touches `hop`, and admission
//! bookkeeping touches `close`. This file holds only what all four share.

pub mod cache;
pub mod close;
pub mod distribution;
pub mod hop;
pub mod load;

pub use cache::{Cache, CacheAction, CacheReceiptState};
pub use close::Close;
pub use distribution::Distribution;
pub use hop::{Hop, Sequence};
pub use load::{Load, Unload};

/// A deployment's identity as far as the adapter is concerned. The adapter
/// never invents one; it is told.
pub type DeploymentId = String;

/// One sequence's identity for the life of a request. KV belongs to the node
/// that holds it, so this is how a hop says which state to continue.
pub type SequenceId = String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Work {
    Load(Load),
    Unload(Unload),
    Hop(Hop),
    /// One instruction about one sequence's cached state: persist it and free
    /// the memory, bring it back, branch it, or delete the durable copy.
    Cache(Cache),
    /// A sequence at this node will not be hopped to again. Idempotent: an
    /// adapter that never reserved anything for `sequence`, or already let it
    /// go, treats this as a no-op success.
    Close(Close),
}
