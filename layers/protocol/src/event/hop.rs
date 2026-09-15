//! Backend-neutral acknowledged delivery frames around canonical P4 Events.
//!
//! `P4H1` is carried inside the existing u32-length socket frame. A legacy
//! peer sees a non-Event body and fails closed before broker or adapter intake.
use crate::ProtocolError;
use sha2::{Digest as _, Sha256};

const MAGIC: [u8; 4] = *b"P4H1";
pub const VERSION: u16 = 1;
pub const MAX_ID_BYTES: usize = 4096;
pub const MAX_DETAIL_BYTES: usize = 4096;

pub type EventDigest = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptStatus {
    AcceptedExact,
    Rejected,
    Conflict,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HopFrame {
    Hello {
        sender_id: String,
        connection_generation: u64,
        max_outstanding: u32,
        max_receipt_bytes: u64,
    },
    HelloAck {
        accepted_connection_generation: u64,
        sender_id: String,
        connection_generation: u64,
        max_outstanding: u32,
        max_receipt_bytes: u64,
    },
    Data {
        attempt: u64,
        digest: EventDigest,
        event: Vec<u8>,
    },
    Receipt {
        attempt: u64,
        digest: EventDigest,
        status: ReceiptStatus,
        detail: String,
    },
    ReceiptAck {
        sender_id: String,
        connection_generation: u64,
        attempt: u64,
        digest: EventDigest,
    },
    Query {
        sender_id: String,
        connection_generation: u64,
        attempt: u64,
        digest: EventDigest,
    },
    QueryResult {
        attempt: u64,
        digest: EventDigest,
        status: ReceiptStatus,
        detail: String,
    },
}

pub fn event_digest(bytes: &[u8]) -> EventDigest {
    Sha256::digest(bytes).into()
}

pub fn encode(frame: &HopFrame) -> Result<Vec<u8>, ProtocolError> {
    validate(frame)?;
    let mut out = Vec::with_capacity(96);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    match frame {
        HopFrame::Hello {
            sender_id,
            connection_generation,
            max_outstanding,
            max_receipt_bytes,
        } => {
            out.push(1);
            put_text(&mut out, sender_id, MAX_ID_BYTES)?;
            put_u64(&mut out, *connection_generation);
            put_u32(&mut out, *max_outstanding);
            put_u64(&mut out, *max_receipt_bytes);
        }
        HopFrame::HelloAck {
            accepted_connection_generation,
            sender_id,
            connection_generation,
            max_outstanding,
            max_receipt_bytes,
        } => {
            out.push(2);
            put_u64(&mut out, *accepted_connection_generation);
            put_text(&mut out, sender_id, MAX_ID_BYTES)?;
            put_u64(&mut out, *connection_generation);
            put_u32(&mut out, *max_outstanding);
            put_u64(&mut out, *max_receipt_bytes);
        }
        HopFrame::Data {
            attempt,
            digest,
            event,
        } => {
            if event_digest(event) != *digest {
                return Err(ProtocolError::new("hop data digest mismatch"));
            }
            out.push(3);
            put_u64(&mut out, *attempt);
            out.extend_from_slice(digest);
            let size = u32::try_from(event.len())
                .map_err(|_| ProtocolError::new("hop event too large"))?;
            put_u32(&mut out, size);
            out.extend_from_slice(event);
        }
        HopFrame::Receipt {
            attempt,
            digest,
            status,
            detail,
        } => {
            out.push(4);
            put_u64(&mut out, *attempt);
            out.extend_from_slice(digest);
            out.push(status_tag(*status));
            put_text(&mut out, detail, MAX_DETAIL_BYTES)?;
        }
        HopFrame::ReceiptAck {
            sender_id,
            connection_generation,
            attempt,
            digest,
        } => {
            out.push(5);
            put_text(&mut out, sender_id, MAX_ID_BYTES)?;
            put_u64(&mut out, *connection_generation);
            put_u64(&mut out, *attempt);
            out.extend_from_slice(digest);
        }
        HopFrame::Query {
            sender_id,
            connection_generation,
            attempt,
            digest,
        } => {
            out.push(6);
            put_text(&mut out, sender_id, MAX_ID_BYTES)?;
            put_u64(&mut out, *connection_generation);
            put_u64(&mut out, *attempt);
            out.extend_from_slice(digest);
        }
        HopFrame::QueryResult {
            attempt,
            digest,
            status,
            detail,
        } => {
            out.push(7);
            put_u64(&mut out, *attempt);
            out.extend_from_slice(digest);
            out.push(status_tag(*status));
            put_text(&mut out, detail, MAX_DETAIL_BYTES)?;
        }
    }
    Ok(out)
}

/// `Ok(None)` identifies a canonical legacy Event body. A `P4H1` prefix with
/// invalid fields is always an error and must not fall back to Event decoding.
pub fn decode(bytes: &[u8]) -> Result<Option<HopFrame>, ProtocolError> {
    if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
        return Ok(None);
    }
    let mut cursor = Cursor {
        bytes: &bytes[MAGIC.len()..],
        offset: 0,
    };
    if cursor.u16()? != VERSION {
        return Err(ProtocolError::new("unsupported hop frame version"));
    }
    if cursor.u16()? != 0 {
        return Err(ProtocolError::new("hop frame reserved bits are nonzero"));
    }
    let frame = match cursor.byte()? {
        1 => HopFrame::Hello {
            sender_id: cursor.text(MAX_ID_BYTES)?,
            connection_generation: cursor.u64()?,
            max_outstanding: cursor.u32()?,
            max_receipt_bytes: cursor.u64()?,
        },
        2 => HopFrame::HelloAck {
            accepted_connection_generation: cursor.u64()?,
            sender_id: cursor.text(MAX_ID_BYTES)?,
            connection_generation: cursor.u64()?,
            max_outstanding: cursor.u32()?,
            max_receipt_bytes: cursor.u64()?,
        },
        3 => {
            let attempt = cursor.u64()?;
            let digest = cursor.digest()?;
            let size = cursor.u32()? as usize;
            let event = cursor.take(size)?.to_vec();
            if event_digest(&event) != digest {
                return Err(ProtocolError::new("hop data digest mismatch"));
            }
            HopFrame::Data {
                attempt,
                digest,
                event,
            }
        }
        4 => HopFrame::Receipt {
            attempt: cursor.u64()?,
            digest: cursor.digest()?,
            status: decode_status(cursor.byte()?)?,
            detail: cursor.text(MAX_DETAIL_BYTES)?,
        },
        5 => HopFrame::ReceiptAck {
            sender_id: cursor.text(MAX_ID_BYTES)?,
            connection_generation: cursor.u64()?,
            attempt: cursor.u64()?,
            digest: cursor.digest()?,
        },
        6 => HopFrame::Query {
            sender_id: cursor.text(MAX_ID_BYTES)?,
            connection_generation: cursor.u64()?,
            attempt: cursor.u64()?,
            digest: cursor.digest()?,
        },
        7 => HopFrame::QueryResult {
            attempt: cursor.u64()?,
            digest: cursor.digest()?,
            status: decode_status(cursor.byte()?)?,
            detail: cursor.text(MAX_DETAIL_BYTES)?,
        },
        _ => return Err(ProtocolError::new("unknown hop frame kind")),
    };
    if !cursor.finished() {
        return Err(ProtocolError::new("trailing hop frame bytes"));
    }
    validate(&frame)?;
    Ok(Some(frame))
}

