use super::*;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("../fixtures.json")).expect("fixtures.json parses as JSON")
}

fn canonical<'a>(root: &'a Value, group: &str, name: &str) -> &'a Value {
    &root["canonical"][group][name]
}

fn malformed<'a>(root: &'a Value, group: &str, name: &str) -> &'a Value {
    &root["malformed"][group][name]
}

#[test]
fn every_canonical_command_parses() {
    let root = fixtures();
    assert!(parse_submit(canonical(&root, "commands", "submit")).is_ok());
    assert!(parse_cancel(canonical(&root, "commands", "cancel")).is_ok());
}

#[test]
fn every_canonical_event_parses() {
    let root = fixtures();
    let names = [
        "accepted",
        "rejected_full",
        "rejected_conflict",
        "rejected_invalid",
        "rejected_deployment_closed",
        "produced",
        "settled_stop",
        "settled_length",
        "settled_canceled",
        "settled_error",
    ];
    for name in names {
        let value = canonical(&root, "events", name);
        assert!(
            parse_event(value).is_ok(),
            "expected {name} to parse, got {:?}",
            parse_event(value)
        );
    }
}

#[test]
fn every_malformed_command_is_rejected() {
    let root = fixtures();
    let object = root["malformed"]["commands"]
        .as_object()
        .expect("commands object");
    for (name, value) in object {
        let submit_result = parse_submit(value);
        let cancel_result = parse_cancel(value);
        assert!(
            submit_result.is_err() && cancel_result.is_err(),
            "expected {name} to be rejected by both parsers"
        );
    }
}

#[test]
fn every_malformed_event_is_rejected() {
    let root = fixtures();
    let object = root["malformed"]["events"]
        .as_object()
        .expect("events object");
    for (name, value) in object {
        assert!(
            parse_event(value).is_err(),
            "expected {name} to be rejected, got {:?}",
            parse_event(value)
        );
    }
}

#[test]
fn the_full_reason_round_trips_through_the_wire_without_string_matching() {
    let event = DeploymentEvent::Rejected(Rejected {
        submission_id: "sub-1".into(),
        reason: RejectedReason::Full,
    });
    let encoded = encode_event(&event);
    assert_eq!(encoded["reason"], "full");
    let decoded = parse_event(&encoded).expect("round-trips");
    match decoded {
        DeploymentEvent::Rejected(rejected) => {
            assert!(matches!(rejected.reason, RejectedReason::Full));
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
}

#[test]
fn encoding_a_submit_reproduces_the_canonical_fixture() {
    let root = fixtures();
    let expected = canonical(&root, "commands", "submit").clone();
    let submit = parse_submit(&expected).expect("canonical submit parses");
    assert_eq!(encode_submit(&submit), expected);
}

#[test]
fn a_negative_generation_and_a_fractional_ordinal_are_each_rejected_specifically() {
    let root = fixtures();
    assert!(parse_submit(malformed(&root, "commands", "submit_negative_generation")).is_err());
    assert!(parse_event(malformed(&root, "events", "produced_non_integer_ordinal")).is_err());
    assert!(parse_event(malformed(&root, "events", "rejected_bad_reason")).is_err());
}
