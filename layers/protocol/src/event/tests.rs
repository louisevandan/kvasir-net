use super::*;

fn sample() -> Event {
    let ingress = Address::tcp("10.0.0.1", 52001);
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: "event-2".into(),
            correlation_id: "request-1".into(),
            causation_id: Some("event-1".into()),
            source: Endpoint::node(Address::tcp("10.0.0.3", 52001), "tail", 7),
            target: Endpoint::outer(ingress.clone(), "outer-7", 9),
            return_route: Some(OuterEndpoint {
                ingress_agent: ingress,
                channel: "outer-7".into(),
                connection_generation: 9,
            }),
            class: EventClass::Output,
            sequence: 4,
            deadline_unix_ms: Some(9_999),
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: "application/vnd.p4.llamacpp.output-v1".into(),
        },
        payload: vec![1, 2, 3, 4],
    }
}

#[test]
fn event_wire_round_trips_every_routing_field() {
    let event = sample();
    assert_eq!(decode(&encode(&event).unwrap()).unwrap(), event);
}

#[test]
fn next_event_preserves_return_route_but_not_source() {
    let event = sample();
    let next = event.envelope.next(
        "event-3",
        event.envelope.target.clone(),
        Endpoint::node(Address::tcp("10.0.0.2", 52001), "first", 5),
        EventClass::Data,
        5,
        "application/vnd.p4.llamacpp.decode-v1",
    );
    assert_eq!(next.causation_id.as_deref(), Some("event-2"));
    assert_eq!(next.return_route, event.envelope.return_route);
    assert_ne!(next.source, event.envelope.source);
}

#[test]
fn source_is_not_treated_as_the_outer_return_route() {
    let event = sample();
    assert_ne!(
        event.envelope.source.agent_address(),
        &event.envelope.return_route.as_ref().unwrap().ingress_agent
    );
}

#[test]
fn outer_generation_and_channel_are_mandatory() {
    let mut event = sample();
    if let Endpoint::Outer(outer) = &mut event.envelope.target {
        outer.connection_generation = 0;
    }
    assert!(event.validate().is_err());
}

#[test]
fn node_generation_is_part_of_the_wire_identity_and_is_mandatory() {
    let mut event = sample();
    if let Endpoint::Node { generation, .. } = &mut event.envelope.source {
        *generation = 0;
    }
    assert!(event.validate().is_err());
}

#[test]
fn trailing_and_truncated_frames_are_rejected() {
    let encoded = encode(&sample()).unwrap();
    assert!(decode(&encoded[..encoded.len() - 1]).is_err());
    let mut trailing = encoded;
    trailing.push(0);
    assert!(decode(&trailing).is_err());
}
