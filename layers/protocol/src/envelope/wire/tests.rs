use super::*;

fn link(node: &str, port: u16) -> Link {
    Link {
        address: Address::tcp("127.0.0.1", port),
        node: node.into(),
        binding: format!("{node}-binding"),
        generation: 7,
    }
}

fn round_trip(envelope: &Envelope) -> Envelope {
    decode(&encode(envelope).expect("encodes")).expect("decodes")
}

#[test]
fn a_control_envelope_survives_the_wire() {
    let envelope = Envelope {
        target: Address::tcp("192.168.0.26", 19001),
        recipient: Recipient::Agent,
        lane: QueueClass::Control,
        route: "route-1".into(),
        deadline_unix_ms: 1_800_000_000_000,
        reply_to: None,
        chain: None,
    };
    assert_eq!(round_trip(&envelope), envelope);
}

#[test]
fn an_inference_envelope_carries_its_whole_chain_and_position() {
    let chain = Chain::at(
        vec![link("n0", 52001), link("n1", 52002), link("n2", 52003)],
        1,
    )
    .expect("a valid position");
    let envelope = Envelope {
        target: Address::tcp("127.0.0.1", 52002),
        recipient: Recipient::node("n1"),
        lane: QueueClass::Prefill,
        route: "route-2".into(),
        deadline_unix_ms: 0,
        reply_to: Some(Address::tcp("10.0.0.1", 19001)),
        chain: Some(chain),
    };
    let decoded = round_trip(&envelope);
    assert_eq!(decoded, envelope);
    assert_eq!(decoded.chain.as_ref().unwrap().position(), 1);
    assert_eq!(decoded.chain.as_ref().unwrap().len(), 3);
}

#[test]
fn every_lane_round_trips() {
    for lane in [
        QueueClass::Control,
        QueueClass::Prefill,
        QueueClass::Decode,
        QueueClass::Response,
    ] {
        let envelope = Envelope {
            target: Address::tcp("h", 1),
            recipient: Recipient::Agent,
            lane,
            route: "r".into(),
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        };
        assert_eq!(round_trip(&envelope).lane, lane);
    }
}

#[test]
fn a_truncated_envelope_is_refused_rather_than_half_read() {
    let bytes = encode(&Envelope {
        target: Address::tcp("h", 1),
        recipient: Recipient::node("n"),
        lane: QueueClass::Decode,
        route: "r".into(),
        deadline_unix_ms: 0,
        reply_to: None,
        chain: None,
    })
    .unwrap();
    for cut in 1..bytes.len() {
        assert!(decode(&bytes[..cut]).is_err(), "prefix {cut} decoded");
    }
}

#[test]
fn trailing_bytes_are_refused() {
    let mut bytes = encode(&Envelope {
        target: Address::tcp("h", 1),
        recipient: Recipient::Agent,
        lane: QueueClass::Control,
        route: "r".into(),
        deadline_unix_ms: 0,
        reply_to: None,
        chain: None,
    })
    .unwrap();
    bytes.push(0);
    assert!(decode(&bytes).is_err());
}

#[test]
fn a_chain_position_past_its_end_is_refused_on_decode() {
    // Hand-build a frame claiming one link but a position of one, which a
    // sender could only produce by corruption or by intent.
    let mut bytes = Vec::new();
    put_text(&mut bytes, "tcp://h:1").unwrap();
    bytes.push(RECIPIENT_NODE);
    put_text(&mut bytes, "n").unwrap();
    bytes.push(1);
    put_text(&mut bytes, "r").unwrap();
    put_u64(&mut bytes, 0);
    bytes.push(ABSENT);
    bytes.push(PRESENT);
    put_u32(&mut bytes, 1);
    put_u32(&mut bytes, 1);
    put_text(&mut bytes, "tcp://h:1").unwrap();
    put_text(&mut bytes, "n").unwrap();
    put_text(&mut bytes, "b").unwrap();
    put_u64(&mut bytes, 1);
    assert!(decode(&bytes).is_err());
}
