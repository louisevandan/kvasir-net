use super::*;

#[test]
fn a_perfect_link_holds_nothing() {
    let link = Impairment::default();
    assert!(link.is_perfect());
    assert_eq!(link.hold(7, 4096), Duration::ZERO);
}

#[test]
fn latency_is_paid_per_chunk_regardless_of_size() {
    let link = Impairment::latency(Duration::from_millis(20), Duration::ZERO);
    assert_eq!(link.hold(1, 16), Duration::from_millis(20));
    assert_eq!(link.hold(2, 65536), Duration::from_millis(20));
}

#[test]
fn a_narrow_link_charges_by_the_byte() {
    // 1000 bytes per second: a thousand bytes is a second.
    let link = Impairment::bandwidth(1_000);
    assert_eq!(link.hold(1, 1_000), Duration::from_secs(1));
    assert_eq!(link.hold(2, 500), Duration::from_millis(500));
}

/// The property the whole mock fleet rests on: two runs of one scenario differ
/// only if P4 differs.
#[test]
fn jitter_is_irregular_but_repeats_exactly() {
    let link = Impairment::latency(Duration::from_millis(10), Duration::from_millis(10));
    let first: Vec<Duration> = (0..16).map(|n| link.hold(n, 64)).collect();
    let again: Vec<Duration> = (0..16).map(|n| link.hold(n, 64)).collect();
    assert_eq!(first, again, "the same sequence every time");

    let distinct: std::collections::BTreeSet<_> = first.iter().collect();
    assert!(
        distinct.len() > 8,
        "and not the same delay over and over: {first:?}"
    );
    assert!(
        first
            .iter()
            .all(|held| *held >= Duration::from_millis(10) && *held <= Duration::from_millis(20)),
        "each within the declared window: {first:?}"
    );
}

#[test]
fn neighbouring_chunks_do_not_ramp() {
    // An ordered ramp is a pattern a queue can ride, which would make a
    // scenario easier than the network it stands for.
    let link = Impairment::latency(Duration::ZERO, Duration::from_millis(40));
    let held: Vec<u128> = (0..12).map(|n| link.hold(n, 64).as_millis()).collect();
    let climbing = held.windows(2).filter(|pair| pair[1] > pair[0]).count();
    assert!(
        climbing > 2 && climbing < 9,
        "delays move both ways: {held:?}"
    );
}

#[test]
fn a_stalling_link_seizes_on_its_interval_and_not_between() {
    let link = Impairment::stalling(4, Duration::from_millis(100));
    assert_eq!(link.hold(4, 64), Duration::from_millis(100));
    assert_eq!(link.hold(8, 64), Duration::from_millis(100));
    assert_eq!(link.hold(5, 64), Duration::ZERO);
    assert!(!link.is_perfect());
}

#[test]
fn latency_and_narrowness_add_rather_than_replace() {
    let link = Impairment {
        delay: Duration::from_millis(50),
        rate: Some(1_000),
        ..Impairment::default()
    };
    assert_eq!(link.hold(1, 1_000), Duration::from_millis(1_050));
}
