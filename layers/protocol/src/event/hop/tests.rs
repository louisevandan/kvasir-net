use super::*;

fn digest() -> EventDigest { event_digest(b"P4E3 canonical event") }

#[test]
fn every_hop_frame_has_one_canonical_round_trip() {
    let frames = [
        HopFrame::Hello { sender_id: "tcp://127.0.0.1:42000".into(), connection_generation: 7,
            max_outstanding: 64, max_receipt_bytes: 1_048_576 },
        HopFrame::HelloAck { accepted_connection_generation: 7, sender_id: "agent-b".into(),
            connection_generation: 11, max_outstanding: 32, max_receipt_bytes: 524_288 },
        HopFrame::Data { attempt: 9, digest: event_digest(b"event"), event: b"event".to_vec() },
        HopFrame::Receipt { attempt: 9, digest: digest(), status: ReceiptStatus::AcceptedExact, detail: String::new() },
        HopFrame::ReceiptAck { sender_id: "agent-a".into(), connection_generation: 7,
            attempt: 9, digest: digest() },
        HopFrame::Query { sender_id: "agent-a".into(), connection_generation: 7, attempt: 9, digest: digest() },
        HopFrame::QueryResult { attempt: 9, digest: digest(), status: ReceiptStatus::Unknown, detail: "not pinned".into() },
    ];
    for frame in frames { assert_eq!(decode(&encode(&frame).unwrap()).unwrap(), Some(frame)); }
}

#[test]
fn digest_matches_independent_sha256_vector() {
    assert_eq!(event_digest(b"abc"), [0xba,0x78,0x16,0xbf,0x8f,0x01,0xcf,0xea,
        0x41,0x41,0x40,0xde,0x5d,0xae,0x22,0x23,0xb0,0x03,0x61,0xa3,0x96,0x17,
        0x7a,0x9c,0xb4,0x10,0xff,0x61,0xf2,0x00,0x15,0xad]);
}

#[test]
fn legacy_and_malformed_frames_never_cross_classify() {
    assert_eq!(decode(b"P4E3 legacy").unwrap(), None);
    let valid = encode(&HopFrame::ReceiptAck { sender_id: "agent-a".into(),
        connection_generation: 7, attempt: 1, digest: digest() }).unwrap();
    for cut in 4..valid.len() { assert!(decode(&valid[..cut]).is_err(), "cut={cut}"); }
    let mut version = valid.clone(); version[4] = 2;
    assert!(decode(&version).unwrap_err().to_string().contains("version"));
    let mut reserved = valid.clone(); reserved[6] = 1;
    assert!(decode(&reserved).unwrap_err().to_string().contains("reserved"));
    let mut trailing = valid; trailing.push(0);
    assert!(decode(&trailing).unwrap_err().to_string().contains("trailing"));
}

#[test]
fn invalid_identity_limits_digest_and_status_are_rejected() {
    for frame in [
        HopFrame::Hello { sender_id: String::new(), connection_generation: 1, max_outstanding: 1, max_receipt_bytes: 1 },
        HopFrame::Hello { sender_id: "a".into(), connection_generation: 0, max_outstanding: 1, max_receipt_bytes: 1 },
        HopFrame::ReceiptAck { sender_id: "agent-a".into(), connection_generation: 7,
            attempt: 0, digest: digest() },
    ] { assert!(encode(&frame).is_err()); }
    let mut bad_digest = HopFrame::Data { attempt: 1, digest: digest(), event: b"different".to_vec() };
    assert!(encode(&bad_digest).is_err());
    if let HopFrame::Data { digest, event, .. } = &mut bad_digest { *digest = event_digest(event); }
    let mut encoded = encode(&bad_digest).unwrap();
    let index = 4 + 2 + 2 + 1 + 8;
    encoded[index] ^= 1;
    assert!(decode(&encoded).unwrap_err().to_string().contains("digest"));
    let mut receipt = encode(&HopFrame::Receipt { attempt: 1, digest: digest(),
        status: ReceiptStatus::AcceptedExact, detail: String::new() }).unwrap();
    receipt[4 + 2 + 2 + 1 + 8 + 32] = 99;
    assert!(decode(&receipt).unwrap_err().to_string().contains("status"));
}
