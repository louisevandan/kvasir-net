//! One pass of execution over a window of sequences.
//!
//! The execution shape, which is the part under pressure from throughput work
//! and therefore kept away from lifecycle.

use crate::work::{DeploymentId, SequenceId};

/// The batch is the point. A cohort window is the shape the runtime actually
/// works in, so a hop that could only carry one sequence would force the node
/// to simulate batching above the boundary, which is exactly where measurement
/// showed a throttle does not belong.
///
/// A hop means the same thing under either distribution. On a staged backend
/// it advances this node's layer range and the next node continues; on an
/// internal one it advances the whole model. Neither case puts hidden state in
/// this interface — that transfer stays inside the backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hop {
    /// Immutable identity for one adapter execution. Events must echo it so
    /// a late completion cannot be applied to a newer batch.
    pub id: u64,
    pub deployment: DeploymentId,
    pub phase: Phase,
    /// The window this hop covers. Its size is the node's decision, bounded by
    /// what the load declared.
    pub sequences: Vec<Sequence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The first pass over a prompt.
    Prefill,
    /// One further step. On a staged chain a lap of the ring produces one
    /// token per sequence; on an internal backend the hop does that itself.
    Decode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequence {
    pub sequence: SequenceId,
    /// Opaque tensor cut-set entering this sequence's staged hop.
    /// `None` is the compatibility path for Internal and served adapters.
    pub inbound_cut_set: Option<Vec<u8>>,
    /// Position reached so far. A decode hop continues from here.
    pub position: u32,
    /// Present when this node begins the work — the chain's first node, or the
    /// only node on an internal backend. Absent on a later stage, which
    /// continues from state it already holds rather than from text.
    pub prompt: Option<String>,
    /// Explicit token input for a staged stage-0 decode. Other adapters leave
    /// this absent because they own their autoregressive loop internally.
    pub initial_tokens: Option<Vec<i32>>,
    /// How many tokens this sequence still wants. Zero ends it.
    pub remaining: u32,
    /// Opaque sampling and generation options. Passed through whole; an
    /// adapter that cannot honour a key must say so rather than drop it.
    pub options: String,
}

const CONTINUATION_MAGIC: [u8; 8] = *b"P4CUT01\0";

pub fn is_continuation(bytes: &[u8]) -> bool {
    bytes.len() >= CONTINUATION_MAGIC.len()
        && bytes[..CONTINUATION_MAGIC.len()] == CONTINUATION_MAGIC
}

/// Carries an opaque adapter cut-set without discarding the P4 sequence
/// context needed by the next node to reconstruct the same request.
///
/// The adapter bytes are never interpreted here. The second body is the
/// original node payload and remains opaque to this crate as well.
pub fn encode_continuation(cut_set: &[u8], original_body: &[u8]) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(CONTINUATION_MAGIC.len() + 8 + cut_set.len() + original_body.len());
    bytes.extend_from_slice(&CONTINUATION_MAGIC);
    bytes.extend_from_slice(&(cut_set.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(original_body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(cut_set);
    bytes.extend_from_slice(original_body);
    bytes
}

pub fn decode_continuation(bytes: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if !is_continuation(bytes) || bytes.len() < CONTINUATION_MAGIC.len() + 8 {
        return None;
    }
    let cut_len = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
    let body_len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    let start = 16usize;
    let cut_end = start.checked_add(cut_len)?;
    let body_end = cut_end.checked_add(body_len)?;
    (body_end == bytes.len()).then(|| {
        (
            bytes[start..cut_end].to_vec(),
            bytes[cut_end..body_end].to_vec(),
        )
    })
}

impl Hop {
    /// A hop covering nothing is not a hop. Callers check this rather than
    /// discovering it inside a backend.
    pub fn is_empty(&self) -> bool {
        self.sequences.is_empty()
    }

    pub fn width(&self) -> usize {
        self.sequences.len()
    }
}

#[cfg(test)]
mod tests;
