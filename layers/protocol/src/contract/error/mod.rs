//! Binary protocol error identity.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolError {
    message: String,
    peer_closed: bool,
}

impl ProtocolError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            peer_closed: false,
        }
    }

    pub fn peer_closed() -> Self {
        Self {
            message: "P4 peer closed before a frame".into(),
            peer_closed: true,
        }
    }

    pub fn is_peer_closed(&self) -> bool {
        self.peer_closed
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProtocolError {}
