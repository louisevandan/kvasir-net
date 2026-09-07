use super::capsule::{MAX_STRING, validate_reply_options};
use super::{Phase, RowOwner};

const MAGIC: &[u8; 4] = b"P4LB";
const VERSION: u16 = 4;
const MAX_ROWS: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalRow {
    pub owner: RowOwner,
    pub token: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalBatch(pub Vec<LogicalRow>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogicalBatchError {
    Truncated,
    TrailingBytes,
    InvalidMagic,
    UnsupportedVersion(u16),
    InvalidRow,
    InvalidUtf8,
    LimitExceeded,
}

impl LogicalBatch {
    pub fn encode(&self) -> Result<Vec<u8>, LogicalBatchError> {
        if self.0.is_empty() || self.0.len() > MAX_ROWS {
            return Err(LogicalBatchError::LimitExceeded);
        }
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        put_u16(&mut output, VERSION);
        put_u16(&mut output, 0);
        put_u32(&mut output, self.0.len() as u32);
        for row in &self.0 {
            validate(row)?;
            put_u64(&mut output, row.owner.load_generation);
            put_u64(&mut output, row.owner.incarnation);
            put_string(&mut output, &row.owner.request_id)?;
            put_string(&mut output, &row.owner.sequence_key)?;
            put_string(&mut output, &row.owner.session_id)?;
            put_string(&mut output, &row.owner.reply)?;
            put_u32(&mut output, row.owner.sequence_id);
            output.push(match row.owner.phase {
                Phase::Prefill => 0,
                Phase::Decode => 1,
                Phase::Verify => 2,
                Phase::Replay => 3,
            });
            output.push(u8::from(row.owner.output));
            put_u16(&mut output, 0);
            put_u32(&mut output, row.owner.position);
            put_u32(&mut output, row.owner.max_tokens);
            put_u32(&mut output, row.owner.generated_tokens);
            put_i32(&mut output, row.token);
            put_u64(&mut output, row.owner.speculative_id);
            put_u32(&mut output, row.owner.speculative_index);
            put_u32(&mut output, row.owner.speculative_count);
            put_string(&mut output, &row.owner.options)?;
        }
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, LogicalBatchError> {
        let mut cursor = Cursor { bytes, offset: 0 };
        if cursor.take(4)? != MAGIC {
            return Err(LogicalBatchError::InvalidMagic);
        }
        let version = cursor.u16()?;
        if version != VERSION {
            return Err(LogicalBatchError::UnsupportedVersion(version));
        }
        if cursor.u16()? != 0 {
            return Err(LogicalBatchError::InvalidRow);
        }
        let count = cursor.u32()? as usize;
        if count == 0 || count > MAX_ROWS {
            return Err(LogicalBatchError::LimitExceeded);
        }
        let mut rows = Vec::with_capacity(count);
        for _ in 0..count {
            let load_generation = cursor.u64()?;
            let incarnation = cursor.u64()?;
            let request_id = cursor.string()?;
            let sequence_key = cursor.string()?;
            let session_id = cursor.string()?;
            let reply = cursor.string()?;
            let sequence_id = cursor.u32()?;
            let phase = match cursor.byte()? {
                0 => Phase::Prefill,
                1 => Phase::Decode,
                2 => Phase::Verify,
                3 => Phase::Replay,
                _ => return Err(LogicalBatchError::InvalidRow),
            };
            let output = match cursor.byte()? {
                0 => false,
                1 => true,
                _ => return Err(LogicalBatchError::InvalidRow),
            };
            if cursor.u16()? != 0 {
                return Err(LogicalBatchError::InvalidRow);
            }
            let position = cursor.u32()?;
            let max_tokens = cursor.u32()?;
            let generated_tokens = cursor.u32()?;
            let token = cursor.i32()?;
            let speculative_id = cursor.u64()?;
            let speculative_index = cursor.u32()?;
            let speculative_count = cursor.u32()?;
            let options = cursor.string()?;
            let row = LogicalRow {
                owner: RowOwner {
                    load_generation,
                    incarnation,
                    request_id,
                    sequence_key,
                    session_id,
                    reply,
                    sequence_id,
                    phase,
                    position,
                    max_tokens,
                    generated_tokens,
                    output,
                    input_token: token,
                    speculative_id,
                    speculative_index,
                    speculative_count,
                    options,
                },
                token,
            };
            validate(&row)?;
            rows.push(row);
        }
        if cursor.offset != bytes.len() {
            return Err(LogicalBatchError::TrailingBytes);
        }
        Ok(Self(rows))
    }
}

fn validate(row: &LogicalRow) -> Result<(), LogicalBatchError> {
    let owner = &row.owner;
    let speculative = matches!(owner.phase, Phase::Verify | Phase::Replay);
    if owner.load_generation == 0
        || owner.incarnation == 0
        || !owner.has_canonical_request_identity()
        || validate_reply_options(&owner.reply, &owner.options).is_err()
        || owner.request_id.len() > MAX_STRING
        || owner.sequence_key.len() > MAX_STRING
        || owner.session_id.len() > MAX_STRING
        || owner.max_tokens == 0
        || owner.generated_tokens >= owner.max_tokens
        || owner.position > i32::MAX as u32
        || owner.input_token != row.token
        || (speculative
            && (owner.speculative_id == 0
                || owner.speculative_count == 0
                || owner.speculative_index >= owner.speculative_count))
        || (!speculative
            && (owner.speculative_id != 0
                || owner.speculative_index != 0
                || owner.speculative_count != 0))
        || (owner.phase == Phase::Verify && !owner.output)
        || (owner.phase == Phase::Replay && owner.output)
    {
        return Err(LogicalBatchError::InvalidRow);
    }
    Ok(())
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod identity_wire_tests {
    use super::*;
    use crate::v2::{CapsuleSet, GeneratedToken, Invocation, PhysicalCapsule, PhysicalOutcome};

    fn row() -> LogicalRow {
        LogicalRow {
            token: 42,
            owner: RowOwner {
                load_generation: 1,
                incarnation: 9,
                request_id: "request-1".into(),
                sequence_key: "pipeline-a\0request-1".into(),
                session_id: "pipeline-a".into(),
                reply: "reply-1".into(),
                sequence_id: 3,
                phase: Phase::Decode,
                position: 9,
                max_tokens: 500,
                generated_tokens: 4,
                output: true,
                input_token: 42,
                speculative_id: 0,
                speculative_index: 0,
                speculative_count: 0,
                options: "{}".into(),
            },
        }
    }

    #[test]
    fn logical_v4_binds_a_nonzero_incarnation_at_the_agreed_offset() {
        let expected = LogicalBatch(vec![row()]);
        let mut bytes = expected.encode().unwrap();
        assert_eq!(&bytes[4..6], &4u16.to_le_bytes());
        assert_eq!(&bytes[20..28], &9u64.to_le_bytes());
        assert_eq!(LogicalBatch::decode(&bytes).unwrap(), expected);
        bytes[4] = 3;
        assert!(matches!(
            LogicalBatch::decode(&bytes),
            Err(LogicalBatchError::UnsupportedVersion(3))
        ));
        bytes[4] = 4;
        bytes[20..28].fill(0);
        assert!(LogicalBatch::decode(&bytes).is_err());
        let mut invalid = expected;
        invalid.0[0].owner.incarnation = 0;
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn physical_v4_preserves_incarnation_and_refuses_legacy_or_zero_identity() {
        let owner = row().owner;
        let expected = CapsuleSet(vec![PhysicalCapsule {
            execution_id: 7,
            terminal: true,
            invocation: Invocation {
                flags: 0,
                n_seq_tokens: 1,
                n_seqs: 1,
                n_seqs_unq: 1,
                n_pos: 1,
                positions: vec![9],
                sequence_counts: vec![1],
                sequence_ids: vec![3],
                output: vec![true],
            },
            owners: vec![owner],
            tensors: Vec::new(),
            outcomes: vec![PhysicalOutcome {
                owner_index: 0,
                generated: vec![GeneratedToken {
                    token: 99,
                    text: "ok".into(),
                    position: 10,
                    stop: None,
                }],
                proposal: Vec::new(),
                retain_from: None,
                replay_tokens: Vec::new(),
                replay_position: 0,
            }],
        }]);
        let mut bytes = expected.encode().unwrap();
        assert_eq!(&bytes[4..6], &4u16.to_le_bytes());
        assert_eq!(CapsuleSet::decode(&bytes).unwrap(), expected);
        bytes[4] = 3;
        assert!(CapsuleSet::decode(&bytes).is_err());
        let mut invalid = expected;
        invalid.0[0].owners[0].incarnation = 0;
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn both_row_codecs_bind_the_exact_session_and_request_names() {
        use crate::v2::CapsuleError;
        let good = row();
        let capsule = |owner: RowOwner| PhysicalCapsule {
            execution_id: 7,
            terminal: true,
            invocation: Invocation {
                flags: 0,
                n_seq_tokens: 1,
                n_seqs: 1,
                n_seqs_unq: 1,
                n_pos: 1,
                positions: vec![9],
                sequence_counts: vec![1],
                sequence_ids: vec![3],
                output: vec![true],
            },
            owners: vec![owner],
            tensors: Vec::new(),
            outcomes: Vec::new(),
        };
        assert!(capsule(good.owner.clone()).validate().is_ok());
        let mut bad = Vec::new();
        for key in [
            "different\0request-1",
            "pipeline-a\0",
            "pipeline-a\0request-1\0suffix",
            "sequence-1",
        ] {
            let mut candidate = good.clone();
            candidate.owner.sequence_key = key.into();
            bad.push(candidate);
        }
        let mut candidate = good.clone();
        candidate.owner.request_id = "request-2".into();
        bad.push(candidate);
        let mut candidate = good.clone();
        candidate.owner.session_id = "pipe\0line".into();
        candidate.owner.sequence_key = "pipe\0line\0request-1".into();
        bad.push(candidate);
        for candidate in bad {
            assert_eq!(
                LogicalBatch(vec![candidate.clone()]).encode(),
                Err(LogicalBatchError::InvalidRow)
            );
            assert_eq!(
                capsule(candidate.owner).validate(),
                Err(CapsuleError::InvalidOwner)
            );
        }
        // Mutate bytes only after a valid encode, so decode refusal is tested
        // independently of the encoder. First session-prefix byte in the key.
        let mut wire = LogicalBatch(vec![good.clone()]).encode().unwrap();
        let key_at = 28 + 2 + good.owner.request_id.len() + 2;
        wire[key_at] = b'x';
        assert_eq!(
            LogicalBatch::decode(&wire),
            Err(LogicalBatchError::InvalidRow)
        );
        wire[key_at] = 0xff;
        assert_eq!(
            LogicalBatch::decode(&wire),
            Err(LogicalBatchError::InvalidUtf8)
        );
    }

    #[test]
    fn reply_and_options_wire_limits_preserve_exact_utf8_bytes() {
        // Literal contract boundary, independent of the production constant.
        for size in [4_095, 4_096, 4_097] {
            for options in [false, true] {
                for multibyte in [false, true] {
                    let text = if multibyte {
                        let mut value = "한".repeat(size / 3);
                        value.push_str(&"x".repeat(size % 3));
                        value
                    } else {
                        "x".repeat(size)
                    };
                    assert_eq!(text.len(), size);
                    let mut logical = row();
                    if options {
                        logical.owner.options = text;
                    } else {
                        logical.owner.reply = text;
                    }
                    let physical = CapsuleSet(vec![PhysicalCapsule {
                        execution_id: 7,
                        terminal: true,
                        invocation: Invocation {
                            flags: 0,
                            n_seq_tokens: 1,
                            n_seqs: 1,
                            n_seqs_unq: 1,
                            n_pos: 1,
                            positions: vec![9],
                            sequence_counts: vec![1],
                            sequence_ids: vec![3],
                            output: vec![true],
                        },
                        owners: vec![logical.owner.clone()],
                        tensors: Vec::new(),
                        outcomes: Vec::new(),
                    }]);
                    let logical = LogicalBatch(vec![logical]);
                    if size <= 4_096 {
                        assert_eq!(
                            LogicalBatch::decode(&logical.encode().unwrap()).unwrap(),
                            logical
                        );
                        assert_eq!(
                            CapsuleSet::decode(&physical.encode().unwrap()).unwrap(),
                            physical
                        );
                    } else {
                        assert_eq!(logical.encode(), Err(LogicalBatchError::InvalidRow));
                        assert_eq!(
                            physical.0[0].validate(),
                            Err(crate::v2::CapsuleError::InvalidOwner)
                        );
                        assert!(physical.encode().is_err());
                    }
                }
            }
        }
        let mut empty_options = row();
        empty_options.owner.options.clear();
        assert!(LogicalBatch(vec![empty_options.clone()]).encode().is_ok());
        empty_options.owner.reply.clear();
        assert_eq!(
            LogicalBatch(vec![empty_options]).encode(),
            Err(LogicalBatchError::InvalidRow)
        );
    }
}
fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}
fn put_i32(output: &mut Vec<u8>, value: i32) {
    output.extend_from_slice(&value.to_le_bytes());
}
fn put_string(output: &mut Vec<u8>, value: &str) -> Result<(), LogicalBatchError> {
    if value.len() > MAX_STRING {
        return Err(LogicalBatchError::LimitExceeded);
    }
    put_u16(output, value.len() as u16);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], LogicalBatchError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(LogicalBatchError::LimitExceeded)?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or(LogicalBatchError::Truncated)?;
        self.offset = end;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8, LogicalBatchError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, LogicalBatchError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, LogicalBatchError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, LogicalBatchError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, LogicalBatchError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, LogicalBatchError> {
        let size = self.u16()? as usize;
        std::str::from_utf8(self.take(size)?)
            .map(str::to_owned)
            .map_err(|_| LogicalBatchError::InvalidUtf8)
    }
}
