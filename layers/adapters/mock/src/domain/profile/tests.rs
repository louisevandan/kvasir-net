use super::*;

#[test]
fn an_empty_plan_is_a_working_deployment() {
    let profile = Profile::parse("{}");
    assert_eq!(profile, Profile::default());
    assert_eq!(profile.token_count(4), 4);
}

#[test]
fn a_plan_that_is_not_json_still_loads() {
    // A simulator that refuses to start on a malformed plan would make the
    // caller debug the fixture instead of the protocol.
    assert_eq!(Profile::parse("not json"), Profile::default());
}

#[test]
fn capacity_comes_from_the_field_a_real_adapter_reads() {
    let profile = Profile::parse(r#"{"load_options":{"batching":{"max_sequences":10}}}"#);
    assert_eq!(profile.max_sequences, 10);
}

#[test]
fn a_capacity_outside_the_accepted_range_takes_the_default() {
    for plan in [
        r#"{"load_options":{"batching":{"max_sequences":0}}}"#,
        r#"{"load_options":{"batching":{"max_sequences":4097}}}"#,
        r#"{"load_options":{"batching":{"max_sequences":"ten"}}}"#,
    ] {
        assert_eq!(Profile::parse(plan).max_sequences, DEFAULT_MAX_SEQUENCES);
    }
}

#[test]
fn timings_are_read_as_milliseconds() {
    let profile = Profile::parse(
        r#"{"mock":{"load_ms":40,"prefill_ms":25,"token_ms":2,"tokens":16,"stages":3}}"#,
    );
    assert_eq!(profile.load, Duration::from_millis(40));
    assert_eq!(profile.prefill, Duration::from_millis(25));
    assert_eq!(profile.token, Duration::from_millis(2));
    assert_eq!(profile.tokens, Some(16));
    assert_eq!(profile.stages, 3);
}

#[test]
fn a_declared_token_count_overrides_what_the_request_asked_for() {
    let profile = Profile::parse(r#"{"mock":{"tokens":3}}"#);
    assert_eq!(profile.token_count(500), 3);
}

#[test]
fn a_request_asking_for_nothing_gets_nothing() {
    assert_eq!(Profile::parse("{}").token_count(0), 0);
}

#[test]
fn load_steps_and_stages_never_reach_zero() {
    let profile = Profile::parse(r#"{"mock":{"load_steps":0,"stages":0}}"#);
    assert_eq!(profile.load_steps, 1);
    assert_eq!(profile.stages, 1);
}

#[test]
fn faults_are_named_by_the_plan() {
    assert_eq!(Profile::parse(r#"{"mock":{"fault":"load"}}"#).fault, Fault::Load);
    assert_eq!(Profile::parse(r#"{"mock":{"fault":"hang"}}"#).fault, Fault::Hang);
    assert_eq!(
        Profile::parse(r#"{"mock":{"fault":"after_tokens","fault_after_tokens":5}}"#).fault,
        Fault::AfterTokens(5)
    );
    assert_eq!(Profile::parse(r#"{"mock":{"fault":"typo"}}"#).fault, Fault::None);
}
