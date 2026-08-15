use super::*;

fn present<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    }
}

#[test]
fn the_current_name_wins_over_the_retired_one() {
    let found = resolve(
        "MAX_INFLIGHT",
        present(&[
            ("P4_PIPELINE_MAX_INFLIGHT", "32"),
            ("P4_ADAPTER_MAX_INFLIGHT", "16"),
        ]),
    );
    assert_eq!(found.as_deref(), Some("32"));
}

#[test]
fn a_benchmark_still_setting_the_retired_name_is_honoured() {
    let found = resolve(
        "MAX_INFLIGHT",
        present(&[("P4_ADAPTER_MAX_INFLIGHT", "16")]),
    );
    assert_eq!(found.as_deref(), Some("16"));
}

#[test]
fn neither_name_set_leaves_the_caller_with_its_default() {
    assert_eq!(resolve("MAX_INFLIGHT", present(&[])), None);
}

#[test]
fn an_out_of_range_or_unparsable_value_takes_the_default() {
    assert_eq!(parse(Some("0".into()), 4, |v| (1..=4096).contains(v)), 4);
    assert_eq!(parse(Some("4097".into()), 4, |v| (1..=4096).contains(v)), 4);
    assert_eq!(parse(Some("many".into()), 4, |v| (1..=4096).contains(v)), 4);
    assert_eq!(parse(None, 4, |v| (1..=4096).contains(v)), 4);
}

#[test]
fn zero_is_valid_where_it_disables_the_behaviour() {
    assert_eq!(parse(Some("0".into()), 5, |v| *v <= 4096), 0);
}
