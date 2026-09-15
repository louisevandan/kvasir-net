use super::*;

#[test]
fn agent_inspection_content_types_are_versioned_protocol_constants() {
    assert_eq!(
        AGENT_INSPECT_CONTENT_TYPE,
        "application/vnd.p4.agent.inspect-v1+json"
    );
    assert_eq!(
        AGENT_SNAPSHOT_CONTENT_TYPE,
        "application/vnd.p4.agent.snapshot-v1+json"
    );
}

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

#[test]
fn return_context_is_mandatory_on_every_hop_and_outer_identity_must_agree() {
    for class in [
        EventClass::Control,
        EventClass::Data,
        EventClass::Output,
        EventClass::Telemetry,
    ] {
        let mut event = sample();
        event.envelope.class = class;
        event.envelope.target = Endpoint::node(Address::tcp("10.0.0.4", 52001), "next", 8);
        event.envelope.return_route = None;
        assert!(
            event.validate().is_err(),
            "node-to-node must not erase request provenance"
        );
        assert!(encode(&event).is_err());
        assert!(
            event.envelope.reply_target().is_err(),
            "source is not a fallback"
        );
    }
    for source in [false, true] {
        for field in 0..3 {
            let mut event = sample();
            let mut other = event.envelope.return_route.clone().unwrap();
            match field {
                0 => other.ingress_agent = Address::tcp("10.0.0.9", 52001),
                1 => other.channel.push_str("-different"),
                _ => other.connection_generation += 1,
            }
            if source {
                event.envelope.source = Endpoint::Outer(other);
            } else {
                event.envelope.target = Endpoint::Outer(other);
            }
            assert!(event.validate().is_err());
        }
    }
}

#[test]
fn request_context_survives_wire_hops_and_mixed_owner_reply_selection() {
    let route = sample().envelope.return_route.unwrap();
    let mut root = sample();
    root.envelope.source = Endpoint::Outer(route.clone());
    root.envelope.target = Endpoint::node(Address::tcp("10.0.0.2", 52001), "head", 3);
    root.envelope.causation_id = None;
    root.envelope.event_id = "root".into();
    let first = decode(&encode(&root).unwrap()).unwrap();
    let forwarded = Event {
        envelope: first.envelope.next(
            "hop",
            first.envelope.target.clone(),
            Endpoint::node(Address::tcp("10.0.0.3", 52001), "tail", 7),
            EventClass::Data,
            2,
            "application/octet-stream",
        ),
        payload: vec![255, 0, 128],
    };
    let tail = decode(&encode(&forwarded).unwrap()).unwrap();
    assert_eq!(tail.envelope.return_route, Some(route.clone()));
    let first_owner = root.envelope.return_context().unwrap();
    let second_owner = ReturnContext {
        route: OuterEndpoint {
            ingress_agent: Address::tcp("10.0.0.5", 52001),
            channel: "another-outer".into(),
            connection_generation: 22,
        },
        correlation_id: "another-request".into(),
        deadline_unix_ms: Some(8888),
    };
    for (index, owner) in [first_owner, second_owner].iter().enumerate() {
        let reply = owner
            .reply(
                &tail.envelope,
                format!("reply-{index}"),
                tail.envelope.target.clone(),
                EventClass::Output,
                index as u64 + 1,
                "application/octet-stream",
            )
            .unwrap();
        let delivered = decode(
            &encode(&Event {
                envelope: reply,
                payload: vec![],
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            delivered.envelope.target,
            Endpoint::Outer(owner.route.clone())
        );
        assert_eq!(delivered.envelope.return_route.as_ref(), Some(&owner.route));
        assert_eq!(delivered.envelope.correlation_id, owner.correlation_id);
        assert_eq!(delivered.envelope.deadline_unix_ms, owner.deadline_unix_ms);
        assert_eq!(delivered.envelope.causation_id.as_deref(), Some("hop"));
    }
}

#[test]
fn wire_decoder_rejects_the_legacy_absent_return_route_flag() {
    let event = sample();
    let mut wire = encode(&event).unwrap();
    let mut field = vec![1];
    for text in ["tcp://10.0.0.1:52001", "outer-7"] {
        field.extend((text.len() as u32).to_le_bytes());
        field.extend(text.as_bytes());
    }
    field.extend(9u64.to_le_bytes());
    let position = wire
        .windows(field.len())
        .rposition(|bytes| bytes == field)
        .unwrap();
    let old_length = u32::from_le_bytes(wire[4..8].try_into().unwrap());
    wire.splice(position..position + field.len(), [0]);
    wire[4..8].copy_from_slice(&(old_length - field.len() as u32 + 1).to_le_bytes());
    assert!(
        decode(&wire)
            .unwrap_err()
            .to_string()
            .contains("explicit OUTER return route")
    );
}
