//! The standard service on top of the agent core.
//!
//! The core routes opaque bodies; this gives them meaning. It is the layer a
//! deployment actually runs: a vocabulary for the bodies, the duties an agent
//! owns, the reading a node does, and the registry a concrete adapter is
//! attached to.
//!
//! Attaching llama.cpp or vLLM stops here. Register a factory under a name,
//! implement `p4_adapter::Adapter`, and nothing else in the system changes.

pub mod cache;
pub mod cache_journal;
pub mod capability;
pub mod duties;
pub mod machine;
pub mod message;
pub mod outer_policy;
pub mod payload;
pub mod registry;
pub mod status;

pub use capability::CapabilityRegistry;
pub use duties::Standard;
pub use payload::Bodies;
pub use registry::Registry;
