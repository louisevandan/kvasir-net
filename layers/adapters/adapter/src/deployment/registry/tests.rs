use super::*;
use crate::deployment::SubmissionId;
use std::sync::Mutex;

#[derive(Default)]
struct FakeClient {
    submitted: Mutex<Vec<Submit>>,
    canceled: Mutex<Vec<SubmissionId>>,
    refuse: bool,
}

impl Client for FakeClient {
    fn try_submit(&self, submit: Submit) -> Result<(), EnqueueError> {
        if self.refuse {
            return Err(EnqueueError("closed".into()));
        }
        self.submitted.lock().expect("lock").push(submit);
        Ok(())
    }

    fn cancel(&self, submission_id: SubmissionId) {
        self.canceled.lock().expect("lock").push(submission_id);
    }
}

fn submit(deployment_id: &str, submission_id: &str) -> Submit {
    Submit {
        deployment_id: deployment_id.into(),
        deployment_generation: 1,
        submission_id: submission_id.into(),
        deadline_unix_ms: 0,
        request: serde_json::json!({}),
    }
}

#[test]
fn dispatches_try_submit_to_the_client_registered_for_that_deployment_id() {
    let registry = Registry::new();
    let client = Arc::new(FakeClient::default());
    registry.register("dep-1".into(), client.clone());

    registry
        .try_submit(submit("dep-1", "s1"))
        .expect("dispatch reaches the registered client");

    assert_eq!(client.submitted.lock().expect("lock").len(), 1);
}

#[test]
fn try_submit_against_an_unregistered_deployment_is_unknown_deployment_not_a_panic() {
    let registry = Registry::new();
    let error = registry
        .try_submit(submit("nowhere", "s1"))
        .expect_err("no client is registered");
    assert_eq!(error, DispatchError::UnknownDeployment);
}

#[test]
fn a_client_enqueue_failure_surfaces_through_the_registry_rather_than_being_swallowed() {
    let registry = Registry::new();
    let client = Arc::new(FakeClient {
        refuse: true,
        ..Default::default()
    });
    registry.register("dep-1".into(), client);

    let error = registry
        .try_submit(submit("dep-1", "s1"))
        .expect_err("client refused");
    assert_eq!(error, DispatchError::Enqueue(EnqueueError("closed".into())));
}

#[test]
fn cancel_reaches_the_registered_client_and_reports_it_was_found() {
    let registry = Registry::new();
    let client = Arc::new(FakeClient::default());
    registry.register("dep-1".into(), client.clone());

    let found = registry.cancel("dep-1", "s1".into());

    assert!(found);
    assert_eq!(
        client.canceled.lock().expect("lock").as_slice(),
        &["s1".to_string()]
    );
}

#[test]
fn cancel_against_an_unregistered_deployment_is_a_reported_no_op() {
    let registry = Registry::new();
    assert!(!registry.cancel("nowhere", "s1".into()));
}

#[test]
fn registering_over_an_existing_deployment_id_replaces_it() {
    let registry = Registry::new();
    let first = Arc::new(FakeClient::default());
    let second = Arc::new(FakeClient::default());
    registry.register("dep-1".into(), first.clone());
    registry.register("dep-1".into(), second.clone());

    registry
        .try_submit(submit("dep-1", "s1"))
        .expect("dispatch");

    assert_eq!(first.submitted.lock().expect("lock").len(), 0);
    assert_eq!(second.submitted.lock().expect("lock").len(), 1);
}

#[test]
fn unregister_removes_the_client_and_returns_it() {
    let registry = Registry::new();
    let client = Arc::new(FakeClient::default());
    registry.register("dep-1".into(), client);
    assert!(registry.contains("dep-1"));

    let removed = registry.unregister("dep-1");

    assert!(removed.is_some());
    assert!(!registry.contains("dep-1"));
    let error = registry
        .try_submit(submit("dep-1", "s1"))
        .expect_err("nothing registered any more");
    assert_eq!(error, DispatchError::UnknownDeployment);
}
