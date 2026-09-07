use super::CapsuleError;

pub(super) struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(super) fn take(&mut self, count: usize) -> Result<&'a [u8], CapsuleError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(CapsuleError::IntegerOverflow)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(CapsuleError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    pub(super) fn byte(&mut self) -> Result<u8, CapsuleError> {
        Ok(self.take(1)?[0])
    }

    pub(super) fn u16(&mut self) -> Result<u16, CapsuleError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(super) fn u32(&mut self) -> Result<u32, CapsuleError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(super) fn i32(&mut self) -> Result<i32, CapsuleError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(super) fn u64(&mut self) -> Result<u64, CapsuleError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(super) fn i64(&mut self) -> Result<i64, CapsuleError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(super) fn string(&mut self) -> Result<String, CapsuleError> {
        let count = self.u16()? as usize;
        let value =
            std::str::from_utf8(self.take(count)?).map_err(|_| CapsuleError::InvalidUtf8)?;
        Ok(value.to_owned())
    }

    pub(super) fn done(&self) -> bool {
        self.offset == self.bytes.len()
    }

    pub(super) fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }
}
