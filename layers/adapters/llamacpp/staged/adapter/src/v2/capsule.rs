use super::Phase;
mod cursor;
mod decode;
mod validate;
use cursor::Cursor;
use decode::read_capsule;

const MAGIC: &[u8; 4] = b"P4PB";
const VERSION: u16 = 4;
const MAX_ROWS: usize = 65_536;
const MAX_TENSORS: usize = 16_384;
const MAX_CAPSULES: usize = 65_536;
pub(super) const MAX_STRING: usize = 4_096;

/// Bounds of the existing LB/PB v4 row strings, also enforced before admission.
/// Measure the actual serialized reply, not its unescaped source fields. This
/// is a wire check; interpretation of options remains the native parser's job.
pub(super) fn validate_reply_options(reply: &str, options: &str) -> Result<(), &'static str> {
    if reply.is_empty() || reply.len() > MAX_STRING {
        return Err("serialized reply exceeds row wire limit or is empty");
    }
    if options.len() > MAX_STRING {
        return Err("request options exceed row wire limit");
    }
    Ok(())
}

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
    pub load_generation: u64,
    pub incarnation: u64,
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
    pub input_token: i32,
    pub speculative_id: u64,
    pub speculative_index: u32,
    pub speculative_count: u32,
    pub options: String,
}

impl RowOwner {
    pub(crate) fn has_canonical_request_identity(&self) -> bool {
        !self.session_id.is_empty()
            && !self.request_id.is_empty()
            && !self.session_id.contains('\0')
            && !self.request_id.contains('\0')
            && self.sequence_key.split_once('\0')
                == Some((self.session_id.as_str(), self.request_id.as_str()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeneratedToken {
    pub token: i32,
    pub text: String,
    pub position: u32,
    pub stop: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhysicalOutcome {
    pub owner_index: u32,
    pub generated: Vec<GeneratedToken>,
    /// The already sampled base token followed by optional llama.cpp proposal tokens.
    pub proposal: Vec<i32>,
    /// Remove target memory in [retain_from, +inf) before the next proposal.
    pub retain_from: Option<u32>,
    /// Non-empty when a full-memory target must restore and replay these rows.
    pub replay_tokens: Vec<i32>,
    pub replay_position: u32,
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
        Self::decode_bounded(bytes, u64::MAX)
    }

    pub fn decode_bounded(bytes: &[u8], max_bytes: u64) -> Result<Self, CapsuleError> {
        if max_bytes == 0
            || u64::try_from(bytes.len()).map_err(|_| CapsuleError::IntegerOverflow)? > max_bytes
        {
            return Err(CapsuleError::LimitExceeded);
        }
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
        put_u64(out, owner.load_generation);
        put_u64(out, owner.incarnation);
        put_string(out, &owner.request_id)?;
        put_string(out, &owner.sequence_key)?;
        put_string(out, &owner.session_id)?;
        put_string(out, &owner.reply)?;
        put_u32(out, owner.sequence_id);
        out.push(match owner.phase {
            Phase::Prefill => 0,
            Phase::Decode => 1,
            Phase::Verify => 2,
            Phase::Replay => 3,
        });
        out.push(u8::from(owner.output));
        put_u16(out, 0);
        put_u32(out, owner.position);
        put_u32(out, owner.max_tokens);
        put_u32(out, owner.generated_tokens);
        put_i32(out, owner.input_token);
        put_u64(out, owner.speculative_id);
        put_u32(out, owner.speculative_index);
        put_u32(out, owner.speculative_count);
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
        put_u32(out, outcome.generated.len() as u32);
        put_u32(out, outcome.proposal.len() as u32);
        put_u32(out, outcome.replay_tokens.len() as u32);
        put_i32(out, outcome.retain_from.map_or(-1, |value| value as i32));
        put_u32(out, outcome.replay_position);
        for generated in &outcome.generated {
            put_i32(out, generated.token);
            put_u32(out, generated.position);
            put_string(out, &generated.text)?;
            put_string(out, generated.stop.as_deref().unwrap_or(""))?;
        }
        for token in &outcome.proposal {
            put_i32(out, *token);
        }
        for token in &outcome.replay_tokens {
            put_i32(out, *token);
        }
    }
    Ok(())
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
