//! How a backend spreads a model, and therefore what a node may be asked to do.
//!
//! This is the axis the interface generalises over, and it changes only when a
//! new *kind* of backend appears — not when a backend is added. vLLM and
//! SGLang joined without touching this file because both are `Internal`.

/// Every backend here loads a model across more than one device. They differ
/// only in who owns the boundary between the pieces.
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

#[cfg(test)]
mod tests;
