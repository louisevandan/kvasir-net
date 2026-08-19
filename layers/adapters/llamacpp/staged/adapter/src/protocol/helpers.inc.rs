fn validate_kv_text(value: &str, limit: usize, field: &'static str) -> Result<(), FrameError> {
    if value.is_empty() {
        return Err(FrameError::InvalidSequence(field));
    }
    if value.len() > limit {
        return Err(FrameError::NameTooLong);
    }
    if std::str::from_utf8(value.as_bytes()).is_err() {
        return Err(FrameError::InvalidSequence(field));
    }
    Ok(())
}

fn put_string(output: &mut Vec<u8>, value: &str) -> Result<(), FrameError> {
    put_u32(
        output,
        u32::try_from(value.len()).map_err(|_| FrameError::NameTooLong)?,
    );
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn validate_alias(index: usize, descriptor: &Descriptor, count: usize) -> Result<(), FrameError> {
    if let Some(alias) = descriptor.alias_of
        && usize::try_from(alias).map_or(true, |target| target >= count || target == index)
    {
        return Err(FrameError::InvalidDescriptor("alias target"));
    }
    Ok(())
}

fn encode_descriptor(
    output: &mut Vec<u8>,
    descriptor: &Descriptor,
    limits: ProtocolLimits,
) -> Result<(), FrameError> {
    output.push(descriptor.wire_type as u8);
    output.push(descriptor.dimensions.len() as u8);
    for dimension in &descriptor.dimensions {
        put_u64(output, *dimension);
    }
    for stride in &descriptor.strides {
        put_u64(output, *stride);
    }
    put_u64(output, descriptor.nbytes);
    put_u64(output, descriptor.view_offset);
    put_u32(output, descriptor.alias_of.unwrap_or(u32::MAX));
    output.push(descriptor.flags);
    let name = descriptor.name.as_bytes();
    if name.len() > limits.max_name_bytes {
        return Err(FrameError::NameTooLong);
    }
    put_u32(
        output,
        u32::try_from(name.len()).map_err(|_| FrameError::NameTooLong)?,
    );
    output.extend_from_slice(name);
    Ok(())
}

fn decode_descriptor(
    cursor: &mut Cursor<'_>,
    limits: ProtocolLimits,
) -> Result<Descriptor, FrameError> {
    let wire_type = WireType::try_from(cursor.u8()?)?;
    let rank = usize::from(cursor.u8()?);
    if rank > 8 {
        return Err(FrameError::InvalidDescriptor("dimension/stride rank"));
    }
    let mut dimensions = Vec::with_capacity(rank);
    let mut strides = Vec::with_capacity(rank);
    for _ in 0..rank {
        dimensions.push(cursor.u64()?);
    }
    for _ in 0..rank {
        strides.push(cursor.u64()?);
    }
    let nbytes = cursor.u64()?;
    let view_offset = cursor.u64()?;
    let alias = cursor.u32()?;
    let flags = cursor.u8()?;
    let name_len = usize::try_from(cursor.u32()?).map_err(|_| FrameError::NameTooLong)?;
    if name_len > limits.max_name_bytes {
        return Err(FrameError::NameTooLong);
    }
    let name = String::from_utf8(cursor.bytes(name_len)?.to_vec())
        .map_err(|_| FrameError::InvalidSequence("descriptor name utf-8"))?;
    Ok(Descriptor {
        wire_type,
        dimensions,
        strides,
        nbytes,
        view_offset,
        alias_of: (alias != u32::MAX).then_some(alias),
        flags,
        name,
    })
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn validate_text(value: &str, limit: usize) -> Result<(), FrameError> {
    if value.len() > limit {
        return Err(FrameError::NameTooLong);
    }
    if std::str::from_utf8(value.as_bytes()).is_err() {
        return Err(FrameError::InvalidSequence("text utf-8"));
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
    fn take(&mut self, size: usize) -> Result<&'a [u8], FrameError> {
        let end = self
            .position
            .checked_add(size)
            .ok_or(FrameError::Truncated)?;
        if end > self.bytes.len() {
            return Err(FrameError::Truncated);
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }
    fn bytes(&mut self, size: usize) -> Result<&'a [u8], FrameError> {
        self.take(size)
    }
    fn u8(&mut self) -> Result<u8, FrameError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, FrameError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, FrameError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn kv_string(
        &mut self,
        _limits: &ProtocolLimits,
        field: &'static str,
        limit: usize,
    ) -> Result<String, FrameError> {
        let length = usize::try_from(self.u32()?).map_err(|_| FrameError::NameTooLong)?;
        if length == 0 {
            return Err(FrameError::InvalidSequence(field));
        }
        if length > limit {
            return Err(FrameError::NameTooLong);
        }
        let value = String::from_utf8(self.bytes(length)?.to_vec())
            .map_err(|_| FrameError::InvalidSequence(field))?;
        Ok(value)
    }
}

impl Descriptor {
    pub fn validate(&self, limits: ProtocolLimits) -> Result<(), FrameError> {
        if self.dimensions.len() != self.strides.len() || self.dimensions.len() > 8 {
            return Err(FrameError::InvalidDescriptor("dimension/stride rank"));
        }
        if self.name.len() > limits.max_name_bytes {
            return Err(FrameError::NameTooLong);
        }
        if self.nbytes as usize > limits.max_payload_bytes {
            return Err(FrameError::PayloadTooLarge);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameError {
    BadMagic,
    Truncated,
    FrameTooLarge,
    PayloadTooLarge,
    NameTooLong,
    UnsupportedRevision(u16),
    UnknownOperation(u8),
    UnknownWireType(u8),
    ReservedFlags(u8),
    InvalidDescriptor(&'static str),
    InvalidSequence(&'static str),
    TooManyDescriptors,
    PayloadLengthMismatch { declared: u64, actual: u64 },
    LengthMismatch { declared: u32, actual: u32 },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid staged adapter frame: {self:?}")
    }
}

impl std::error::Error for FrameError {}

