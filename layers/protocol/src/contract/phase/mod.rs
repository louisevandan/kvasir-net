//! Pipeline execution phase.

use crate::ProtocolError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Prefill,
    Decode,
}

impl Phase {
    pub(crate) fn code(&self) -> u8 {
        match self {
            Self::Prefill => 1,
            Self::Decode => 2,
        }
    }

    pub(crate) fn parse(code: u8) -> Result<Self, ProtocolError> {
        match code {
            1 => Ok(Self::Prefill),
            2 => Ok(Self::Decode),
            _ => Err(ProtocolError::new("invalid phase")),
        }
    }
}
