//! Where backends are attached to this build.
//!
//! This file is the whole of what adding llama.cpp or vLLM costs. Each is a
//! name, a factory, and an implementation of `p4_adapter::Adapter`. Nothing
//! above this line — envelope, queue, worker, node, chain — changes for any of
//! them, which is the property the communication layer was built to have.

use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_service::Registry;
use std::sync::Arc;
use std::time::Duration;

/// Every backend this build can serve.
///
/// The mock is always here: it is what lets a fleet be loaded without hardware,
/// and it is the second implementation that keeps the interface honest.
pub fn registry() -> Registry {
    let mut registry = Registry::new();

    // Reproduces the measured shape — cost belonging to a chain position,
    // prefill dearer than a lap — so a fleet run under it looks like the real
    // workload without needing a device.
    registry.register_fn("mock", |node| {
        build(node, Profile::measured_shape(Duration::from_millis(4)))
    });

    // Answers instantly. For proving routing and ordering at rates a timed
    // backend would hide.
    registry.register_fn("mock-instant", |node| build(node, Profile::default()));

    // ---------------------------------------------------------------------
    // Concrete backends go here. Each is one registration:
    //
    //   registry.register_fn("llamacpp", |node| Arc::new(LlamaCpp::new(node)));
    //   registry.register_fn("vllm", |node| Arc::new(Vllm::new(node)));
    //
    // A staged backend reads its chain position from the node name; an
    // internal one ignores it, since it spreads the model itself.
    // ---------------------------------------------------------------------

    registry
}

/// Reads a chain position out of a node named `stage-N`.
///
/// A staged backend has to know which position it plays, because cost belongs
/// to the position rather than to the device, and whether it holds the output
/// layer — only the end of a model produces a token. Both are load-time facts
/// for a real backend; here they come from the name.
///
/// `stage-N` is an intermediate stage, `tail-N` is the end of a chain, and
/// anything else is a backend that spreads the model itself and is therefore
/// its own end.
fn stage_of(node: &str) -> Option<usize> {
    node.strip_prefix("stage-")?.parse().ok()
}

fn tail_of(node: &str) -> Option<usize> {
    node.strip_prefix("tail-")?.parse().ok()
}

fn build(node: &str, profile: Profile) -> Arc<dyn p4_adapter::Adapter> {
    if let Some(position) = stage_of(node) {
        return Arc::new(Mock::staged(position, profile));
    }
    if let Some(position) = tail_of(node) {
        return Arc::new(Mock::terminal(position, profile));
    }
    Arc::new(Mock::internal(profile))
}

#[cfg(test)]
mod tests;
