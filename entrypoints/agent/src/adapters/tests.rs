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
    // Until llamacpp is registered here, naming it is a placement mistake
    // rather than a silent fallback to whatever happens to be present.
    let registry = registry();
    assert!(!registry.knows("llamacpp"));
    assert!(registry.build("llamacpp", "stage-0").is_none());
}

#[test]
fn a_position_is_read_from_the_node_name() {
    assert_eq!(stage_of("stage-0"), Some(0));
    assert_eq!(stage_of("stage-12"), Some(12));
    assert_eq!(stage_of("solo"), None);
    assert_eq!(stage_of("stage-x"), None);
}
