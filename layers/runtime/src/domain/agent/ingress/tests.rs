use super::*;

#[test]
fn issued_sessions_are_controller_scoped_and_monotonic() {
    let agent = AgentProcessor::new();
    assert!(agent.issue_session("controller-a").ends_with("-session-1"));
    assert!(agent.issue_session("controller-b").ends_with("-session-2"));
}
