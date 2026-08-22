//! Newline-delimited JSON, the wire shape `apps/llama`'s submission stream
//! uses (`pipeline-inference-stream.ts`'s `RingStreamWriter` writes
//! `JSON.stringify(event) + "\n"` and reads by splitting on `0x0a`).
//!
//! Encoding and decoding themselves are not this file's job: every command
//! and event is built and parsed by the canonical
//! `p4_adapter::deployment::wire` module, the same one the llama-path
//! server's TypeScript twin (`packages/llama_domain/src/common/protocol/
//! pipeline-submission/parse.ts`) must agree with byte-for-byte. This file
//! only adds and strips the trailing `\n` that turns one JSON value into one
//! line on the socket.

use crate::contract::{Command, Event};
use p4_adapter::deployment::wire;
use std::io::{self, BufRead, Write};

pub fn encode_command(command: &Command) -> io::Result<Vec<u8>> {
    let value = match command {
        Command::Submit(submit) => wire::encode_submit(submit),
        Command::Cancel(cancel) => wire::encode_cancel(cancel),
    };
    let mut line = serde_json::to_vec(&value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    line.push(b'\n');
    Ok(line)
}

pub fn write_command<W: Write>(writer: &mut W, command: &Command) -> io::Result<()> {
    let line = encode_command(command)?;
    writer.write_all(&line)?;
    writer.flush()
}

/// Reads exactly one line and decodes it as an `Event`. Returns `Ok(None)`
/// on clean end-of-stream (no bytes read at all), matching
/// `TransportReader::recv`'s contract.
pub fn read_event<R: BufRead>(reader: &mut R) -> io::Result<Option<Event>> {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line)?;
    if bytes == 0 {
        return Ok(None);
    }
    let trimmed = line.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        // A blank keep-alive line carries no event; the caller loops and
        // reads the next one rather than treating this as end-of-stream.
        return read_event(reader);
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    wire::parse_event(&value)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{RejectReason, Rejected, Submit};
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn command_round_trips_through_a_line() {
        let mut buffer = Vec::new();
        let command = Command::Submit(Submit {
            deployment_id: "dep".into(),
            deployment_generation: 1,
            submission_id: "sub".into(),
            request: json!({"prompt": "hi"}),
        });
        write_command(&mut buffer, &command).expect("write");
        assert_eq!(buffer.last(), Some(&b'\n'));
        let mut cursor = Cursor::new(buffer);
        let mut line = String::new();
        cursor.read_line(&mut line).expect("read back");
        let value: serde_json::Value = serde_json::from_str(line.trim_end()).expect("json");
        let decoded: Submit = wire::parse_submit(&value).expect("decode");
        let Command::Submit(expected) = command else {
            unreachable!()
        };
        assert_eq!(decoded, expected);
    }

    #[test]
    fn encoded_submit_carries_the_protocol_field_and_an_object_request() {
        let command = Command::Submit(Submit {
            deployment_id: "dep".into(),
            deployment_generation: 1,
            submission_id: "sub".into(),
            request: json!({"messages": []}),
        });
        let line = encode_command(&command).expect("encode");
        let text = String::from_utf8(line).expect("utf8");
        let value: serde_json::Value = serde_json::from_str(text.trim_end()).expect("json");
        assert_eq!(value["protocol"], json!(wire::PROTOCOL));
        assert!(value["request"].is_object());
    }

    #[test]
    fn read_event_returns_none_on_clean_eof() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        assert_eq!(read_event(&mut cursor).expect("read"), None);
    }

    #[test]
    fn read_event_skips_blank_keepalive_lines() {
        let mut cursor = Cursor::new(
            format!(
                "\n\n{}\n",
                json!({
                    "protocol": wire::PROTOCOL,
                    "type": "rejected",
                    "submission_id": "s",
                    "reason": "full",
                })
            )
            .into_bytes(),
        );
        let event = read_event(&mut cursor).expect("read").expect("some");
        assert_eq!(
            event,
            Event::Rejected(Rejected {
                submission_id: "s".into(),
                reason: RejectReason::Full,
            })
        );
    }
}
