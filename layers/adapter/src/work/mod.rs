//! The unit of work a node hands to its adapter.
//!
//! Three shapes, because the workload has three: materialise this node's share
//! of a distributed model, release it, or run one hop. Everything the system
//! actually does is built from those.

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
}

/// How a backend spreads a model, and therefore what a node may be asked to do.
///
/// This is the axis the interface generalises over. Every backend here loads a
/// model across more than one device; they differ only in who owns the
/// boundary between the pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Distribution {
    /// The backend spreads the model itself and presents one entry point.
    /// vLLM and SGLang work this way — their tensor and pipeline parallelism
    /// is coordinated inside the backend, so the pieces are not separately
    /// addressable and a chain over them is one node long.
    Internal,
    /// The boundary is ours. A node holds a layer range and a chain of nodes
    /// spans the model, which is how the llama.cpp pipeline runtime is driven.
    Staged,
}

impl Distribution {
    /// Whether this adapter can be one link of a chain rather than the whole
    /// of it. An `Internal` backend cannot: asking it to be a middle stage
    /// would mean addressing pieces it does not expose.
    pub fn can_be_a_stage(self) -> bool {
        matches!(self, Self::Staged)
    }
}

/// This node's share of a distributed model.
///
/// A model is split across devices, but how is not stated here: a node does
/// not know its own placement, and the plan that does is opaque text the
/// adapter interprets. What the node knows is that a load is in progress and
/// that it will hear about it per stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Load {
    pub deployment: DeploymentId,
    /// Opaque to everything above the adapter. Carries the placement, the
    /// declared concurrency ceiling, and whatever else the backend needs.
    pub plan: String,
    /// What the adapter should materialise, named the way the plan names it.
    pub artifact: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unload {
    pub deployment: DeploymentId,
}

/// One pass of execution over a batch of sequences.
///
/// The batch is the point. A cohort window is the shape the runtime actually
/// works in, so a hop that could only carry one sequence would force the node
/// to simulate batching above the boundary, which is exactly where measurement
/// showed a throttle does not belong.
///
/// A hop means the same thing under either distribution. On a `Staged` backend
/// it advances this node's layer range and the next node continues; on an
/// `Internal` one it advances the whole model. Neither case puts hidden state
/// in this interface — that transfer stays inside the backend.
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
