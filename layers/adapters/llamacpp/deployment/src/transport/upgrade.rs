//! The HTTP/1.1 Upgrade handshake this client performs before sending or
//! receiving a single JSON line.
//!
//! The llama-path server only starts serving submission events after
//! `server.on("upgrade", ...)` sees exactly this exchange
//! (`apps/llama/src/server/pipeline-inference-stream.ts`): a plain
//! `TcpStream::connect` followed immediately by JSON lines -- what this
//! crate did before this file existed -- is bytes the server's HTTP parser
//! never gets a chance to read as anything but a malformed request, and the
//! two sides never spoke to each other at all.
//!
//! Generic over `Write`/`BufRead` rather than tied to `TcpStream` so the
//! request line and response parsing are unit-testable against in-memory
//! buffers, with no socket and no `apps/llama` process.

use std::io::{self, BufRead, Write};

/// The path both the v1 ring-inference stream and this submission stream
/// upgrade on, discriminated only by the `Upgrade` header's value -- see
/// `packages/llama_domain/src/common/protocol/pipeline-runtime/types.ts`'s
/// `PIPELINE_INFERENCE_STREAM_PATH`. That module is TypeScript, outside this
/// crate's allowlist, so its value is mirrored here as a literal rather than
/// imported; the cross-wire test (`tests/cross_wire.rs`) is what proves the
/// two have not drifted apart.
pub const UPGRADE_PATH: &str = "/api/pipeline-inference-stream";

/// Writes the Upgrade request, then blocks until the server's response
/// headers have been fully read and confirmed as `101 Switching Protocols`.
///
/// `reader` must be the same buffered reader the caller keeps using
/// afterward for event lines. `BufRead::read_line` only ever consumes up to
/// and including the newline it stops at, so any bytes the server already
/// sent past the blank line terminating the headers stay in `reader`'s
/// internal buffer rather than being discarded -- nothing sent immediately
/// after the handshake is lost.
pub fn perform<W: Write, R: BufRead>(
    writer: &mut W,
    reader: &mut R,
    host: &str,
    protocol: &str,
) -> io::Result<()> {
    let request = format!(
        "GET {UPGRADE_PATH} HTTP/1.1\r\nHost: {host}\r\nConnection: Upgrade\r\nUpgrade: {protocol}\r\n\r\n"
    );
    writer.write_all(request.as_bytes())?;
    writer.flush()?;

    let mut status_line = String::new();
    read_crlf_line(reader, &mut status_line)?;
    if !status_line.starts_with("HTTP/1.1 101") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected HTTP/1.1 101 Switching Protocols, got {status_line:?}"),
        ));
    }
    // Drain header lines until the blank line that ends them. Header
    // content itself is not read for anything -- the status line already
    // confirmed the protocol switch, and the llama-path server's own
    // `Upgrade` response header only ever echoes back what this request
    // asked for.
    loop {
        let mut line = String::new();
        let bytes = read_crlf_line(reader, &mut line)?;
        if bytes == 0 || line.is_empty() {
            return Ok(());
        }
    }
}

/// Reads one line and strips its trailing `\r\n`/`\n`. Distinct from
/// `BufRead::read_line` only in that trimming happens here once, in one
/// place, instead of at every call site.
fn read_crlf_line<R: BufRead>(reader: &mut R, out: &mut String) -> io::Result<usize> {
    let bytes = reader.read_line(out)?;
    while out.ends_with(['\n', '\r']) {
        out.pop();
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn writes_a_well_formed_upgrade_request() {
        let mut written = Vec::new();
        let mut response = Cursor::new(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: proto\r\nConnection: Upgrade\r\n\r\n"
                .to_vec(),
        );
        perform(&mut written, &mut response, "127.0.0.1:9", "proto").expect("upgrade");
        let text = String::from_utf8(written).expect("utf8");
        assert!(text.starts_with("GET /api/pipeline-inference-stream HTTP/1.1\r\n"));
        assert!(text.contains("Host: 127.0.0.1:9\r\n"));
        assert!(text.contains("Upgrade: proto\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
    }

    #[test]
    fn a_non_101_status_is_an_error_rather_than_silently_proceeding() {
        let mut written = Vec::new();
        let mut response =
            Cursor::new(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n".to_vec());
        let error = perform(&mut written, &mut response, "h", "proto").expect_err("should fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn bytes_after_the_blank_line_are_left_for_the_caller_to_read() {
        let mut written = Vec::new();
        let mut response = Cursor::new(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: proto\r\n\r\n{\"leftover\":true}\n"
                .to_vec(),
        );
        perform(&mut written, &mut response, "h", "proto").expect("upgrade");
        let mut remainder = String::new();
        response.read_line(&mut remainder).expect("read remainder");
        assert_eq!(remainder, "{\"leftover\":true}\n");
    }
}
