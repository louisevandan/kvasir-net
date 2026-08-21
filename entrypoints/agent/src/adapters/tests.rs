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
    assert!(!registry.knows("tensorrt"));
    assert!(registry.build("tensorrt", "stage-0").is_none());
}

/// The three OpenAI-compatible servers are carried, and each is the
/// self-contained shape: one process holds the model, so a chain over it is
/// one link whatever the node is called.
///
/// All three, not just the one there is hardware for here. They are the same
/// implementation because they answer the same HTTP, and a registration that
/// was never built is the kind of compatibility claim that turns out to be
/// false the first time somebody types the name.
#[test]
fn the_openai_compatible_backends_are_attached_and_spread_the_model_themselves() {
    let registry = registry();
    for backend in ["llamacpp", "vllm", "sglang"] {
        assert!(registry.knows(backend), "{backend} is registered");
        for node in ["stage-0", "tail-1", "solo"] {
            assert_eq!(
                registry.build(backend, node).unwrap().distribution(),
                Distribution::Internal,
                "{backend} on {node} is one link"
            );
        }
    }
}

#[test]
fn a_position_is_read_from_the_node_name() {
    assert_eq!(stage_of("stage-0"), Some(0));
    assert_eq!(stage_of("stage-12"), Some(12));
    assert_eq!(stage_of("solo"), None);
    assert_eq!(stage_of("stage-x"), None);
}

#[test]
fn a_present_stage_server_builds_the_concrete_staged_adapter() {
    let binary = std::env::current_exe().expect("test executable path");
    let mut registry = p4_service::Registry::new();
    register_staged(&mut registry, binary, None, 0, 0);

    let adapter = registry
        .build("llamacpp-staged", "stage-0")
        .expect("present stage server is attachable");
    assert_eq!(adapter.distribution(), Distribution::Staged);
    assert_eq!(
        adapter.report(),
        "staged lifecycle=Empty\nP4_STAGED_TOMBSTONE_REJECTED_V1 count=0\nP4_STAGED_TOMBSTONE_EVICTED_V1 count=0\nP4_RUNTIME_EVIDENCE_V1 retained=0 dropped=0\n"
    );
}
