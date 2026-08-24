const MAGIC: [u8; 4] = *b"LCP4";
const TOKEN_COUNT_MAGIC: [u8; 4] = *b"NTOK";
const HEADER_BYTES: usize = 12;

/// The first revision of the adapter/server wire. All integers are little
/// endian so the C++ peer does not depend on host byte order.
pub const PROTOCOL_REVISION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Operation {
    Hello = 1,
    Hop = 2,
    HopResult = 3,
    Cancel = 4,
    KvSave = 5,
    KvRestore = 6,
    KvDrop = 7,
    KvResult = 8,
    Unload = 9,
    Error = 10,
    KvPrepare = 11,
    KvCommit = 12,
    KvAbort = 13,
    KvReconcile = 14,
    KvReceipt = 15,
    LogicalBatch = 16,
    PhysicalBatch = 17,
    PhysicalResult = 18,
    Tokenize = 19,
    Tokenized = 20,
    PhysicalRelease = 21,
}

impl TryFrom<u8> for Operation {
    type Error = FrameError;

    fn try_from(value: u8) -> Result<Self, FrameError> {
        let operation = match value {
            1 => Self::Hello,
            2 => Self::Hop,
            3 => Self::HopResult,
            4 => Self::Cancel,
            5 => Self::KvSave,
            6 => Self::KvRestore,
            7 => Self::KvDrop,
            8 => Self::KvResult,
            9 => Self::Unload,
            10 => Self::Error,
            11 => Self::KvPrepare,
            12 => Self::KvCommit,
            13 => Self::KvAbort,
            14 => Self::KvReconcile,
            15 => Self::KvReceipt,
            16 => Self::LogicalBatch,
            17 => Self::PhysicalBatch,
            18 => Self::PhysicalResult,
            19 => Self::Tokenize,
            20 => Self::Tokenized,
            21 => Self::PhysicalRelease,
            _ => return Err(FrameError::UnknownOperation(value)),
        };
        Ok(operation)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WireType {
    F32 = 1,
    F16 = 2,
    Q8 = 3,
    Q4 = 4,
    Bytes = 255,
}

impl TryFrom<u8> for WireType {
    type Error = FrameError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::F32),
            2 => Ok(Self::F16),
            3 => Ok(Self::Q8),
            4 => Ok(Self::Q4),
            255 => Ok(Self::Bytes),
            _ => Err(FrameError::UnknownWireType(value)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolLimits {
    pub max_frame_bytes: usize,
    pub max_descriptors: usize,
    pub max_payload_bytes: usize,
    pub max_name_bytes: usize,
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            // One F32 cut per token per sequence: a 5,000-token prompt at
            // n_embd 2,048 is 39 MiB for one sequence, so a ten-wide prefill
            // window is 391 MiB. At 128 MiB the window stopped at three and
            // the stage server refused the rest. Two gibibytes keeps the
            // `u32` wire length honest and matches the outer P4 frame limit,
            // so neither side is the one that refuses first.
            max_frame_bytes: 2 * 1024 * 1024 * 1024,
            // A 5k-token prefill can produce one outbound cut descriptor per
            // token; keep headroom for larger non-MTP context windows.
            max_descriptors: 16_384,
            max_payload_bytes: 2 * 1024 * 1024 * 1024,
            max_name_bytes: 4096,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameHeader {
    pub revision: u16,
    pub operation: Operation,
    pub body_bytes: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub header: FrameHeader,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn new(operation: Operation, body: Vec<u8>) -> Result<Self, FrameError> {
        let body_bytes = u32::try_from(body.len()).map_err(|_| FrameError::FrameTooLarge)?;
        Ok(Self {
            header: FrameHeader {
                revision: PROTOCOL_REVISION,
                operation,
                body_bytes,
            },
            body,
        })
    }

    pub fn encode(&self, limits: ProtocolLimits) -> Result<Vec<u8>, FrameError> {
        let body_bytes = u32::try_from(self.body.len()).map_err(|_| FrameError::FrameTooLarge)?;
        if body_bytes != self.header.body_bytes {
            return Err(FrameError::LengthMismatch {
                declared: self.header.body_bytes,
                actual: body_bytes,
            });
        }
        let total = HEADER_BYTES
            .checked_add(self.body.len())
            .ok_or(FrameError::FrameTooLarge)?;
        if total > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }

        let mut output = Vec::with_capacity(total);
        output.extend_from_slice(&MAGIC);
        output.extend_from_slice(&self.header.revision.to_le_bytes());
        output.push(self.header.operation as u8);
        output.push(0); // reserved flags; must remain zero in revision 1
        output.extend_from_slice(&self.header.body_bytes.to_le_bytes());
        output.extend_from_slice(&self.body);
        Ok(output)
    }

    pub fn decode(bytes: &[u8], limits: ProtocolLimits) -> Result<Self, FrameError> {
        if bytes.len() < HEADER_BYTES {
            return Err(FrameError::Truncated);
        }
        if bytes.len() > limits.max_frame_bytes {
            return Err(FrameError::FrameTooLarge);
        }
        if bytes[..4] != MAGIC {
            return Err(FrameError::BadMagic);
        }

        let revision = u16::from_le_bytes([bytes[4], bytes[5]]);
        if revision != PROTOCOL_REVISION {
            return Err(FrameError::UnsupportedRevision(revision));
        }
        if bytes[7] != 0 {
            return Err(FrameError::ReservedFlags(bytes[7]));
        }
        let operation = Operation::try_from(bytes[6])?;
        let declared = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let declared_usize = usize::try_from(declared).map_err(|_| FrameError::FrameTooLarge)?;
        let expected = HEADER_BYTES
            .checked_add(declared_usize)
            .ok_or(FrameError::FrameTooLarge)?;
        if expected != bytes.len() {
            return Err(FrameError::LengthMismatch {
                declared,
                actual: u32::try_from(bytes.len() - HEADER_BYTES)
                    .map_err(|_| FrameError::FrameTooLarge)?,
            });
        }
        Ok(Self {
            header: FrameHeader {
                revision,
                operation,
                body_bytes: declared,
            },
            body: bytes[HEADER_BYTES..].to_vec(),
        })
    }

    /// Writes one complete frame to a connected local stream.
    pub fn write_to<W: Write>(
        &self,
        writer: &mut W,
        limits: ProtocolLimits,
    ) -> Result<(), FrameIoError> {
        let encoded = self.encode(limits)?;
        writer.write_all(&encoded).map_err(FrameIoError::Io)
    }

    /// Reads exactly one frame from a connected local stream.
    pub fn read_from<R: Read>(
        reader: &mut R,
        limits: ProtocolLimits,
    ) -> Result<Self, FrameIoError> {
        let mut header = [0u8; HEADER_BYTES];
        reader.read_exact(&mut header).map_err(FrameIoError::Io)?;
        let body_bytes = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
        let total = HEADER_BYTES
            .checked_add(usize::try_from(body_bytes).map_err(|_| FrameError::FrameTooLarge)?)
            .ok_or(FrameError::FrameTooLarge)?;
        if total > limits.max_frame_bytes {
            return Err(FrameIoError::Frame(FrameError::FrameTooLarge));
        }
        let mut encoded = header.to_vec();
        encoded.resize(total, 0);
        reader
            .read_exact(&mut encoded[HEADER_BYTES..])
            .map_err(FrameIoError::Io)?;
        Ok(Self::decode(&encoded, limits)?)
    }
}

#[derive(Debug)]
pub enum FrameIoError {
    Io(io::Error),
    Frame(FrameError),
}

impl fmt::Display for FrameIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "frame stream I/O failed: {error}"),
            Self::Frame(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for FrameIoError {}

impl From<FrameError> for FrameIoError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]


pub struct Descriptor {
    pub wire_type: WireType,
    pub dimensions: Vec<u64>,
    pub strides: Vec<u64>,
    pub nbytes: u64,
    pub view_offset: u64,
    pub alias_of: Option<u32>,
    pub flags: u8,
    pub name: String,
}
