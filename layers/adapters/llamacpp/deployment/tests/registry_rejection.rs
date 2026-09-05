//! The registry's refusal path, which needs no `apps/llama` checkout.
//!
//! This lived in `broker_registry.rs`, and when that target moved behind the
//! `cross-wire-fixture` feature it went with it - dropped from every default
//! run for a dependency it does not have. It calls no fixture and opens no
//! socket: an empty `Registry` must refuse a `deployment_id` nobody
//! registered, which is P4's own broker surface per `SEALED-CONTRACT.md`
//! §9.1/§9.4 and is exactly the check a default run should keep.

use p4_adapter::deployment::Registry;
use p4_llamacpp_deployment::contract::Submit;
use serde_json::{Value, json};

fn neutral_request() -> Value {
    json!({
        "model": "test-model",
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": true,
    })
}

#[test]
fn a_submission_against_an_unregistered_deployment_id_never_reaches_any_client() {
    let registry: Registry = Registry::new();
    let error = registry
        .try_submit(Submit {
            deployment_id: "nobody-registered-this".into(),
            deployment_generation: 1,
            submission_id: "s1".into(),
            deadline_unix_ms: 0,
            request: neutral_request(),
        })
        .expect_err("no client is registered for this deployment_id");
    assert_eq!(
        error,
        p4_adapter::deployment::DispatchError::UnknownDeployment
    );
}
