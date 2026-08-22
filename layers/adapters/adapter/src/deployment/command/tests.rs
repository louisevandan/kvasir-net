use super::*;

#[test]
fn a_submit_carries_its_request_opaquely() {
    let submit = Submit {
        deployment_id: "dep-1".into(),
        deployment_generation: 3,
        submission_id: "sub-1".into(),
        request: serde_json::json!({ "prompt": "hello" }),
    };
    assert_eq!(submit.deployment_id, "dep-1");
    assert_eq!(submit.deployment_generation, 3);
    assert_eq!(submit.request, serde_json::json!({ "prompt": "hello" }));
}

#[test]
fn a_cancel_names_only_the_submission_not_the_deployment() {
    let cancel = Cancel {
        submission_id: "sub-1".into(),
    };
    assert_eq!(cancel.submission_id, "sub-1");
}

#[test]
fn two_submits_with_the_same_fields_are_equal() {
    let a = Submit {
        deployment_id: "dep-1".into(),
        deployment_generation: 1,
        submission_id: "sub-1".into(),
        request: serde_json::Value::Null,
    };
    let b = a.clone();
    assert_eq!(a, b);
}
