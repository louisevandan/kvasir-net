use super::Phase;
mod cursor;
mod validate;
use cursor::Cursor;

const MAGIC: &[u8; 4] = b"P4PB";
const VERSION: u16 = 2;
const MAX_ROWS: usize = 65_536;
const MAX_TENSORS: usize = 16_384;
const MAX_CAPSULES: usize = 65_536;
const MAX_STRING: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub flags: u32,
    pub n_seq_tokens: u32,
    pub n_seqs: u32,
    pub n_seqs_unq: u32,
    pub n_pos: u32,
    pub positions: Vec<i32>,
    pub sequence_counts: Vec<u32>,
    pub sequence_ids: Vec<i32>,
    pub output: Vec<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowOwner {
    pub request_id: String,
    pub sequence_key: String,
    pub session_id: String,
    pub reply: String,
    pub sequence_id: u32,
    pub phase: Phase,
    pub position: u32,
    pub max_tokens: u32,
    pub generated_tokens: u32,
    pub output: bool,
    pub options: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalOutcome {
    pub owner_index: u32,
    pub token: i32,
    pub text: String,
    pub position: u32,
    pub stop: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TensorDescriptor {
    pub tensor_type: i32,
    pub dimensions: Vec<i64>,
    pub strides: Vec<u64>,
    pub nbytes: u64,
    pub view_offset: u64,
    pub alias_of: Option<u32>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tensor {
    pub descriptor: TensorDescriptor,
    /// Alias tensors carry no duplicate bytes.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalCapsule {
    pub execution_id: u64,
    pub terminal: bool,
    pub invocation: Invocation,
    pub owners: Vec<RowOwner>,
    pub tensors: Vec<Tensor>,
    pub outcomes: Vec<PhysicalOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapsuleSet(pub Vec<PhysicalCapsule>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapsuleError {
    Truncated,
    TrailingBytes,
    InvalidMagic,
    UnsupportedVersion(u16),
    InvalidInvocation,
    InvalidOwner,
    InvalidTensor,
    LimitExceeded,
    IntegerOverflow,
    InvalidUtf8,
}

impl CapsuleSet {
    pub fn encode(&self) -> Result<Vec<u8>, CapsuleError> {
        if self.0.is_empty() || self.0.len() > MAX_CAPSULES {
            return Err(CapsuleError::LimitExceeded);
        }
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u16(&mut out, VERSION);
        put_u16(&mut out, 0);
        put_u32(&mut out, self.0.len() as u32);
        for capsule in &self.0 {
            capsule.validate()?;
            put_capsule(&mut out, capsule)?;
        }
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CapsuleError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.take(4)? != MAGIC {
            return Err(CapsuleError::InvalidMagic);
        }
        let version = cursor.u16()?;
        if version != VERSION {
            return Err(CapsuleError::UnsupportedVersion(version));
        }
        if cursor.u16()? != 0 {
            return Err(CapsuleError::InvalidInvocation);
        }
        let count = cursor.u32()? as usize;
        if count == 0 || count > MAX_CAPSULES {
            return Err(CapsuleError::LimitExceeded);
        }
        let mut capsules = Vec::with_capacity(count);
        for _ in 0..count {
            capsules.push(read_capsule(&mut cursor)?);
        }
        if !cursor.done() {
            return Err(CapsuleError::TrailingBytes);
        }
        Ok(Self(capsules))
    }
}

fn put_capsule(out: &mut Vec<u8>, capsule: &PhysicalCapsule) -> Result<(), CapsuleError> {
    let invocation = &capsule.invocation;
    put_u64(out, capsule.execution_id);
    put_u32(out, u32::from(capsule.terminal));
    put_u32(out, invocation.flags);
    put_u32(out, invocation.n_seq_tokens);
    put_u32(out, invocation.n_seqs);
    put_u32(out, invocation.n_seqs_unq);
    put_u32(out, invocation.n_pos);
    put_u32(out, invocation.sequence_counts.len() as u32);
    put_u32(out, invocation.sequence_ids.len() as u32);
    put_u32(out, capsule.tensors.len() as u32);
    put_u32(out, capsule.outcomes.len() as u32);
    for value in &invocation.positions {
        put_i32(out, *value);
    }
    for value in &invocation.sequence_counts {
        put_u32(out, *value);
    }
    for value in &invocation.sequence_ids {
        put_i32(out, *value);
    }
    for value in &invocation.output {
        out.push(u8::from(*value));
    }
    for owner in &capsule.owners {
        put_string(out, &owner.request_id)?;
        put_string(out, &owner.sequence_key)?;
        put_string(out, &owner.session_id)?;
        put_string(out, &owner.reply)?;
        put_u32(out, owner.sequence_id);
        out.push(match owner.phase {
            Phase::Prefill => 0,
            Phase::Decode => 1,
        });
        out.push(u8::from(owner.output));
        put_u16(out, 0);
        put_u32(out, owner.position);
        put_u32(out, owner.max_tokens);
        put_u32(out, owner.generated_tokens);
        put_string(out, &owner.options)?;
    }
    for tensor in &capsule.tensors {
        let descriptor = &tensor.descriptor;
        put_i32(out, descriptor.tensor_type);
        out.push(descriptor.dimensions.len() as u8);
        out.extend_from_slice(&[0, 0, 0]);
        for value in &descriptor.dimensions {
            put_i64(out, *value);
        }
        for value in &descriptor.strides {
            put_u64(out, *value);
        }
        put_u64(out, descriptor.nbytes);
        put_u64(out, descriptor.view_offset);
        put_i32(out, descriptor.alias_of.map_or(-1, |value| value as i32));
        put_string(out, &descriptor.name)?;
        put_u64(out, tensor.data.len() as u64);
        out.extend_from_slice(&tensor.data);
    }
    for outcome in &capsule.outcomes {
        put_u32(out, outcome.owner_index);
        put_i32(out, outcome.token);
        put_u32(out, outcome.position);
        put_string(out, &outcome.text)?;
        put_string(out, outcome.stop.as_deref().unwrap_or(""))?;
    }
    Ok(())
}

fn read_capsule(cursor: &mut Cursor<'_>) -> Result<PhysicalCapsule, CapsuleError> {
    let execution_id = cursor.u64()?;
    let capsule_flags = cursor.u32()?;
    if capsule_flags & !1 != 0 {
        return Err(CapsuleError::InvalidInvocation);
    }
    let terminal = capsule_flags & 1 != 0;
    let flags = cursor.u32()?;
    let n_seq_tokens = cursor.u32()?;
    let n_seqs = cursor.u32()?;
    let n_seqs_unq = cursor.u32()?;
    let n_pos = cursor.u32()?;
    let rows = cursor.u32()? as usize;
    let sequence_total = cursor.u32()? as usize;
    let tensor_count = cursor.u32()? as usize;
    let outcome_count = cursor.u32()? as usize;
    if rows == 0 || rows > MAX_ROWS || tensor_count > MAX_TENSORS {
        return Err(CapsuleError::LimitExceeded);
    }
    let position_count = rows
        .checked_mul(n_pos as usize)
        .ok_or(CapsuleError::IntegerOverflow)?;
    let positions = (0..position_count)
        .map(|_| cursor.i32())
        .collect::<Result<_, _>>()?;
    let sequence_counts = (0..rows).map(|_| cursor.u32()).collect::<Result<_, _>>()?;
    let sequence_ids = (0..sequence_total)
        .map(|_| cursor.i32())
        .collect::<Result<_, _>>()?;
    let output = cursor.take(rows)?.iter().map(|value| *value != 0).collect();
    let mut owners = Vec::with_capacity(rows);
    for _ in 0..rows {
        let request_id = cursor.string()?;
        let sequence_key = cursor.string()?;
        let session_id = cursor.string()?;
        let reply = cursor.string()?;
        let sequence_id = cursor.u32()?;
        let phase = match cursor.byte()? {
            0 => Phase::Prefill,
            1 => Phase::Decode,
            _ => return Err(CapsuleError::InvalidOwner),
        };
        let owner_output = match cursor.byte()? {
            0 => false,
            1 => true,
            _ => return Err(CapsuleError::InvalidOwner),
        };
        if cursor.u16()? != 0 {
            return Err(CapsuleError::InvalidOwner);
        }
        let position = cursor.u32()?;
        let max_tokens = cursor.u32()?;
        let generated_tokens = cursor.u32()?;
        let options = cursor.string()?;
        owners.push(RowOwner {
            request_id,
            sequence_key,
            session_id,
            reply,
            sequence_id,
            phase,
            position,
            max_tokens,
            generated_tokens,
            output: owner_output,
            options,
        });
    }
    let mut tensors = Vec::with_capacity(tensor_count);
    for _ in 0..tensor_count {
        let tensor_type = cursor.i32()?;
        let dimensions_count = cursor.byte()? as usize;
        if cursor.take(3)? != [0, 0, 0] || dimensions_count == 0 || dimensions_count > 4 {
            return Err(CapsuleError::InvalidTensor);
        }
        let dimensions = (0..dimensions_count)
            .map(|_| cursor.i64())
            .collect::<Result<_, _>>()?;
        let strides = (0..dimensions_count)
            .map(|_| cursor.u64())
            .collect::<Result<_, _>>()?;
        let nbytes = cursor.u64()?;
        let view_offset = cursor.u64()?;
        let alias_raw = cursor.i32()?;
        if alias_raw < -1 {
            return Err(CapsuleError::InvalidTensor);
        }
        let name = cursor.string()?;
        let data_size =
            usize::try_from(cursor.u64()?).map_err(|_| CapsuleError::IntegerOverflow)?;
        let data = cursor.take(data_size)?.to_vec();
        tensors.push(Tensor {
            descriptor: TensorDescriptor {
                tensor_type,
                dimensions,
                strides,
                nbytes,
                view_offset,
                alias_of: (alias_raw >= 0).then_some(alias_raw as u32),
                name,
            },
            data,
        });
    }
    let mut outcomes = Vec::with_capacity(outcome_count);
    for _ in 0..outcome_count {
        let owner_index = cursor.u32()?;
        let token = cursor.i32()?;
        let position = cursor.u32()?;
        let text = cursor.string()?;
        let stop_value = cursor.string()?;
        outcomes.push(PhysicalOutcome {
            owner_index,
            token,
            text,
            position,
            stop: (!stop_value.is_empty()).then_some(stop_value),
        });
    }
    let capsule = PhysicalCapsule {
        execution_id,
        terminal,
        invocation: Invocation {
            flags,
            n_seq_tokens,
            n_seqs,
            n_seqs_unq,
            n_pos,
            positions,
            sequence_counts,
            sequence_ids,
            output,
        },
        owners,
        tensors,
        outcomes,
    };
    capsule.validate()?;
    Ok(capsule)
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_string(out: &mut Vec<u8>, value: &str) -> Result<(), CapsuleError> {
    if value.len() > MAX_STRING || value.len() > u16::MAX as usize {
        return Err(CapsuleError::LimitExceeded);
    }
    put_u16(out, value.len() as u16);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}
