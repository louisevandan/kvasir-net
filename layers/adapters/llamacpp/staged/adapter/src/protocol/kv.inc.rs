
/// The request body used by KV_SAVE, KV_RESTORE, and KV_DROP.
///
/// This intentionally mirrors the C++ `protocol::KvPayload` field-for-field:
/// length-prefixed UTF-8 strings, followed by three little-endian u32 values
/// and a length-prefixed checksum (`-` means no expected checksum).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KvPayload {
    pub sequence_id: String,
    pub cache_key: String,
    pub model_identity: String,
    pub stage_begin: i32,
    pub stage_end: i32,
    pub flags: u32,
    pub expected_checksum: String,
    /// Optional on the legacy direct KV wire; required by transaction verbs.
    pub operation_id: String,
}

impl KvPayload {
    pub fn encode(&self, limits: ProtocolLimits) -> Result<Vec<u8>, FrameError> {
        validate_kv_text(&self.sequence_id, limits.max_name_bytes, "sequence id")?;
        validate_kv_text(&self.cache_key, 256, "cache key")?;
        if !self
            .cache_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(FrameError::InvalidSequence("invalid KV cache key"));
        }
        validate_kv_text(&self.model_identity, 4096, "model identity")?;
        if !self.operation_id.is_empty() {
            validate_kv_text(&self.operation_id, limits.max_name_bytes, "operation id")?;
        }
        if self.stage_begin < 0 || self.stage_end <= self.stage_begin || self.flags > 3 {
            return Err(FrameError::InvalidSequence("invalid KV stage metadata"));
        }
        if !self.expected_checksum.is_empty()
            && (self.expected_checksum.len() != 64
                || std::str::from_utf8(self.expected_checksum.as_bytes()).is_err())
        {
            return Err(FrameError::InvalidSequence("invalid KV checksum"));
        }

