//! Backend-neutral model artifact inspection.
//!
//! This module reads only the GGUF container facts needed by an OUTER planner.
//! It does not choose placement, interpret generation options, or depend on
//! llama.cpp headers. A concrete adapter may call it from `inspect_model` and
//! return the JSON unchanged through the P4 wire.

mod gguf;
mod profile;

pub use gguf::inspect_artifact;
pub use profile::{ModelProfile, TensorProfile};
