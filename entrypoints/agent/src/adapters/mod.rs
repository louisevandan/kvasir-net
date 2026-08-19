//! Where backends are attached to this build.
//!
//! This file is the whole of what adding llama.cpp or vLLM costs. Each is a
//! name, a factory, and an implementation of `p4_adapter::Adapter`. Nothing
//! above this line — envelope, queue, worker, node, chain — changes for any of
//! them, which is the property the communication layer was built to have.

use p4_llamacpp_served::Served;
use p4_llamacpp_served::flavour::Flavour;
use p4_llamacpp_staged_adapter::{StagedAdapter, StagedConfig};
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_service::Registry;
use std::net::SocketAddr;
use std::path::PathBuf;
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

    // Expose staged only when this host has the prepared server artifact. A
    // missing artifact is a missing capability, not a runtime fallback.
    if let Some(binary) = std::env::var_os("P4_STAGED_SERVER_BINARY") {
        let binary = PathBuf::from(binary);
        let model_identity = std::env::var("P4_STAGED_MODEL_IDENTITY").ok();
        let layer_begin = std::env::var("P4_STAGED_LAYER_BEGIN")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let layer_end = std::env::var("P4_STAGED_LAYER_END")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        register_staged(
            &mut registry,
            binary,
            model_identity,
            layer_begin,
            layer_end,
        );
    }

    registry
}

fn register_staged(
    registry: &mut Registry,
    binary: PathBuf,
    model_identity: Option<String>,
    layer_begin: i32,
    layer_end: i32,
) {
    registry.register(
        "llamacpp-staged",
        Arc::new(move |_| {
            if !binary.is_file() {
                return None;
            }
            let endpoint: SocketAddr = "127.0.0.1:0".parse().expect("valid staged endpoint");
            let mut config = StagedConfig::new(binary.clone(), endpoint);
            if let Some(identity) = &model_identity
                && layer_end > layer_begin
            {
                config = config.with_kv_metadata(identity.clone(), layer_begin, layer_end);
            }
            Some(Arc::new(StagedAdapter::new(config)) as Arc<dyn p4_adapter::Adapter>)
        }),
    );
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
