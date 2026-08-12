//! Bounded primitive-field encoding and decoding.

use crate::ProtocolError;

const MAX_FIELD_BYTES: usize = 256 * 1024;

pub(super) fn texts(payload: &mut Vec<u8>, values: &[&str]) -> Result<(), ProtocolError> {
    for v in values {
        put_text(payload, v)?;
    }
    Ok(())
}
pub(super) fn put_text(payload: &mut Vec<u8>, value: &str) -> Result<(), ProtocolError> {
    if value.len() > MAX_FIELD_BYTES {
        return Err(ProtocolError::new("text field too large"));
    }
    put_u32(payload, value.len() as u32);
    payload.extend_from_slice(value.as_bytes());
    Ok(())
}
pub(super) fn put_u32(payload: &mut Vec<u8>, value: u32) {
    payload.extend_from_slice(&value.to_le_bytes());
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
            .ok_or_else(|| ProtocolError::new("truncated P4 payload"))?;
        self.offset += 1;
        Ok(value)
    }
    pub(super) fn u32(&mut self) -> Result<u32, ProtocolError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().unwrap();
        Ok(u32::from_le_bytes(bytes))
    }
    pub(super) fn u64(&mut self) -> Result<u64, ProtocolError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().unwrap();
        Ok(u64::from_le_bytes(bytes))
    }
    pub(super) fn f32(&mut self) -> Result<f32, ProtocolError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().unwrap();
        Ok(f32::from_le_bytes(bytes))
    }
    pub(super) fn text(&mut self) -> Result<String, ProtocolError> {
        let length = self.u32()? as usize;
        if length > MAX_FIELD_BYTES {
            return Err(ProtocolError::new("P4 text too large"));
        }
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| ProtocolError::new("P4 text must be UTF-8"))
    }
    pub(super) fn take(&mut self, length: usize) -> Result<&[u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| ProtocolError::new("P4 offset overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| ProtocolError::new("truncated P4 payload"))?;
        self.offset = end;
        Ok(bytes)
    }
}
