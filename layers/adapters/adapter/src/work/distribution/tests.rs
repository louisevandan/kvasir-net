use super::*;

#[test]
fn only_a_staged_backend_can_be_one_link_of_a_chain() {
    // vLLM and SGLang coordinate their own parallelism and expose one entry
    // point, so a chain over them is one node long. Asking either to be a
    // middle stage would mean addressing pieces it does not publish.
    assert!(Distribution::Staged.can_be_a_stage());
    assert!(!Distribution::Internal.can_be_a_stage());
}
