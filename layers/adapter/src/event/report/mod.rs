//! What an adapter reports.
//!
//! The vocabulary of reporting, which grows as observability needs grow. Kept
//! apart from the sink so adding a variant never touches how events travel.

use crate::work::{DeploymentId, SequenceId};

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
        deployment: DeploymentId,
        outcomes: Vec<Outcome>,
    },
    /// The work could not be done. Terminal for whatever it names.
    Failed {
        deployment: DeploymentId,
        sequence: Option<SequenceId>,
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub category: String,
    pub bytes: u64,
}

/// What one sequence got out of a hop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub sequence: SequenceId,
    /// Empty on a stage that is not where generation lands. Logits exist only
    /// at the end of a chain, so only the node holding that end produces text.
    pub text: String,
    pub position: u32,
    /// Set when this sequence is finished and should not be scheduled again.
    pub stop: Option<String>,
}

impl Outcome {
    pub fn is_finished(&self) -> bool {
        self.stop.is_some()
    }
}

#[cfg(test)]
mod tests;
