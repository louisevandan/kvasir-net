use super::*;

#[test]
fn an_address_survives_a_round_trip_through_its_text() {
    let address = Address::tcp("192.168.0.26", 19001);
    assert_eq!(address.to_string(), "tcp://192.168.0.26:19001");
    assert_eq!(
        "tcp://192.168.0.26:19001".parse::<Address>().unwrap(),
        address
    );
}

#[test]
fn an_ipv6_literal_keeps_its_own_colons() {
    // Splitting from the left would take the first colon of the address and
    // leave a host that never matches, so a relay would forward to itself.
    let parsed = "tcp://[fe80::1]:52001".parse::<Address>().unwrap();
    assert_eq!(parsed.host, "[fe80::1]");
    assert_eq!(parsed.port, 52001);
}

#[test]
fn two_spellings_of_one_address_compare_equal() {
    // Equality is how a worker decides a message is its own, so it has to be
    // exact rather than approximate.
    assert_eq!(Address::tcp("host", 1), Address::tcp("host", 1));
    assert_ne!(Address::tcp("host", 1), Address::tcp("host", 2));
    assert_ne!(Address::tcp("host", 1), Address::tcp("other", 1));
}

#[test]
fn a_malformed_address_is_refused_rather_than_guessed_at() {
    for value in [
        "192.168.0.26:19001",
        "tcp://192.168.0.26",
        "tcp://:19001",
        "tcp://host:0",
        "tcp://host:port",
        "http://host:80",
    ] {
        assert!(
            value.parse::<Address>().is_err(),
            "{value} should not parse"
        );
    }
}
