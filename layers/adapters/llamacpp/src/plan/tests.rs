use super::*;

#[test]
fn a_plan_needs_an_endpoint_and_nothing_else() {
    let plan = Plan::parse(r#"{"endpoint":"127.0.0.1:8080"}"#).expect("parsed");
    assert_eq!(plan.endpoint.port, 8080);
    assert_eq!(plan.model, "default", "a one-model server takes anything");
}

#[test]
fn the_keys_that_are_there_are_read() {
    let plan = Plan::parse(r#"{"endpoint":"gpu-2:9001","model":"qwen","patience_ms":5000}"#)
        .expect("parsed");
    assert_eq!(plan.endpoint.host, "gpu-2");
    assert_eq!(plan.model, "qwen");
    assert_eq!(plan.patience, Duration::from_millis(5000));
}

/// A plan that cannot be honoured is refused at the load, where a caller is
/// waiting and can be told, rather than at the first hop.
#[test]
fn a_plan_that_names_nowhere_is_refused_with_a_reason() {
    for (plan, expected) in [
        ("not json", "not json"),
        ("{}", "no endpoint"),
        (r#"{"endpoint":"nowhere"}"#, "host:port"),
    ] {
        let error = Plan::parse(plan).unwrap_err();
        assert!(error.contains(expected), "{plan} -> {error}");
    }
}