fn validate(frame: &HopFrame) -> Result<(), ProtocolError> {
    match frame {
        HopFrame::Hello {
            sender_id,
            connection_generation,
            max_outstanding,
            max_receipt_bytes,
        } if sender_id.is_empty()
            || *connection_generation == 0
            || *max_outstanding == 0
            || *max_receipt_bytes == 0 =>
        {
            Err(ProtocolError::new(
                "hop hello requires positive identity and limits",
            ))
        }
        HopFrame::HelloAck {
            accepted_connection_generation,
            sender_id,
            connection_generation,
            max_outstanding,
            max_receipt_bytes,
        } if *accepted_connection_generation == 0
            || sender_id.is_empty()
            || *connection_generation == 0
            || *max_outstanding == 0
            || *max_receipt_bytes == 0 =>
        {
            Err(ProtocolError::new(
                "hop hello ACK requires positive generation and limits",
            ))
        }
        HopFrame::Data { attempt, event, .. } if *attempt == 0 || event.is_empty() => Err(
            ProtocolError::new("hop data requires an attempt and Event bytes"),
        ),
        HopFrame::Receipt {
            attempt, detail, ..
        }
        | HopFrame::QueryResult {
            attempt, detail, ..
        } if *attempt == 0 || detail.len() > MAX_DETAIL_BYTES => Err(ProtocolError::new(
            "hop receipt requires an attempt and bounded detail",
        )),
        HopFrame::ReceiptAck {
            sender_id,
            connection_generation,
            attempt,
            ..
        } if sender_id.is_empty() || *connection_generation == 0 || *attempt == 0 => Err(
            ProtocolError::new("hop receipt ACK requires identity, generation and attempt"),
        ),
        HopFrame::Query {
            sender_id,
            connection_generation,
            attempt,
            ..
        } if sender_id.is_empty() || *connection_generation == 0 || *attempt == 0 => Err(
            ProtocolError::new("hop query requires identity, generation and attempt"),
        ),
        _ => Ok(()),
    }
}

fn status_tag(status: ReceiptStatus) -> u8 {
    match status {
        ReceiptStatus::AcceptedExact => 1,
        ReceiptStatus::Rejected => 2,
        ReceiptStatus::Conflict => 3,
        ReceiptStatus::Unknown => 4,
    }
}
fn decode_status(value: u8) -> Result<ReceiptStatus, ProtocolError> {
    match value {
        1 => Ok(ReceiptStatus::AcceptedExact),
        2 => Ok(ReceiptStatus::Rejected),
        3 => Ok(ReceiptStatus::Conflict),
        4 => Ok(ReceiptStatus::Unknown),
        _ => Err(ProtocolError::new("unknown hop receipt status")),
    }
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_text(out: &mut Vec<u8>, value: &str, limit: usize) -> Result<(), ProtocolError> {
    if value.len() > limit {
        return Err(ProtocolError::new("hop text field too large"));
    }
    put_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| ProtocolError::new("hop frame length overflow"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProtocolError::new("truncated hop frame"))?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn digest(&mut self) -> Result<EventDigest, ProtocolError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
    fn text(&mut self, limit: usize) -> Result<String, ProtocolError> {
        let count = self.u32()? as usize;
        if count > limit {
            return Err(ProtocolError::new("hop text field too large"));
        }
        String::from_utf8(self.take(count)?.to_vec())
            .map_err(|_| ProtocolError::new("hop text field is not UTF-8"))
    }
    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests;
