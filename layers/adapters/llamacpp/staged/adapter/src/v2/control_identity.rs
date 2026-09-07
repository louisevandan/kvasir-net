//! Adapter-owned native mutation envelope. Bare slot mutations are not v1.
use super::{ReleaseSequence, SettlementSequence};

pub(crate) fn prefix(
    generation: u64,
    session: &str,
    key: &str,
    slot: u32,
    incarnation: u64,
    operation: u64,
) -> Result<Vec<u8>, String> {
    if generation == 0
        || incarnation == 0
        || operation == 0
        || session.is_empty()
        || session.contains('\0')
        || key.is_empty()
        || session.len() > 4096
        || key.len() > 4096
        || key.split_once('\0').is_none_or(|(owner, request)| {
            owner != session || request.is_empty() || request.contains('\0')
        })
    {
        return Err("native control identity is invalid".into());
    }
    let mut bytes = b"P4ID".to_vec();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    for value in [generation, incarnation, operation] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&slot.to_le_bytes());
    for text in [session, key] {
        bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    }
    Ok(bytes)
}

pub(crate) fn release(
    generation: u64,
    session: &str,
    sequence: &ReleaseSequence,
) -> Result<Vec<u8>, String> {
    prefix(
        generation,
        session,
        &sequence.key,
        sequence.id,
        sequence.incarnation,
        sequence.operation_id,
    )
}

pub(crate) fn settlement(
    generation: u64,
    session: &str,
    sequence: &SettlementSequence,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let prefix = prefix(
        generation,
        session,
        &sequence.key,
        sequence.id,
        sequence.incarnation,
        sequence.operation_id,
    )?;
    let mut body = prefix.clone();
    let count =
        u32::try_from(sequence.replay_tokens.len()).map_err(|_| "native replay length overflow")?;
    for value in [sequence.retain_from, sequence.replay_position, count] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for token in &sequence.replay_tokens {
        body.extend_from_slice(&token.to_le_bytes());
    }
    Ok((prefix, body))
}

pub(crate) fn settlement_reply(prefix: &[u8], response: &[u8]) -> Result<Vec<i32>, String> {
    if !response.starts_with(prefix) {
        return Err("physical settlement identity echo changed".into());
    }
    let body = &response[prefix.len()..];
    if body.len() < 4 {
        return Err("physical settlement result is truncated".into());
    }
    let count = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    if count.checked_mul(4).and_then(|n| n.checked_add(4)) != Some(body.len()) {
        return Err("physical settlement result length is invalid".into());
    }
    Ok(body[4..]
        .chunks_exact(4)
        .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn echo_is_exact_and_old_bare_control_has_no_valid_identity() {
        let id = prefix(7, "session", "session\0request", 3, 11, 13).unwrap();
        assert_eq!(&id[8..16], &7u64.to_le_bytes());
        assert_eq!(&id[32..36], &3u32.to_le_bytes());
        let mut response = id.clone();
        response.extend_from_slice(&1u32.to_le_bytes());
        response.extend_from_slice(&17i32.to_le_bytes());
        assert_eq!(settlement_reply(&id, &response), Ok(vec![17]));
        for index in [8, 16, 24, 32, 40] {
            let mut bad = response.clone();
            bad[index] ^= 1;
            assert!(settlement_reply(&id, &bad).is_err());
        }
        assert!(settlement_reply(&id, b"SEQUENCE_RELEASED").is_err());
        response.push(0);
        assert!(settlement_reply(&id, &response).is_err());
        assert!(prefix(7, "session", "session\0request", 3, 0, 13).is_err());
    }

    #[test]
    fn control_key_is_canonical_and_required_fields_cannot_default_from_old_wire() {
        for (session, key) in [
            ("session", "another\0request"),
            ("session", "session\0"),
            ("session", "session\0request\0suffix"),
            ("ses\0sion", "ses\0sion\0request"),
        ] {
            assert!(prefix(1, session, key, 0, 1, 1).is_err());
        }
        let valid = serde_json::json!({ "key":"session\0request", "id":0, "incarnation":1,
            "operation_id":1, "retain_from":0, "replay_position":0 });
        for field in ["incarnation", "operation_id"] {
            let mut old = valid.clone();
            old.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<ReleaseSequence>(old.clone()).is_err());
            assert!(serde_json::from_value::<SettlementSequence>(old).is_err());
        }
        let mut command = crate::v2::tests::request_state(vec![7]).command.clone();
        command.session_id = "s".into();
        command.request_id = "r".repeat(4094);
        assert!(
            command.validate().is_ok(),
            "exactly 4096 canonical key bytes fit native"
        );
        command.request_id.push('r');
        assert!(
            command.validate().is_err(),
            "refuse before accepting a key that can never be issued"
        );
    }
}
