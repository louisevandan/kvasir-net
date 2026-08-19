
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequencePayload {
    /// This is the P4 SequenceId, not a local numeric handle.
    pub sequence_id: String,
    pub descriptors: Vec<Descriptor>,
    /// A non-alias descriptor must have one payload. Alias descriptors must
    /// have `None`; their storage is identified by `Descriptor::alias_of`.
    pub payloads: Vec<Option<Vec<u8>>>,
    /// Logical token count for a stage cut-set. Never infer this from tensor
    /// dimensions because a rank-1 hidden tensor's dim[0] is n_embd.
    pub n_tokens: Option<u32>,
    /// Optional stage-zero text input. It is tokenized by the C++ server using
    /// the loaded model vocabulary; it is never sent to a middle stage.
    pub prompt: Option<String>,
    /// Optional explicit token input for callers that already tokenized the
    /// prompt. Token ids are signed because llama_token is signed.
    pub initial_tokens: Option<Vec<i32>>,
    /// Optional P4 progress. It is omitted by legacy callers.
    pub position: Option<u32>,
    /// Opaque request-level generation options. Empty means the field is
    /// omitted from the v2 wire; the staged server does not interpret it yet.
    pub options: String,
    /// Tail-only sampled result. Middle stages and old payloads omit it.
    pub outcome: Option<OutcomeMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutcomeMetadata {
    pub token: i32,
    pub text: String,
    pub position: u32,
    pub stop: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum HopPhase {
    Prefill = 0,
    Decode = 1,
}

/// The body of one HOP frame. A legacy one-sequence body is still accepted by
/// the decoder, while the encoder always emits the HMUX envelope so a P4
/// window is transferred in one request/response exchange.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HopPayload {
    pub phase: HopPhase,
    pub sequences: Vec<SequencePayload>,
    /// True only for a body decoded from the pre-v2 local wire. Such a body
    /// is accepted for framing compatibility but cannot drive a real llama
    /// HOP because it has no phase or stage-zero input metadata.
    pub legacy: bool,
}

impl HopPayload {
    const ENVELOPE_MAGIC: [u8; 4] = *b"HMUX";
    const ENVELOPE_VERSION: u8 = 2;

    pub fn encode(&self, limits: ProtocolLimits) -> Result<Vec<u8>, FrameError> {
        if self.sequences.is_empty() {
            return Err(FrameError::InvalidSequence("empty HOP sequence list"));
        }
        if self.sequences.len() > limits.max_descriptors {
            return Err(FrameError::TooManyDescriptors);
        }
        let mut output = Self::ENVELOPE_MAGIC.to_vec();
        output.push(Self::ENVELOPE_VERSION);
        output.push(self.phase as u8);
        output.extend_from_slice(&0u16.to_le_bytes());
        put_u32(
            &mut output,
            u32::try_from(self.sequences.len()).map_err(|_| FrameError::TooManyDescriptors)?,
        );
        for sequence in &self.sequences {
            let encoded = sequence.encode_v2(limits)?;
            let length = u32::try_from(encoded.len()).map_err(|_| FrameError::FrameTooLarge)?;
            put_u32(&mut output, length);
            output.extend_from_slice(&encoded);
            if output.len() > limits.max_frame_bytes {
                return Err(FrameError::FrameTooLarge);
            }
        }
        Ok(output)
    }

    pub fn decode(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        if bytes.len() < Self::ENVELOPE_MAGIC.len()
            || bytes[..Self::ENVELOPE_MAGIC.len()] != Self::ENVELOPE_MAGIC
        {
            // Compatibility with the original one-sequence local wire.
            return Ok(Self {
                phase: HopPhase::Decode,
                sequences: vec![SequencePayload::decode(bytes, limits)?],
                legacy: true,
            });
        }
        // The old HMUX body was HMUX + count + legacy SequencePayloads. Keep
        // decoding it, but mark it legacy so the runtime can refuse unsafe
        // execution rather than inventing a phase.
        if bytes.len() < 8 || bytes[4] != Self::ENVELOPE_VERSION {
            let mut cursor = Cursor::new(&bytes[Self::ENVELOPE_MAGIC.len()..]);
            let count =
                usize::try_from(cursor.u32()?).map_err(|_| FrameError::TooManyDescriptors)?;
            if count == 0 || count > limits.max_descriptors {
                return Err(FrameError::InvalidSequence(
                    "empty or oversized legacy HOP sequence list",
                ));
            }
            let mut sequences = Vec::with_capacity(count);
            for _ in 0..count {
                let size = usize::try_from(cursor.u32()?).map_err(|_| FrameError::FrameTooLarge)?;
                let sequence = SequencePayload::decode(cursor.bytes(size)?, limits)?;
                sequences.push(sequence);
            }
            if cursor.remaining() != 0 {
                return Err(FrameError::InvalidSequence("trailing legacy HOP bytes"));
            }
            return Ok(Self {
                phase: HopPhase::Decode,
                sequences,
                legacy: true,
            });
        }
        let phase = match bytes[5] {
            0 => HopPhase::Prefill,
            1 => HopPhase::Decode,
            _ => return Err(FrameError::InvalidSequence("unknown HOP phase")),
        };
        if bytes[6] != 0 || bytes[7] != 0 {
            return Err(FrameError::InvalidSequence("reserved HOP envelope flags"));
        }
        let mut cursor = Cursor::new(&bytes[8..]);
        let count = usize::try_from(cursor.u32()?).map_err(|_| FrameError::TooManyDescriptors)?;
        if count == 0 {
            return Err(FrameError::InvalidSequence("empty HOP sequence list"));
        }
        if count > limits.max_descriptors {
            return Err(FrameError::TooManyDescriptors);
        }
        let mut sequences = Vec::with_capacity(count);
        for _ in 0..count {
            let size = usize::try_from(cursor.u32()?).map_err(|_| FrameError::FrameTooLarge)?;
            if size > limits.max_frame_bytes || size > cursor.remaining() {
                return Err(if size > limits.max_frame_bytes {
                    FrameError::FrameTooLarge
                } else {
                    FrameError::Truncated
                });
            }
            let sequence = SequencePayload::decode_v2(cursor.bytes(size)?, limits)?;
            sequences.push(sequence);
        }
        if cursor.remaining() != 0 {
            return Err(FrameError::InvalidSequence("trailing HOP envelope bytes"));
        }
        Ok(Self {
            phase,
            sequences,
            legacy: false,
        })
    }
}
