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

/// The plan's patience reaches the socket, not only the channel above it.
///
/// They were separate, and the plan set only one. An operator asking for a
/// quarter of an hour got it on the channel and two minutes on the socket
/// underneath, so a backend that went quiet for two minutes — a crowded server
/// working its other slots — killed the stream from below while the adapter
/// waited patiently above. Sequences died around their ninetieth token with a
/// timeout nobody had asked for.
#[test]
fn patience_is_one_number_and_the_socket_gets_it_too() {
    let plan =
        Plan::parse(r#"{"endpoint":"127.0.0.1:8080","patience_ms":900000}"#).expect("parsed");
    assert_eq!(plan.patience, Duration::from_millis(900_000));
    assert_eq!(
        plan.endpoint.idle, plan.patience,
        "the socket waits as long as the plan said"
    );
}

/// Left alone when the plan says nothing, so a plan written before this still
/// means what it meant.
#[test]
fn a_plan_that_asks_for_no_patience_keeps_the_default() {
    let bare = Plan::parse(r#"{"endpoint":"127.0.0.1:8080"}"#).expect("parsed");
    assert_eq!(bare.patience, bare.endpoint.idle);
    assert_eq!(bare.endpoint.idle, Endpoint::new("h", 1).idle);
}

/// A load's patience and a token's patience are two numbers.
///
/// Deliberately not derived from one another. The plan's own `patience_ms` is
/// how long one token may take and is measured in seconds; reading seventy
/// gibibytes off a disk is measured in minutes. Deriving the second from the
/// first would kill a load that was going fine, which is the same shape of
/// mistake as a channel and its socket waiting different amounts — and that one
/// cost a debugging session already.
#[test]
fn a_start_waits_on_its_own_number_rather_than_the_plans() {
    let plan = Plan::parse(
        r#"{"endpoint":"127.0.0.1:8080","patience_ms":120000,
            "start":{"binary":"b","weights":"w","context":4096,"slots":4,
                     "batch":512,"ubatch":128,"patience_ms":900000}}"#,
    )
    .expect("parsed");
    let start = plan.start.clone().expect("a start");
    assert_eq!(plan.patience, Duration::from_millis(120_000));
    assert_eq!(start.patience, Duration::from_millis(900_000));
}

/// A `start` with no patience is refused rather than given one.
///
/// Every field of a start is required, because a guessed load timeout is this
/// adapter deciding how long an operator is willing to wait for weights it
/// knows nothing about.
#[test]
fn a_start_that_names_no_patience_is_refused() {
    let refused = Plan::parse(
        r#"{"endpoint":"127.0.0.1:8080",
            "start":{"binary":"b","weights":"w","context":4096,"slots":4,
                     "batch":512,"ubatch":128}}"#,
    )
    .expect_err("a start without patience is not a start");
    assert!(refused.contains("patience_ms"), "{refused}");
}
