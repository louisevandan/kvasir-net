use super::*;
use p4_protocol::QueueClass;

fn own() -> Address {
    Address::tcp("192.168.0.6", 19001)
}

fn envelope(target: Address, recipient: Recipient) -> Envelope {
    Envelope {
        target,
        recipient,
        lane: QueueClass::Control,
        route: "r".into(),
        request_id: "r".into(),
        stream_id: "r".into(),
        origin_agent: None,
        return_channel: None,
        ingress_generation: 0,
        event_seq: 0,
        deadline_unix_ms: 0,
        reply_to: None,
        chain: None,
    }
}

#[test]
fn a_message_for_another_agent_is_forwarded_whole() {
    let elsewhere = Address::tcp("192.168.0.26", 19001);
    let verdict = judge(&envelope(elsewhere.clone(), Recipient::Agent), &own());
    assert_eq!(verdict, Verdict::Forward(elsewhere));
}

#[test]
fn a_message_for_outer_is_forwarded_by_the_same_path() {
    // The point of the collapse: nothing distinguishes a reply travelling out
    // to OUTER from a message travelling on to a peer.
    let outer = Address::tcp("10.0.0.1", 19001);
    let verdict = judge(&envelope(outer.clone(), Recipient::Agent), &own());
    assert_eq!(verdict, Verdict::Forward(outer));
}

#[test]
fn our_own_address_with_an_agent_recipient_stays_here() {
    assert_eq!(
        judge(&envelope(own(), Recipient::Agent), &own()),
        Verdict::Agent
    );
}

#[test]
fn our_own_address_with_a_node_recipient_names_the_node() {
    assert_eq!(
        judge(&envelope(own(), Recipient::node("node-a")), &own()),
        Verdict::Node("node-a".into())
    );
}

#[test]
fn the_same_host_on_a_different_port_is_somebody_else() {
    // Two agents share a machine during a fleet run, so this is a real case
    // and not a pedantic one.
    let sibling = Address::tcp("192.168.0.6", 19002);
    assert_eq!(
        judge(&envelope(sibling.clone(), Recipient::Agent), &own()),
        Verdict::Forward(sibling)
    );
}

#[test]
fn the_verdict_never_depends_on_the_lane_or_the_chain() {
    // A relay must cost the same whatever it carries. If any of these changed
    // the verdict, forwarding would need to understand the traffic.
    let mut carrying = envelope(Address::tcp("192.168.0.26", 19001), Recipient::node("n"));
    carrying.lane = QueueClass::Decode;
    let plain = judge(
        &envelope(Address::tcp("192.168.0.26", 19001), Recipient::Agent),
        &own(),
    );
    assert_eq!(judge(&carrying, &own()), plain);
}
