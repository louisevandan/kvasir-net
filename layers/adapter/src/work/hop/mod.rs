//! One pass of execution over a window of sequences.
//!
//! The execution shape, which is the part under pressure from throughput work
//! and therefore kept away from lifecycle.

use crate::work::{DeploymentId, SequenceId};

/// The batch is the point. A cohort window is the shape the runtime actually
/// works in, so a hop that could only carry one sequence would force the node
/// to simulate batching above the boundary, which is exactly where measurement
/// showed a throttle does not belong.
///
/// A hop means the same thing under either distribution. On a staged backend
/// it advances this node's layer range and the next node continues; on an
/// internal one it advances the whole model. Neither case puts hidden state in
/// this interface — that transfer stays inside the backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hop {
    pub deployment: DeploymentId,
    pub phase: Phase,
    /// The window this hop covers. Its size is the node's decision, bounded by
    /// what the load declared.
    pub sequences: Vec<Sequence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The first pass over a prompt.
    Prefill,
    /// One further step. On a staged chain a lap of the ring produces one
    /// token per sequence; on an internal backend the hop does that itself.
    Decode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequence {
    pub sequence: SequenceId,
    /// Position reached so far. A decode hop continues from here.
    pub position: u32,
    /// Present when this node begins the work — the chain's first node, or the
    /// only node on an internal backend. Absent on a later stage, which
    /// continues from state it already holds rather than from text.
    pub prompt: Option<String>,
    /// How many tokens this sequence still wants. Zero ends it.
    pub remaining: u32,
    /// Opaque sampling and generation options. Passed through whole; an
    /// adapter that cannot honour a key must say so rather than drop it.
    pub options: String,
}

impl Hop {
    /// A hop covering nothing is not a hop. Callers check this rather than
    /// discovering it inside a backend.
    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty()
    }

    pub fn width(&self) -> usize {
        self.sequences.len()
    }
}

#[cfg(test)]
mod tests;
