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
pub mod model;
pub mod work;

pub use event::{Allocation, Event, EventSink, Outcome};
pub use work::{
    Cache, CacheAction, CacheReceiptState, Close, DeploymentId, Distribution, Hop, Load, Sequence,
    SequenceId, Unload, Work,
};

/// What a node drives.
///
/// Every method is a procedure. Results arrive at the `EventSink`, never as a
/// return value, so no caller can be written to wait — which is the property
/// that keeps a hop's duration out of the agent's workers.
///
/// See docs/adapter-boundary.md for what crosses this trait's boundary and
/// why almost nothing does.
pub trait Adapter: Send + Sync {
    /// Discovers local model facts without changing the loaded deployment.
    /// The profile is opaque to P4 and is interpreted by OUTER and this
    /// adapter's owner. A backend that cannot inspect the reference refuses it
    /// explicitly instead of returning a guessed profile.
    fn inspect_model(&self, _artifact: &str) -> Result<String, String> {
        Err("model inspection is unsupported".into())
    }

    /// How this backend spreads a model. Fixed for the adapter's lifetime and
    /// read by whoever composes chains, not by the node.
    fn distribution(&self) -> Distribution;

    /// Whether the adapter owns a finite native sequence table that must be
    /// reserved before a staged prefill is started. The node uses this to
    /// close the admission race between launching a hop and receiving the
    /// adapter's SequenceAcquired event.
    fn reserves_sequence_slots(&self) -> bool {
        false
    }

    /// Begins work. Returns immediately, having at most enqueued it.
    ///
    /// A hop must not be started while another is running for the same
    /// deployment; the node guarantees that by starting the next hop only when
    /// it sees the previous one complete.
    fn start(&self, work: Work, events: &dyn EventSink);

    /// What this backend is doing, for whoever is asking.
    ///
    /// The mirror of a plan. A plan goes down opaque — the layer above carries
    /// it and never reads it — and this comes up the same way: the node puts it
    /// in a status snapshot and never interprets a byte of it. That symmetry is
    /// what lets a backend be observable without the core learning what a
    /// backend is.
    ///
    /// It exists because every defect found under load in this layer was
    /// diagnosed by reading a backend's own logs and the machine's socket
    /// table, neither of which an operator elsewhere can see. What is cheap to
    /// publish should be published; what needs a debugger should not be here.
    ///
    /// Called on a status request, so it must be cheap and must not block. An
    /// adapter with nothing to say says nothing, which is the default.
    fn report(&self) -> String {
        String::new()
    }
}
