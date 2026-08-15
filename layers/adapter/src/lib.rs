//! The interface a node uses to drive a concrete adapter.
//!
//! This crate exists because the contract used to be nothing but the P4
//! message set, which put the whole burden of backend neutrality on the wire
//! and let transformer vocabulary leak into it. Here the contract is an
//! artifact, and a second implementation — the mock — is what proves it holds.
//!
//! It generalises over backends that load a model across more than one device:
//! the llama.cpp pipeline runtime, where we own the stage boundary, and vLLM
//! and SGLang, which own theirs. It does not generalise beyond that, and it
//! names no backend.

pub mod event;
pub mod work;

pub use event::{Allocation, Event, EventSink, Outcome};
pub use work::{DeploymentId, Distribution, Hop, Load, Phase, Sequence, SequenceId, Unload, Work};

/// What a node drives.
///
/// Every method is a procedure. Results arrive at the `EventSink`, never as a
/// return value, so no caller can be written to wait — which is the property
/// that keeps a hop's duration out of the agent's workers.
pub trait Adapter: Send + Sync {
    /// How this backend spreads a model. Fixed for the adapter's lifetime and
    /// read by whoever composes chains, not by the node.
    fn distribution(&self) -> Distribution;

    /// Begins work. Returns immediately, having at most enqueued it.
    ///
    /// A hop must not be started while another is running for the same
    /// deployment; the node guarantees that by starting the next hop only when
    /// it sees the previous one complete.
    fn start(&self, work: Work, events: &dyn EventSink);
}
