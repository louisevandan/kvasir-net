use super::*;

#[test]
fn connection_admission_is_not_the_inference_parallel_limit() {
    assert_eq!(DEFAULT_MAX_CONNECTIONS, 4096);
    assert_eq!(configured_limit("P4_AGENT_TEST_MISSING", 4), 4);
}

#[test]
fn workers_default_to_twice_physical_cores_and_allow_an_override() {
    assert_eq!(resolve_worker_count(None, 28), 56);
    assert_eq!(resolve_worker_count(Some(40), 28), 40);
    assert_eq!(resolve_worker_count(None, 0), 2);
}
