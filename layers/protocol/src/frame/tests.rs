use super::*;
use crate::{Address, Chain, Link, QueueClass, Recipient};

fn envelope() -> Envelope {
    Envelope {
        target: Address::tcp("127.0.0.1", 19001),
        recipient: Recipient::node("n0"),
        lane: QueueClass::Prefill,
        route: "route-1".into(),
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
