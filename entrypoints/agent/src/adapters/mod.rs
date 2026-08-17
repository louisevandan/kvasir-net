//! Where backends are attached to this build.
//!
//! This file is the whole of what adding llama.cpp or vLLM costs. Each is a
//! name, a factory, and an implementation of `p4_adapter::Adapter`. Nothing
//! above this line — envelope, queue, worker, node, chain — changes for any of
//! them, which is the property the communication layer was built to have.

use p4_llamacpp_served::Served;
use p4_llamacpp_served::flavour::Flavour;
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

    // llama.cpp behind its OpenAI-compatible server: one process holding the
    // whole model, so a chain over it is one link and the node name says
    // nothing. Where the server is comes from the load's plan rather than from
    // here, which is why attaching a backend is this line and an `Adapter`
    // implementation — the claim the layer was built to make.
    //
    // Three names, one implementation, because the three servers answer the
    // same HTTP. Registered rather than asserted: a compatibility claim that
    // is never built is a claim nobody has checked, and the name an operator
    // types is what decides which of the small differences applies.
    for flavour in [Flavour::LlamaCpp, Flavour::Vllm, Flavour::Sglang] {
        registry.register_fn(flavour.name(), move |_| Arc::new(Served::new(flavour)));
    }

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
