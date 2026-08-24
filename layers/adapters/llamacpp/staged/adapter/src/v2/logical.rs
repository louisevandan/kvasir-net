use super::{Phase, RowOwner};

const MAGIC: &[u8; 4] = b"P4LB";
const VERSION: u16 = 2;
const MAX_ROWS: usize = 65_536;
const MAX_STRING: usize = 4_096;

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
            put_string(&mut output, &row.owner.request_id)?;
            put_string(&mut output, &row.owner.sequence_key)?;
            put_string(&mut output, &row.owner.session_id)?;
            put_string(&mut output, &row.owner.reply)?;
            put_u32(&mut output, row.owner.sequence_id);
            output.push(match row.owner.phase {
                Phase::Prefill => 0,
                Phase::Decode => 1,
            });
            output.push(u8::from(row.owner.output));
            put_u16(&mut output, 0);
            put_u32(&mut output, row.owner.position);
            put_u32(&mut output, row.owner.max_tokens);
            put_u32(&mut output, row.owner.generated_tokens);
            put_i32(&mut output, row.token);
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
            let request_id = cursor.string()?;
            let sequence_key = cursor.string()?;
            let session_id = cursor.string()?;
            let reply = cursor.string()?;
            let sequence_id = cursor.u32()?;
            let phase = match cursor.byte()? {
                0 => Phase::Prefill,
                1 => Phase::Decode,
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
            let options = cursor.string()?;
            let row = LogicalRow {
                owner: RowOwner {
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
    if owner.request_id.is_empty()
        || owner.sequence_key.is_empty()
        || owner.session_id.is_empty()
        || owner.reply.is_empty()
        || owner.request_id.len() > MAX_STRING
        || owner.sequence_key.len() > MAX_STRING
        || owner.session_id.len() > MAX_STRING
        || owner.reply.len() > MAX_STRING
        || owner.options.len() > MAX_STRING
        || owner.max_tokens == 0
        || owner.generated_tokens >= owner.max_tokens
        || owner.position > i32::MAX as u32
    {
        return Err(LogicalBatchError::InvalidRow);
    }
    Ok(())
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(output: &mut Vec<u8>, value: u32) {
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
