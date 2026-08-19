use super::*;

fn link(node: &str, port: u16) -> Link {
    Link {
        address: Address::tcp("127.0.0.1", port),
        node: node.into(),
        binding: format!("{node}-binding"),
        generation: 1,
    }
}

fn inference(hops: u16) -> Envelope {
    let chain = Chain::new(
        (0..hops)
            .map(|i| link(&format!("n{i}"), 52001 + i))
            .collect(),
    )
    .unwrap();
    Envelope {
        target: chain.current().address.clone(),
        recipient: Recipient::node(chain.current().node.clone()),
        lane: QueueClass::Prefill,
        route: "route-1".into(),
        request_id: "request-1".into(),
        stream_id: "stream-1".into(),
        origin_agent: Some(Address::tcp("10.0.0.2", 19001)),
        return_channel: Some("channel-1".into()),
        ingress_generation: 0,
        event_seq: 7,
        deadline_unix_ms: 0,
        reply_to: Some(Address::tcp("10.0.0.1", 19001)),
        chain: Some(chain),
    }
}

fn control() -> Envelope {
    Envelope {
        target: Address::tcp("127.0.0.1", 19001),
        recipient: Recipient::Agent,
        lane: QueueClass::Control,
        route: "route-2".into(),
        request_id: "request-2".into(),
        stream_id: "stream-2".into(),
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
fn ownership_is_decided_by_the_address_alone() {
    let envelope = control();
    assert!(envelope.is_mine(&Address::tcp("127.0.0.1", 19001)));
    assert!(!envelope.is_mine(&Address::tcp("127.0.0.1", 19002)));
    assert!(!envelope.is_mine(&Address::tcp("127.0.0.2", 19001)));
}

#[test]
fn a_hop_retargets_at_the_next_node_and_keeps_the_request_wide_fields() {
    let first = inference(3);
    let second = first.to_next_hop().expect("a second hop exists");

    assert_eq!(second.target, Address::tcp("127.0.0.1", 52002));
    assert_eq!(second.recipient, Recipient::node("n1"));
    assert_eq!(second.chain.as_ref().unwrap().position(), 1);
    // The deadline and the reply address belong to the request, not the hop.
    assert_eq!(second.reply_to, first.reply_to);
    assert_eq!(second.route, first.route);
    assert_eq!(second.request_id, first.request_id);
    assert_eq!(second.stream_id, first.stream_id);
    assert_eq!(second.return_channel, first.return_channel);
    assert_eq!(second.event_seq, first.event_seq);
}

#[test]
fn the_last_node_has_nowhere_to_hand_the_work_on_to() {
    let last = inference(2).to_next_hop().expect("second of two");
    assert!(last.chain.as_ref().unwrap().is_last());
    assert_eq!(last.to_next_hop(), None);
}

#[test]
fn a_single_node_chain_never_hops() {
    assert_eq!(inference(1).to_next_hop(), None);
}

#[test]
fn a_lap_returns_to_the_first_node_in_the_decode_lane() {
    let last = inference(2).to_next_hop().unwrap();
    let lap = last.to_next_lap().expect("a chain can lap");

    assert_eq!(lap.recipient, Recipient::node("n0"));
    assert_eq!(lap.chain.as_ref().unwrap().position(), 0);
    // A lap is decode work even though the pass that produced it was prefill.
    assert_eq!(lap.lane, QueueClass::Decode);
}

#[test]
fn a_control_message_has_no_chain_to_walk() {
    let envelope = control();
    assert_eq!(envelope.to_next_hop(), None);
    assert_eq!(envelope.to_next_lap(), None);
}

/// The chain used to be cleared here, on the reading that a reply is not still
/// traversing. True, and it threw away the only record of how the answer got
/// to where it is — so an agent that could not reach the caller had nothing to
/// fall back on. It is kept now, and this test says so in the place that used
/// to say the opposite.
#[test]
fn a_reply_goes_to_whoever_asked_and_keeps_the_way_it_came() {
    let reply = inference(3).to_reply().expect("a reply address was given");
    assert_eq!(reply.target, Address::tcp("10.0.0.2", 19001));
    assert_eq!(reply.recipient, Recipient::Agent);
    assert_eq!(reply.lane, QueueClass::Response);
    // Nothing further replies to a reply.
    assert_eq!(reply.reply_to, None);
    assert_eq!(
        reply.chain,
        inference(3).chain,
        "every link is an address that demonstrably reached this machine"
    );
}

#[test]
fn the_ingress_agent_wins_over_a_stale_direct_reply_target() {
    let mut request = inference(2);
    request.reply_to = Some(Address::tcp("192.0.2.99", 19001));
    assert_eq!(
        request.to_reply().expect("ingress return anchor").target,
        Address::tcp("10.0.0.2", 19001)
    );
}

#[test]
fn return_key_separates_channels_even_when_transport_routes_are_reused() {
    let mut left = inference(1);
    let mut right = left.clone();
    left.route = "reused-route".into();
    right.route = "reused-route".into();
    left.return_channel = Some("outer-a".into());
    right.return_channel = Some("outer-b".into());
    assert_ne!(left.return_key(), right.return_key());
}

#[test]
fn a_message_sent_without_a_continuation_has_nowhere_to_reply() {
    assert_eq!(control().to_reply(), None);
}

/// The route home is the chain's first link.
///
/// Not a guess about where the caller is, which nothing here can answer, but a
/// record of who it was talking to: the first link is the stage the caller sent
/// the work to, so it is an agent the caller reached — and an address that
/// demonstrably reached this machine, because the work came from it.
#[test]
fn a_reply_that_cannot_be_delivered_knows_who_the_caller_was_talking_to() {
    let reply = inference(3).to_reply().expect("a reply address was given");
    let elsewhere = Address::tcp("10.9.9.9", 52001);
    assert_eq!(
        reply.relay_home(&elsewhere),
        Some(Address::tcp("127.0.0.1", 52001)),
    );
}

/// Relaying to the address that just failed is not a fallback.
#[test]
fn the_route_home_is_never_the_target_that_could_not_be_reached() {
    let mut reply = inference(3).to_reply().expect("a reply");
    reply.target = Address::tcp("127.0.0.1", 52001);
    let elsewhere = Address::tcp("10.9.9.9", 52001);
    assert_eq!(reply.relay_home(&elsewhere), None);
}

/// Nor is handing the frame back to the agent that could not send it.
#[test]
fn the_route_home_is_never_this_agent() {
    let reply = inference(3).to_reply().expect("a reply");
    let first = reply.chain.as_ref().unwrap().links()[0].address.clone();
    assert_eq!(reply.relay_home(&first), None);
}

/// A message that never travelled has no way back that it can prove.
#[test]
fn a_message_with_no_chain_has_no_route_home() {
    assert_eq!(control().relay_home(&Address::tcp("10.9.9.9", 1)), None);
}
