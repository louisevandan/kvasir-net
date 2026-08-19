//! One transmission: a header, an envelope, and a body nobody in between reads.
//!
//! The body's length is in the header beside the envelope's, so a relay can
//! forward a frame by copying bytes it never decoded. That is the property the
//! whole relay design rests on — forwarding costs the same whatever the
//! message is, and a new message kind cannot make a relay heavier.

use crate::envelope::wire;
use crate::{Envelope, ProtocolError};

pub const MAGIC: [u8; 4] = *b"P4B1";
pub const VERSION: u8 = 7;

const HEADER_BYTES: usize = 16;
const MAX_ENVELOPE_BYTES: usize = 256 * 1024;
// A 5k-token staged prefill can carry a tens-of-megabytes hidden-state
// cut-set. Keep the outer P4 frame limit aligned with the staged local wire
// limit; the latter already bounds allocation and validates the payload.
const MAX_BODY_BYTES: usize = 128 * 1024 * 1024;

/// A frame still in its wire form. The envelope is decoded because every hop
/// needs it; the body is not, because only its destination does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub envelope: Envelope,
    pub body: Vec<u8>,
}

pub fn encode(envelope: &Envelope, body: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let encoded = wire::encode(envelope)?;
    if encoded.len() > MAX_ENVELOPE_BYTES {
        return Err(ProtocolError::new("envelope too large"));
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(ProtocolError::new("body too large"));
    }
    let mut frame = Vec::with_capacity(HEADER_BYTES + encoded.len() + body.len());
    frame.extend_from_slice(&MAGIC);
    frame.push(VERSION);
    frame.extend_from_slice(&[0, 0, 0]);
    frame.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&encoded);
    frame.extend_from_slice(body);
    Ok(frame)
}

/// How many bytes the frame beginning here occupies, read from its header
/// alone. A reader uses this to know when it has a whole frame without
/// decoding any of it.
pub fn frame_len(header: &[u8]) -> Result<usize, ProtocolError> {
    if header.len() < HEADER_BYTES {
        return Err(ProtocolError::new("truncated frame header"));
    }
    if header[..4] != MAGIC {
        return Err(ProtocolError::new("frame magic mismatch"));
    }
    if header[4] != VERSION {
        return Err(ProtocolError::new(format!(
            "frame version {} is not {VERSION}",
            header[4]
        )));
    }
    let envelope = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    let body = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    if envelope > MAX_ENVELOPE_BYTES || body > MAX_BODY_BYTES {
        return Err(ProtocolError::new("frame declares an impossible length"));
    }
    Ok(HEADER_BYTES + envelope + body)
}

pub fn decode(frame: &[u8]) -> Result<Frame, ProtocolError> {
    let total = frame_len(frame)?;
    if frame.len() != total {
        return Err(ProtocolError::new("frame length does not match its header"));
    }
    let envelope_len = u32::from_le_bytes(frame[8..12].try_into().unwrap()) as usize;
    let envelope = wire::decode(&frame[HEADER_BYTES..HEADER_BYTES + envelope_len])?;
    Ok(Frame {
        envelope,
        body: frame[HEADER_BYTES + envelope_len..].to_vec(),
    })
}

/// Rewrites a frame's envelope while leaving its body untouched.
///
/// This is what a hop does: the same work travels on with a new target, and
/// the body is moved rather than re-encoded, so a chain of any length costs
/// one copy per hop.
pub fn reseal(envelope: &Envelope, body: Vec<u8>) -> Result<Vec<u8>, ProtocolError> {
    encode(envelope, &body)
}

#[cfg(test)]
mod tests;
