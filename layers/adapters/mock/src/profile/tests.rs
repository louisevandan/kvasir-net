use super::*;

#[test]
fn cost_belongs_to_the_position_not_the_device() {
    // Measurement found the leading stage far more expensive than the trailing
    // one, and swapping the cards did not move the cost. The mock charges by
    // position for the same reason.
    let profile = Profile::measured_shape(Duration::from_millis(10));
    assert!(profile.hop_cost(0, false) > profile.hop_cost(1, false));
    assert_eq!(profile.hop_cost(1, false), profile.hop_cost(5, false));
}

#[test]
fn prefill_costs_more_than_a_lap_at_the_same_position() {
    let profile = Profile::measured_shape(Duration::from_millis(10));
    assert!(profile.hop_cost(0, true) > profile.hop_cost(0, false));
    assert!(profile.hop_cost(1, true) > profile.hop_cost(1, false));
}

#[test]
fn a_load_is_divided_across_its_stages() {
    let profile = Profile {
        load: Duration::from_millis(40),
        stages: 4,
        ..Profile::default()
    };
    assert_eq!(profile.stage_cost(), Duration::from_millis(10));
}

#[test]
fn a_stageless_profile_does_not_divide_by_zero() {
    let profile = Profile {
        load: Duration::from_millis(40),
        stages: 0,
        ..Profile::default()
    };
    assert_eq!(profile.stage_cost(), Duration::from_millis(40));
}

#[test]
fn a_default_profile_costs_nothing_and_is_the_fast_case_for_tests() {
    let profile = Profile::default();
    assert_eq!(profile.hop_cost(0, true), Duration::ZERO);
    assert_eq!(profile.fault, Fault::None);
}

#[test]
fn scaling_keeps_the_proportions() {
    // A test runs in milliseconds what a GPU took a minute to do; only the
    // ratios have to survive.
    let fast = Profile::measured_shape(Duration::from_millis(1));
    let slow = Profile::measured_shape(Duration::from_millis(100));
    assert_eq!(
        slow.leading_hop.as_millis() / slow.trailing_hop.as_millis(),
        fast.leading_hop.as_millis() / fast.trailing_hop.as_millis()
    );
}
