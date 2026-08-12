use super::{DEFAULT_MAX_INFLIGHT, max_inflight};

#[test]
fn an_absent_field_uses_the_conservative_default() {
    assert_eq!(max_inflight("{}"), DEFAULT_MAX_INFLIGHT);
    assert_eq!(max_inflight(r#"{"other":4}"#), DEFAULT_MAX_INFLIGHT);
}

#[test]
fn a_valid_value_is_taken_verbatim() {
    assert_eq!(max_inflight(r#"{"p4_max_inflight":4}"#), 4);
    assert_eq!(max_inflight(r#"{"p4_max_inflight":1024}"#), 1024);
}

#[test]
fn out_of_range_and_malformed_values_fall_back() {
    assert_eq!(max_inflight(r#"{"p4_max_inflight":0}"#), DEFAULT_MAX_INFLIGHT);
    assert_eq!(
        max_inflight(r#"{"p4_max_inflight":1025}"#),
        DEFAULT_MAX_INFLIGHT
    );
    assert_eq!(
        max_inflight(r#"{"p4_max_inflight":-1}"#),
        DEFAULT_MAX_INFLIGHT
    );
    assert_eq!(max_inflight("not json"), DEFAULT_MAX_INFLIGHT);
}

#[test]
fn unrelated_placement_data_is_ignored_rather_than_interpreted() {
    let spec = r#"{"p4_max_inflight":8,"placement":[{"node":"a","layers":[0,16]}]}"#;
    assert_eq!(max_inflight(spec), 8);
}