        let mut output = Vec::new();
        put_string(&mut output, &self.sequence_id)?;
        put_string(&mut output, &self.cache_key)?;
        put_string(&mut output, &self.model_identity)?;
        put_u32(&mut output, self.stage_begin as u32);
        put_u32(&mut output, self.stage_end as u32);
        put_u32(&mut output, self.flags);
        put_string(
            &mut output,
            if self.expected_checksum.is_empty() {
                "-"
            } else {
                &self.expected_checksum
            },
        )?;
        if !self.operation_id.is_empty() {
            put_string(&mut output, &self.operation_id)?;
        }
        if output.len() > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }
        Ok(output)
    }

    pub fn decode(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        if bytes.len() > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }
        let mut cursor = Cursor::new(bytes);
        let sequence_id = cursor.kv_string(&limits, "sequence id", limits.max_name_bytes)?;
        let cache_key = cursor.kv_string(&limits, "cache key", 256)?;
        if !cache_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(FrameError::InvalidSequence("invalid KV cache key"));
        }
        let model_identity = cursor.kv_string(&limits, "model identity", 4096)?;
        let stage_begin = cursor.u32()? as i32;
        let stage_end = cursor.u32()? as i32;
        let flags = cursor.u32()?;
        let checksum = cursor.kv_string(&limits, "checksum", 64)?;
        let expected_checksum = if checksum == "-" {
            String::new()
        } else {
            checksum
        };
        let operation_id = if cursor.remaining() == 0 {
            String::new()
        } else {
            cursor.kv_string(&limits, "operation id", limits.max_name_bytes)?
        };
        if stage_begin < 0 || stage_end <= stage_begin || flags > 3 || cursor.remaining() != 0 {
            return Err(FrameError::InvalidSequence("invalid KV metadata"));
        }
        if !expected_checksum.is_empty() && expected_checksum.len() != 64 {
            return Err(FrameError::InvalidSequence("invalid KV checksum"));
        }
        Ok(Self {
            sequence_id,
            cache_key,
            model_identity,
            stage_begin,
            stage_end,
            flags,
            expected_checksum,
            operation_id,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum KvReceiptState { Absent = 0, Prepared = 1, Committed = 2, Aborted = 3, Inconsistent = 4, Committing = 5 }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KvReceipt {
    pub operation_id: String,
    pub sequence_id: String,
    pub cache_key: String,
    pub model_identity: String,
    pub stage_begin: i32,
    pub stage_end: i32,
    pub kind: u32,
    pub state: KvReceiptState,
    pub bytes: u64,
    pub checksum: String,
    pub detail: String,
}

impl KvReceipt {
    pub fn encode(&self, limits: ProtocolLimits) -> Result<Vec<u8>, FrameError> {
        validate_kv_text(&self.operation_id, limits.max_name_bytes, "operation id")?;
        validate_kv_text(&self.sequence_id, limits.max_name_bytes, "sequence id")?;
        validate_kv_text(&self.cache_key, 256, "cache key")?;
        validate_kv_text(&self.model_identity, 4096, "model identity")?;
        validate_kv_text(&self.checksum, 64, "receipt checksum")?;
        validate_kv_text(&self.detail, 4096, "receipt detail")?;
        if self.stage_begin < 0 || self.stage_end <= self.stage_begin
            || !((1..=3).contains(&self.kind)
                || self.kind == 0
                    && matches!(self.state, KvReceiptState::Absent | KvReceiptState::Inconsistent))
            || self.checksum.len() != 64 {
            return Err(FrameError::InvalidSequence("invalid KV receipt"));
        }
        let mut out = Vec::new();
        put_string(&mut out, &self.operation_id)?;
        put_string(&mut out, &self.sequence_id)?;
        put_string(&mut out, &self.cache_key)?;
        put_string(&mut out, &self.model_identity)?;
        put_u32(&mut out, self.stage_begin as u32);
        put_u32(&mut out, self.stage_end as u32);
        put_u32(&mut out, self.kind);
        out.push(self.state as u8);
        put_u64(&mut out, self.bytes);
        put_string(&mut out, &self.checksum)?;
        put_string(&mut out, &self.detail)?;
        Ok(out)
    }

    pub fn decode(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        let mut cursor = Cursor::new(bytes);
        let operation_id = cursor.kv_string(&limits, "operation id", limits.max_name_bytes)?;
        let sequence_id = cursor.kv_string(&limits, "sequence id", limits.max_name_bytes)?;
        let cache_key = cursor.kv_string(&limits, "cache key", 256)?;
        let model_identity = cursor.kv_string(&limits, "model identity", 4096)?;
        let stage_begin = cursor.u32()? as i32;
        let stage_end = cursor.u32()? as i32;
        let kind = cursor.u32()?;
        let state = match cursor.u8()? {
            0 => KvReceiptState::Absent,
            1 => KvReceiptState::Prepared,
            2 => KvReceiptState::Committed,
            3 => KvReceiptState::Aborted,
            4 => KvReceiptState::Inconsistent,
            5 => KvReceiptState::Committing,
            _ => return Err(FrameError::InvalidSequence("invalid KV receipt state")),
        };
        let bytes = cursor.u64()?;
        let checksum = cursor.kv_string(&limits, "receipt checksum", 64)?;
        let detail = cursor.kv_string(&limits, "receipt detail", 4096)?;
        if stage_begin < 0 || stage_end <= stage_begin
            || !((1..=3).contains(&kind)
                || kind == 0
                    && matches!(state, KvReceiptState::Absent | KvReceiptState::Inconsistent))
            || checksum.len() != 64 || cursor.remaining() != 0 {
            return Err(FrameError::InvalidSequence("invalid KV receipt"));
        }
        Ok(Self { operation_id, sequence_id, cache_key, model_identity,
            stage_begin, stage_end, kind, state, bytes, checksum, detail })
    }
}

/// The response body used by the C++ `protocol::KvResult`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KvResult {
    pub sequence_id: String,
    pub cache_key: String,
    pub bytes: u64,
    pub checksum: String,
}

impl KvResult {
    pub fn encode(&self, limits: ProtocolLimits) -> Result<Vec<u8>, FrameError> {
        validate_kv_text(&self.sequence_id, limits.max_name_bytes, "sequence id")?;


        validate_kv_text(&self.cache_key, 256, "cache key")?;
        validate_kv_text(&self.checksum, 64, "checksum")?;
        if self.checksum.len() != 64 {
            return Err(FrameError::InvalidSequence("invalid KV result checksum"));
        }
        let mut output = Vec::new();
        put_string(&mut output, &self.sequence_id)?;
        put_string(&mut output, &self.cache_key)?;
        put_u64(&mut output, self.bytes);
        put_string(&mut output, &self.checksum)?;
        Ok(output)
    }

    pub fn decode(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        if bytes.len() > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }
        let mut cursor = Cursor::new(bytes);
        let sequence_id = cursor.kv_string(&limits, "sequence id", limits.max_name_bytes)?;
        let cache_key = cursor.kv_string(&limits, "cache key", 256)?;
        if !cache_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(FrameError::InvalidSequence("invalid KV cache key"));
        }
        let bytes = cursor.u64()?;
        let checksum = cursor.kv_string(&limits, "checksum", 64)?;
        if checksum.len() != 64 || cursor.remaining() != 0 {
            return Err(FrameError::InvalidSequence("invalid KV result"));
        }
        Ok(Self {
            sequence_id,
            cache_key,
            bytes,
            checksum,
        })
    }
}
