use p4_protocol::{Message, RoutedMessage, decode_routed_message, encode_routed_message};
use tokio::io::{AsyncRead, AsyncReadExt};

type AsyncError = Box<dyn std::error::Error + Send + Sync>;
const HEADER_BYTES: usize = 16;
const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub(super) async fn read_message(
    stream: &mut (impl AsyncRead + Unpin),
) -> Result<RoutedMessage, AsyncError> {
    let mut header = [0u8; HEADER_BYTES];
    stream.read_exact(&mut header).await?;
    let length = u32::from_le_bytes(header[8..12].try_into()?) as usize;
    if length > MAX_FRAME_BYTES {
        return Err("invalid P4 frame length".into());
    }
    let mut frame = Vec::with_capacity(HEADER_BYTES + length);
    frame.extend_from_slice(&header);
    frame.resize(HEADER_BYTES + length, 0);
    stream.read_exact(&mut frame[HEADER_BYTES..]).await?;
    Ok(decode_routed_message(&frame)?)
}

pub(super) fn encode(routed: &RoutedMessage) -> Result<Vec<u8>, AsyncError> {
    Ok(encode_routed_message(routed)?)
}

/// Restores the caller's business request id on an outbound frame.
///
/// A response is routed by `route_id`; the request id inside it is the
/// caller's and must survive untouched, since two controllers are allowed to
/// reuse one business id over the same socket.
pub(super) fn restore_request_id(message: Message, request_id: &str) -> Message {
    match message {
        Message::Token(mut value) => {
            value.request_id = request_id.into();
            Message::Token(value)
        }
        Message::Done(mut value) => {
            value.request_id = request_id.into();
            Message::Done(value)
        }
        Message::Error { detail, .. } => Message::Error {
            request_id: request_id.into(),
            detail,
        },
        other => other,
    }
}
