//! Bounded primitive reading and writing.
//!
//! Every length is checked before it is trusted, because a decoder that
//! allocates from a number a peer chose is a decoder a peer can exhaust.

use crate::ProtocolError;

const MAX_FIELD_BYTES: usize = 256 * 1024;

/// Ceiling on a repeated field's element count. A chain names the nodes one
/// request visits, so this bounds a decoder's allocation against a hostile
/// length without constraining any plausible placement.
pub(super) const MAX_ELEMENTS: usize = 256;

pub(super) fn put_text(out: &mut Vec<u8>, value: &str) -> Result<(), ProtocolError> {
    if value.len() > MAX_FIELD_BYTES {
        return Err(ProtocolError::new("text field too large"));
    }
    put_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

pub(super) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) struct Cursor<'a> {
    pub(super) bytes: &'a [u8],
    pub(super) offset: usize,
}

impl Cursor<'_> {
    pub(super) fn byte(&mut self) -> Result<u8, ProtocolError> {
        let value = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| ProtocolError::new("truncated envelope"))?;
        self.offset += 1;
        Ok(value)
    }

    pub(super) fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    pub(super) fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    pub(super) fn text(&mut self) -> Result<String, ProtocolError> {
        let length = self.u32()? as usize;
        if length > MAX_FIELD_BYTES {
            return Err(ProtocolError::new("envelope text too large"));
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| ProtocolError::new("envelope text must be UTF-8"))
    }

    pub(super) fn take(&mut self, length: usize) -> Result<&[u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| ProtocolError::new("envelope offset overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProtocolError::new("truncated envelope"))?;
        self.offset = end;
        Ok(bytes)
    }
}
