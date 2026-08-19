//! Doing something with a sequence's KV cache other than continuing it.
//!
//! A request's identity already means something below the boundary: the
//! backend keeps that sequence's attention state against it, and a hop says
//! "continue this one". What was missing is every other verb. An agent that
//! holds a conversation open for hours cannot keep its state resident the
//! whole time, cannot resume it after the process that served it went away,
//! and cannot branch it to explore two continuations — all three of which are
//! ordinary now and none of which the protocol could express.
//!
//! These are deliberately about *one sequence*, never a window. A load is one
//! instruction about a whole deployment; these are one instruction about one
//! request, and both are alike in the way that matters to a node: they run
//! alone rather than batched.
//!
//! What "persist" means is the backend's business, as everything below the
//! boundary is. llama.cpp writes sequence state to a file; a server backend
//! may hand it to its own store. Nothing above cares, which is why the plan
//! and the location are not here.

use crate::work::{DeploymentId, SequenceId};

/// One instruction about one sequence's cached state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cache {
    pub deployment: DeploymentId,
    /// The concrete stage/node that owns this shard of the cache.
    pub stage_id: String,
    /// The deployment generation this operation was issued against. A cache
    /// record must never be restored into a rebound deployment silently.
    pub generation: u64,
    /// Immutable request identity for one multi-stage cache operation.
    pub operation_id: String,
    /// The request whose state this is about.
    pub sequence: SequenceId,
    pub action: CacheAction,
}

/// Durable state observed for one cache operation during reconciliation.
/// This is deliberately separate from the requested action: a coordinator
/// may ask an adapter what survived a restart without replaying a mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheReceiptState {
    Absent,
    Prepared,
    Committed,
    Aborted,
    /// A receipt exists, but its durable manifest is missing, corrupt, or
    /// does not match the receipt identity. Coordinators must stop rather
    /// than treating this as a replayable success.
    Inconsistent,
}

impl CacheReceiptState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Prepared => "prepared",
            Self::Committed => "committed",
            Self::Aborted => "aborted",
            Self::Inconsistent => "inconsistent",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheAction {
    /// Query durable receipt state without changing resident or durable KV.
    Reconcile,
    /// Stage a persist without releasing resident state. Commit or abort is
    /// required before the staged mutation becomes visible.
    PreparePersist,
    /// Write the state somewhere durable and give the memory back.
    ///
    /// One verb rather than two, because a persist that left the state
    /// resident would not free anything, and freeing without persisting is
    /// what already happens when a request ends. The point of the operation is
    /// that the memory goes and the state does not.
    Persist,
    /// Stage a restore while leaving the durable copy and resident state
    /// untouched. Commit or abort is required.
    PrepareRestore,
    /// Bring it back into memory under the same id, so the next hop continues
    /// where it left off.
    Restore,
    /// Stage a discard without deleting the durable copy. Commit or abort is
    /// required.
    PrepareDiscard,
    /// Copy it to a new id, leaving the original as it was.
    ///
    /// The branch case. Both continuations then have their own state and their
    /// own future, and neither can disturb the other — which is only true
    /// because this copies rather than aliases, however tempting a shared
    /// prefix looks.
    Fork { into: SequenceId },
    /// Throw the durable copy away.
    ///
    /// Not in the original three, and necessary: persisted state that nothing
    /// ever deletes is a disk filling up on a schedule nobody set. A protocol
    /// that can only create is a protocol with a leak in it.
    Discard,
    /// Apply a previously prepared cache mutation for this operation.
    Commit,
    /// Forget a previously prepared cache mutation and restore its pre-state.
    Abort,
}

impl Cache {
    /// The id this instruction will leave state under, which is the new one
    /// for a fork and the original otherwise.
    ///
    /// A caller reporting what happened needs this, and deriving it at each
    /// call site is how the fork case ends up reported against the wrong id.
    pub fn subject(&self) -> &SequenceId {
        match &self.action {
            CacheAction::Fork { into } => into,
            _ => &self.sequence,
        }
    }
}

#[cfg(test)]
mod tests;
