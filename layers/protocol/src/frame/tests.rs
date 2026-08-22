use super::*;
use crate::{Address, Chain, Link, QueueClass, Recipient};

fn envelope() -> Envelope {
    Envelope {
        target: Address::tcp("127.0.0.1", 19001),
        recipient: Recipient::node("n0"),
        lane: QueueClass::Prefill,
        route: "route-1".into(),
        request_id: "request-1".into(),
        stream_id: "stream-1".into(),
        origin_agent: Some(Address::tcp("10.0.0.2", 19001)),
        return_channel: Some("channel-1".into()),
        ingress_generation: 0,
        event_seq: 1,
        deadline_unix_ms: 0,
        reply_to: Some(Address::tcp("10.0.0.1", 19001)),
        chain: Some(
            Chain::new(vec![Link {
                address: Address::tcp("127.0.0.1", 52001),
                node: "n0".into(),
                binding: "b".into(),
                generation: 1,
            }])
            .unwrap(),
        ),
    }
}

#[test]
fn a_frame_survives_a_round_trip_with_its_body_intact() {
    let body = b"an opaque payload".to_vec();
    let bytes = encode(&envelope(), &body).unwrap();
    let decoded = decode(&bytes).unwrap();
    assert_eq!(decoded.envelope, envelope());
    assert_eq!(decoded.body, body);
}

#[test]
fn an_empty_body_is_a_valid_frame() {
    let decoded = decode(&encode(&envelope(), &[]).unwrap()).unwrap();
    assert!(decoded.body.is_empty());
}

#[test]
fn the_header_alone_says_how_long_the_frame_is() {
    // A reader needs this before it has the rest, and must get it without
    // decoding anything.
    let bytes = encode(&envelope(), b"body").unwrap();
    assert_eq!(frame_len(&bytes[..16]).unwrap(), bytes.len());
}

#[test]
fn a_version_that_is_not_ours_is_refused_rather_than_misread() {
    // A stale binary on a benchmark host must fail here rather than parse a
    // v6 payload as if it were v5.
    let mut bytes = encode(&envelope(), b"body").unwrap();
    bytes[4] = 5;
    assert!(frame_len(&bytes).is_err());
    assert!(decode(&bytes).is_err());
}

#[test]
fn a_mixed_frame_version_7_and_8_fleet_is_refused_before_any_body_is_read() {
    // Pins the coordinated-upgrade property the `SessionClose`/`SessionClosed`
    // ack contract depends on (see `EOS-ACK-BRIEF.md` section 1 and
    // `docs/protocol.md` section 13.2): a fleet half on the old one-way close
    // and half on the new acked one must never admit work to each other, in
    // either direction, rather than run normally until the first early stop
    // silently leaks a reservation on the stale side.
    //
    // The body carried here is deliberately not a valid envelope-adjacent
    // payload -- it is a handful of bytes nothing in this crate could ever
    // decode. If either assertion below failed to reject the frame, this
    // test would still catch it: `decode` would go on to try `wire::decode`
    // on garbage and fail there too, but for a completely different reason
    // ("frame version 7 is not 8" vs an envelope decode error). Asserting
    // the exact message pins that the version check runs first, which is
    // what lets frame version alone -- without a body catalog and without a
    // handshake -- fence admission.
    let garbage_body = vec![0xFFu8; 32];

    // Direction 1: an old (7) peer's frame arrives at this (8) codebase.
    let mut old_peer_frame = encode(&envelope(), &garbage_body).unwrap();
    old_peer_frame[4] = 7;
    let err = decode(&old_peer_frame).unwrap_err();
    assert_eq!(err.to_string(), "frame version 7 is not 8");
    assert!(
        frame_len(&old_peer_frame).is_err(),
        "rejected before length is even trusted, let alone the body read"
    );

    // Direction 2: this (8) codebase's own frame, as an old (7) peer would
    // see it. There is no live v7 decoder left in this tree to call, so this
    // states the same header check a v7 build carried (`header[4] != 7`)
    // against a genuine v8-encoded frame, proving the rejection is
    // symmetric rather than an artifact of which side is "newer".
    let new_peer_frame = encode(&envelope(), &garbage_body).unwrap();
    assert_eq!(new_peer_frame[4], VERSION);
    assert_ne!(
        new_peer_frame[4], 7,
        "a v7 reader's own `header[4] != VERSION` check would reject this frame"
    );
}

#[test]
fn a_foreign_magic_is_refused() {
    let mut bytes = encode(&envelope(), b"body").unwrap();
    bytes[0] = b'X';
    assert!(frame_len(&bytes).is_err());
}

#[test]
fn a_header_shorter_than_a_header_is_refused() {
    assert!(frame_len(&[]).is_err());
    assert!(frame_len(&[0; 15]).is_err());
}

#[test]
fn a_length_disagreeing_with_the_header_is_refused() {
    let mut bytes = encode(&envelope(), b"body").unwrap();
    bytes.push(0);
    assert!(decode(&bytes).is_err());
}

#[test]
fn resealing_moves_a_body_to_a_new_target_without_re_encoding_it() {
    let body = b"hidden work".to_vec();
    let first = decode(&encode(&envelope(), &body).unwrap()).unwrap();

    let mut onward = first.envelope.clone();
    onward.target = Address::tcp("192.168.0.29", 19001);
    let resealed = decode(&reseal(&onward, first.body.clone()).unwrap()).unwrap();

    assert_eq!(
        resealed.envelope.target,
        Address::tcp("192.168.0.29", 19001)
    );
    assert_eq!(resealed.body, body);
}
