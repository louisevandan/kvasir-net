use super::Flavour;

#[test]
fn a_name_round_trips() {
    for flavour in [Flavour::LlamaCpp, Flavour::Vllm, Flavour::Sglang] {
        assert_eq!(Flavour::parse(flavour.name()), Some(flavour));
    }
}

#[test]
fn a_name_nobody_registered_is_not_a_flavour() {
    assert_eq!(Flavour::parse("tensorrt"), None);
    assert_eq!(Flavour::parse(""), None);
}

/// The one difference that reaches the wire, and only one backend has it.
///
/// vLLM matches the request's `model` against what it serves and answers 404
/// otherwise. Making every backend ask anyway would be simpler and would be
/// wrong: it charges every deployment a round trip for one deployment's rule,
/// and it hides the difference instead of stating it.
#[test]
fn only_vllm_insists_on_being_told_what_it_serves() {
    assert!(Flavour::Vllm.insists_on_the_model_name());
    assert!(!Flavour::LlamaCpp.insists_on_the_model_name());
    assert!(!Flavour::Sglang.insists_on_the_model_name());
}
