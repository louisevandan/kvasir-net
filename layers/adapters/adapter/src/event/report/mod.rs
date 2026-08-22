//! What an adapter reports.
//!
//! The vocabulary of reporting, which grows as observability needs grow. Kept
//! apart from the sink so adding a variant never touches how events travel.

use crate::work::{CacheReceiptState, DeploymentId, SequenceId};

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A load moved. Distributed loads report per stage, because a model
    /// spread over layer ranges finishes when its slowest piece does, and a
    /// single overall figure would hide which piece is slow — which is exactly
    /// how a non-final stage reserving the whole model went unseen.
    LoadProgress {
        deployment: DeploymentId,
        stage: u32,
        percent: u32,
        detail: String,
    },
    /// The deployment is executable. Carries the generation, the one
    /// identifier an adapter issues rather than receives, because it names a
    /// materialisation that only the adapter witnessed.
    Loaded {
        deployment: DeploymentId,
        generation: u64,
        /// What was reserved, in the adapter's own categories. Nothing above
        /// reads the names.
        allocations: Vec<Allocation>,
    },
    Unloaded {
        deployment: DeploymentId,
    },
    /// A hop ended. This is the event the node waits on, and the only moment
    /// at which it reconsiders its queue, its deadlines and its cancellations.
    HopComplete {
        hop_id: u64,
        deployment: DeploymentId,
        /// The exact sequence set the adapter accepted for this hop. The
        /// node uses it to reject partial, duplicate, or cross-deployment
        /// completions before consuming its in-flight map.
        expected: Vec<SequenceId>,
        outcomes: Vec<Outcome>,
    },
    /// A staged backend has materialised a native sequence slot for a route.
    /// The node uses this only for admission; ordinary adapters need not emit
    /// it and therefore retain their existing scheduling behaviour.
    SequenceAcquired {
        deployment: DeploymentId,
        sequence: SequenceId,
    },
    /// A staged backend has released the native sequence/KV slot for a route.
    /// This is emitted before the corresponding HopComplete so a waiting
    /// prefill can be admitted without racing the backend release.
    SequenceReleased {
        deployment: DeploymentId,
        sequence: SequenceId,
    },
    /// A `Work::Close` instruction finished. Raised whether or not the
    /// adapter actually held anything for `sequence` -- an adapter that never
    /// reserved it, or already released it, still owes this so the node's
    /// single lifecycle slot is freed. See `Work::Close` for why this exists
    /// apart from `SequenceReleased`: that event is scoped to an in-flight
    /// hop's own fence and would be rejected as orphaned if raised from a
    /// `Close` dispatch, which runs alone the way `Load`/`Unload`/`Cache` do.
    Closed {
        deployment: DeploymentId,
        sequence: SequenceId,
    },
    /// A cache instruction finished.
    ///
    /// `bytes` is what the durable copy occupies — nought after a discard, and
    /// after a restore what was read back. A KV cache is large enough that an
    /// operator persisting thousands of conversations needs the number, and
    /// the backend is the only thing that knows it.
    Cached {
        deployment: DeploymentId,
        stage_id: String,
        generation: u64,
        operation_id: String,
        /// The id the state now lives under: the new one after a fork.
        sequence: SequenceId,
        bytes: u64,
        detail: String,
    },
    /// A read-only durable receipt query result. Unlike `Cached`, this never
    /// claims that a cache mutation was applied.
    CacheStatus {
        deployment: DeploymentId,
        stage_id: String,
        generation: u64,
        operation_id: String,
        sequence: SequenceId,
        state: CacheReceiptState,
        bytes: u64,
        detail: String,
    },
    /// The work could not be done. Terminal for whatever it names.
    Failed {
        deployment: DeploymentId,
        sequence: Option<SequenceId>,
        /// Present for an execution failure; absent for lifecycle/cache
        /// failures that have no adapter hop identity.
        hop_id: Option<u64>,
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub category: String,
    pub bytes: u64,
}

/// What one sequence got out of a hop.
///
/// See docs/adapter-boundary.md for what crosses the adapter boundary in
/// this shape and why almost nothing else does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub sequence: SequenceId,
    /// What this session needs to be carried one step further, as this
    /// adapter wrote it. It travels to the next node, or back to this one on
    /// the next lap, and is handed back as `Sequence::state` untouched.
    ///
    /// `None` when the session ends here and there is nothing to carry.
    pub forward: Option<Vec<u8>>,
    /// Empty on a stage that is not where generation lands. Logits exist only
    /// at the end of a chain, so only the node holding that end produces text.
    pub text: String,
    /// Set when this sequence is finished and should not be scheduled again.
    pub stop: Option<String>,
    /// The request-level output count committed with a terminal outcome.
    ///
    /// This is deliberately not a backend position: an adapter may supply it
    /// only when it has proved a terminal request contract.  It lets a native
    /// length terminal account for decode steps that produced no visible text,
    /// without making P4's normal streaming tally depend on backend state.
    /// See `docs/adapter-boundary.md#terminal-length-accounting`.
    pub terminal_generated: Option<u32>,
}

impl Outcome {
    pub fn is_finished(&self) -> bool {
        self.stop.is_some()
    }
}

#[cfg(test)]
mod tests;
