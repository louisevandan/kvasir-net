//! Header validation and stream framing.

use super::payload::{decode_payload, encode_payload};
use crate::{Message, PROTOCOL, ProtocolError, VERSION};
use std::io::{Read, Write};

const HEADER_BYTES: usize = 16;
const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_ROUTE_BYTES: usize = 4096;

/// Transport correlation that is independent of a business request/operation ID.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutedMessage {
    pub route_id: String,
    /// Absolute Unix time in milliseconds. Zero means no wire deadline.
    pub deadline_unix_ms: u64,
    pub message: Message,
}

impl RoutedMessage {
    pub fn new(
        route_id: impl Into<String>,
        deadline_unix_ms: u64,
        message: Message,
    ) -> Result<Self, ProtocolError> {
        let route_id = route_id.into();
        if route_id.is_empty() || route_id.len() > MAX_ROUTE_BYTES {
            return Err(ProtocolError::new(
                "route_id must contain 1..4096 UTF-8 bytes",
            ));
        }
        Ok(Self {
            route_id,
            deadline_unix_ms,
            message,
        })
    }

    fn compatibility(message: Message) -> Result<Self, ProtocolError> {
        Self::new(message.correlation_id(), 0, message.clone())
    }
}

pub fn write_message(writer: &mut impl Write, message: &Message) -> Result<(), ProtocolError> {
    write_routed_message(writer, &RoutedMessage::compatibility(message.clone())?)
}

pub fn write_routed_message(
    writer: &mut impl Write,
    routed: &RoutedMessage,
) -> Result<(), ProtocolError> {
    writer
        .write_all(&encode_routed_message(routed)?)
        .map_err(io_error)?;
    writer.flush().map_err(io_error)
}

pub fn encode_message(message: &Message) -> Result<Vec<u8>, ProtocolError> {
    encode_routed_message(&RoutedMessage::compatibility(message.clone())?)
}

pub fn encode_routed_message(routed: &RoutedMessage) -> Result<Vec<u8>, ProtocolError> {
    if routed.route_id.is_empty() || routed.route_id.len() > MAX_ROUTE_BYTES {
        return Err(ProtocolError::new(
            "route_id must contain 1..4096 UTF-8 bytes",
        ));
    }
    let (kind, message_payload) = encode_payload(&routed.message)?;
    let route_len = u32::try_from(routed.route_id.len())
        .map_err(|_| ProtocolError::new("route_id exceeds u32"))?;
    let mut payload = Vec::with_capacity(12 + routed.route_id.len() + message_payload.len());
    payload.extend_from_slice(&route_len.to_le_bytes());
    payload.extend_from_slice(routed.route_id.as_bytes());
    payload.extend_from_slice(&routed.deadline_unix_ms.to_le_bytes());
    payload.extend_from_slice(&message_payload);
    let bytes =
        u32::try_from(payload.len()).map_err(|_| ProtocolError::new("frame exceeds u32"))?;
    let mut header = [0u8; HEADER_BYTES];
    header[..4].copy_from_slice(&PROTOCOL);
    header[4] = VERSION;
    header[5] = kind;
    header[8..12].copy_from_slice(&bytes.to_le_bytes());
    let mut frame = Vec::with_capacity(HEADER_BYTES + payload.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_message(frame: &[u8]) -> Result<Message, ProtocolError> {
    Ok(decode_routed_message(frame)?.message)
}

pub fn decode_routed_message(frame: &[u8]) -> Result<RoutedMessage, ProtocolError> {
    if frame.len() < HEADER_BYTES {
        return Err(ProtocolError::new("incomplete P4 binary header"));
    }
    let header = &frame[..HEADER_BYTES];
    if header[..4] != PROTOCOL
        || header[4] != VERSION
        || header[6] != 0
        || header[7] != 0
        || header[12..16] != [0; 4]
    {
        return Err(ProtocolError::new("unsupported P4 binary header"));
    }
    let length = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if length > MAX_FRAME_BYTES || frame.len() != HEADER_BYTES + length {
        return Err(ProtocolError::new("invalid P4 frame length"));
    }
    decode_routed_payload(header[5], &frame[HEADER_BYTES..])
}

pub fn read_message(reader: &mut impl Read) -> Result<Message, ProtocolError> {
    Ok(read_routed_message(reader)?.message)
}

pub fn read_routed_message(reader: &mut impl Read) -> Result<RoutedMessage, ProtocolError> {
    let mut header = [0u8; HEADER_BYTES];
    match reader.read(&mut header[..1]) {
        Ok(0) => return Err(ProtocolError::peer_closed()),
        Ok(_) => {}
        Err(error) => return Err(io_error(error)),
    }
    reader.read_exact(&mut header[1..]).map_err(io_error)?;
    if header[..4] != PROTOCOL
        || header[4] != VERSION
        || header[6] != 0
        || header[7] != 0
        || header[12..16] != [0; 4]
    {
        return Err(ProtocolError::new("unsupported P4 binary header"));
    }
    let length = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(ProtocolError::new("invalid P4 frame length"));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).map_err(io_error)?;
    decode_routed_payload(header[5], &payload)
}

fn decode_routed_payload(kind: u8, payload: &[u8]) -> Result<RoutedMessage, ProtocolError> {
    if payload.len() < 12 {
        return Err(ProtocolError::new("incomplete P4 route envelope"));
    }
    let route_len = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
    if route_len == 0 || route_len > MAX_ROUTE_BYTES || payload.len() < 12 + route_len {
        return Err(ProtocolError::new("invalid P4 route envelope"));
    }
    let route_id = std::str::from_utf8(&payload[4..4 + route_len])
        .map_err(|_| ProtocolError::new("route_id is not UTF-8"))?
        .to_owned();
    let deadline_offset = 4 + route_len;
    let deadline_unix_ms = u64::from_le_bytes(
        payload[deadline_offset..deadline_offset + 8]
            .try_into()
            .unwrap(),
    );
    let message = decode_payload(kind, &payload[deadline_offset + 8..])?;
    Ok(RoutedMessage {
        route_id,
        deadline_unix_ms,
        message,
    })
}

fn io_error(error: std::io::Error) -> ProtocolError {
    ProtocolError::new(format!("P4 I/O: {error}"))
}
