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

/// A plan with no role is the one that serves, so every plan written before
/// distributed loading existed still means what it meant.
#[test]
fn a_plan_without_a_role_is_a_front() {
    let plan = Plan::parse(r#"{"endpoint":"127.0.0.1:8080"}"#).expect("parsed");
    assert_eq!(plan.role, Role::Front);
    assert!(plan.workers.is_empty());
    assert_eq!(plan.share(), "declared");
}

/// The two halves of a distributed load, as an operator writes them.
#[test]
fn a_distributed_plan_names_a_device_and_what_it_claims() {
    let worker = Plan::parse(
        r#"{"role":"worker","device":"CUDA1","vram_gb":23,"endpoint":"127.0.0.1:50052"}"#,
    )
    .expect("parsed");
    assert_eq!(worker.role, Role::Worker);
    assert_eq!(worker.device.as_deref(), Some("CUDA1"));
    assert_eq!(worker.vram_gb, Some(23));
    assert_eq!(worker.share(), "CUDA1.declared_23GiB");

    let front = Plan::parse(
        r#"{"role":"front","device":"CUDA0","vram_gb":11,
            "endpoint":"127.0.0.1:18090","workers":["127.0.0.1:50052"]}"#,
    )
    .expect("parsed");
    assert_eq!(front.role, Role::Front);
    assert_eq!(front.share(), "CUDA0.declared_11GiB");
    assert_eq!(front.workers.len(), 1);
    assert_eq!(front.workers[0].port, 50052);
}

/// A worker holds a share; it does not reach for others. The shape would be a
/// chain, and this backend spreads internally.
#[test]
fn a_worker_may_not_declare_workers_of_its_own() {
    let error = Plan::parse(
        r#"{"role":"worker","endpoint":"127.0.0.1:50052","workers":["127.0.0.1:50053"]}"#,
    )
    .unwrap_err();
    assert!(error.contains("cannot declare workers"), "{error}");
}

#[test]
fn a_role_that_is_not_one_of_the_two_is_refused() {
    let error = Plan::parse(r#"{"role":"middle","endpoint":"127.0.0.1:1"}"#).unwrap_err();
    assert!(error.contains("unknown role"), "{error}");
}

#[test]
fn a_worker_that_is_not_an_address_is_refused_at_the_load() {
    let error = Plan::parse(r#"{"endpoint":"127.0.0.1:1","workers":["nowhere"]}"#).unwrap_err();
    assert!(error.contains("host:port"), "{error}");
}
