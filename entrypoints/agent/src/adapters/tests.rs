use super::*;
use p4_adapter::Distribution;

#[test]
fn the_build_ships_with_a_backend_that_needs_no_hardware() {
    // What lets a fleet be loaded anywhere, and the second implementation that
    // keeps the adapter interface honest.
    let registry = registry();
    assert!(registry.knows("mock"));
    assert!(registry.knows("mock-instant"));
}

#[test]
fn a_stage_named_node_builds_a_staged_backend() {
    let registry = registry();
    assert_eq!(
        registry.build("mock", "stage-0").unwrap().distribution(),
        Distribution::Staged
    );
    assert_eq!(
        registry.build("mock", "stage-3").unwrap().distribution(),
        Distribution::Staged
    );
}

#[test]
fn any_other_name_builds_a_backend_that_spreads_the_model_itself() {
    // The vLLM and SGLang shape: one addressable node for the whole model.
    assert_eq!(
        registry().build("mock", "solo").unwrap().distribution(),
        Distribution::Internal
    );
}

#[test]
fn a_backend_this_build_does_not_carry_is_reported_missing() {
    // Naming a backend this build has no implementation for is a placement
    // mistake, and the caller is the only one who can fix it — so it is
    // refused rather than falling back to whatever happens to be present.
    let registry = registry();
    assert!(!registry.knows("vllm"));
    assert!(registry.build("vllm", "stage-0").is_none());
}

/// llama.cpp is carried, and is the self-contained shape: one process holds
/// the model, so a chain over it is one link whatever the node is called.
#[test]
fn llamacpp_is_attached_and_spreads_the_model_itself() {
    let registry = registry();
    assert!(registry.knows("llamacpp"));
    for node in ["stage-0", "tail-1", "solo"] {
        assert_eq!(
            registry.build("llamacpp", node).unwrap().distribution(),
            Distribution::Internal,
            "{node} is one link"
        );
    }
}

#[test]
fn a_position_is_read_from_the_node_name() {
    assert_eq!(stage_of("stage-0"), Some(0));
    assert_eq!(stage_of("stage-12"), Some(12));
    assert_eq!(stage_of("solo"), None);
    assert_eq!(stage_of("stage-x"), None);
}
