use super::*;
use p4_protocol::event::hop::event_digest;

fn key(attempt: u64) -> ReceiptKey {
    ReceiptKey { sender_id: "agent-a".into(), connection_generation: 7, attempt }
}

#[test]
fn reservation_commit_query_and_ack_have_one_owner() {
    let digest = event_digest(b"event");
    let required = receipt_bytes(&key(1), "event-1").unwrap();
    let mut store = ReceiptStore::new(1, required);
    assert_eq!(store.reserve(key(1), digest, "event-1"), Ok(Reservation::Reserved));
    assert_eq!(store.event_id(&key(1)), Some("event-1"));
    assert_eq!(store.snapshot(), ReceiptSnapshot { limit_count: 1, limit_bytes: required,
        records: 1, reserved_bytes: required, pending: 1, accepted: 0, rejected: 0,
        oldest_unix_ms: store.snapshot().oldest_unix_ms });
    assert_eq!(store.query(&key(1), digest).status, ReceiptStatus::Unknown);
    let accepted = store.commit(&key(1), digest, ReceiptStatus::AcceptedExact, "").unwrap();
    assert_eq!(accepted.status, ReceiptStatus::AcceptedExact);
    assert_eq!(store.reserve(key(1), digest, "event-1"), Ok(Reservation::Existing(accepted.clone())));
    assert!(!store.acknowledge(&key(1), event_digest(b"other")));
    assert_eq!(store.query(&key(1), digest), accepted);
    assert!(store.acknowledge(&key(1), digest));
    assert!(!store.acknowledge(&key(1), digest));
    assert_eq!(store.snapshot().records, 0);
    assert_eq!(store.snapshot().reserved_bytes, 0);
}

#[test]
fn count_and_byte_boundaries_refuse_without_mutation() {
    let digest = event_digest(b"event");
    let required = receipt_bytes(&key(1), "event-1").unwrap();
    let mut bytes_short = ReceiptStore::new(1, required - 1);
    assert_eq!(bytes_short.reserve(key(1), digest, "event-1"), Err(ReserveError::TooLarge));
    assert_eq!(bytes_short.snapshot().records, 0);
    let mut count_full = ReceiptStore::new(1, required * 2);
    assert_eq!(count_full.reserve(key(1), digest, "event-1"), Ok(Reservation::Reserved));
    assert_eq!(count_full.reserve(key(2), digest, "event-2"), Err(ReserveError::Full));
    assert_eq!(count_full.snapshot().records, 1);
    assert_eq!(count_full.snapshot().reserved_bytes, required);
}

#[test]
fn digest_conflict_and_oversized_detail_preserve_the_original_record() {
    let digest = event_digest(b"event");
    let required = receipt_bytes(&key(1), "event-1").unwrap();
    let mut store = ReceiptStore::new(1, required);
    store.reserve(key(1), digest, "event-1").unwrap();
    let before = store.snapshot();
    assert_eq!(store.reserve(key(1), event_digest(b"conflict"), "event-1"), Err(ReserveError::Conflict));
    assert_eq!(store.snapshot(), before);
    assert!(store.commit(&key(1), digest, ReceiptStatus::Rejected,
        "x".repeat(MAX_DETAIL_BYTES + 1)).is_err());
    assert_eq!(store.snapshot(), before);
}
