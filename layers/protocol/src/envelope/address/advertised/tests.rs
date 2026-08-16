use super::*;

#[test]
fn no_hint_uses_the_bound_socket() {
    let own = Address::advertised(None, "0.0.0.0", 19311).expect("advertised");
    assert_eq!(own.to_string(), "tcp://0.0.0.0:19311");
}

#[test]
fn a_bare_host_takes_the_bound_port() {
    let own = Address::advertised(Some("192.168.0.29"), "0.0.0.0", 52001).expect("advertised");
    assert_eq!(own.to_string(), "tcp://192.168.0.29:52001");
}

/// The defect this module exists for: the port was appended to a value that
/// already carried one, and the agent reported itself ready on it.
#[test]
fn a_host_and_port_is_taken_whole() {
    let own =
        Address::advertised(Some("192.168.0.29:52001"), "0.0.0.0", 52001).expect("advertised");
    assert_eq!(own.to_string(), "tcp://192.168.0.29:52001");
}

#[test]
fn a_hint_may_state_a_port_the_socket_did_not_bind() {
    let own = Address::advertised(Some("gateway:52002"), "0.0.0.0", 52001).expect("advertised");
    assert_eq!(own.to_string(), "tcp://gateway:52002");
}

#[test]
fn a_full_address_is_parsed() {
    let own =
        Address::advertised(Some("tcp://192.168.0.29:52001"), "0.0.0.0", 1).expect("advertised");
    assert_eq!(own.to_string(), "tcp://192.168.0.29:52001");
}

#[test]
fn a_bare_ipv6_literal_is_a_host() {
    let own = Address::advertised(Some("::1"), "0.0.0.0", 52001).expect("advertised");
    assert_eq!(own.to_string(), "tcp://::1:52001");
}

#[test]
fn a_bracketed_ipv6_literal_may_carry_a_port() {
    let own = Address::advertised(Some("[::1]:52001"), "0.0.0.0", 1).expect("advertised");
    assert_eq!(own.host, "[::1]");
    assert_eq!(own.port, 52001);
}

#[test]
fn an_empty_hint_falls_back_rather_than_naming_nothing() {
    let own = Address::advertised(Some("   "), "10.0.0.1", 52001).expect("advertised");
    assert_eq!(own.to_string(), "tcp://10.0.0.1:52001");
}

#[test]
fn a_hint_that_is_not_an_address_is_refused() {
    assert!(Address::advertised(Some("192.168.0.29:not-a-port"), "0.0.0.0", 52001).is_err());
    assert!(Address::advertised(Some("192.168.0.29:0"), "0.0.0.0", 52001).is_err());
    assert!(Address::advertised(Some("udp://192.168.0.29:52001"), "0.0.0.0", 52001).is_err());
}

#[test]
fn an_identity_only_this_machine_can_reach_is_recognised() {
    for host in [
        "0.0.0.0",
        "::",
        "127.0.0.1",
        "127.0.1.5",
        "localhost",
        "[::1]",
    ] {
        assert!(
            Address::tcp(host, 52001).is_local_only(),
            "{host} should be local only"
        );
    }
    for host in ["192.168.0.29", "10.0.0.4", "agent-2.internal"] {
        assert!(
            !Address::tcp(host, 52001).is_local_only(),
            "{host} should be reachable"
        );
    }
}
