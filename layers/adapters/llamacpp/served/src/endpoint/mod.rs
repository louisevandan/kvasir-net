//! Where the backend is, and the least HTTP that reaches it.
//!
//! `llama-server` speaks HTTP/1.1 and answers a streamed completion as
//! server-sent events. That is a small enough surface to write against the
//! standard library, and writing it that way keeps this crate's dependencies
//! to the one that parses JSON — which is the part worth not hand-rolling,
//! because the bytes come from somewhere else and escapes are where hand-rolled
//! parsers are wrong.
//!
//! Everything here blocks. The adapter interface is a procedure and the node
//! runs it on a blocking thread precisely because a real backend waits on a
//! device; an async client would buy nothing and cost a runtime.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

/// A backend's address, and how patient to be with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    /// How long to wait on a socket that has gone quiet.
    ///
    /// A first token can be slow — a long prompt on a busy device — so this is
    /// generous. What it protects against is the backend dying mid-stream,
    /// where the alternative is a node that never sees its hop end.
    pub idle: Duration,
}

impl Endpoint {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            idle: Duration::from_secs(120),
        }
    }

    /// Parses `host:port`, the form a plan carries.
    pub fn parse(value: &str) -> Option<Self> {
        let (host, port) = value.trim().rsplit_once(':')?;
        if host.is_empty() {
            return None;
        }
        Some(Self::new(host, port.parse().ok()?))
    }

    fn connect(&self) -> std::io::Result<TcpStream> {
        let stream = TcpStream::connect((self.host.as_str(), self.port))?;
        stream.set_read_timeout(Some(self.idle))?;
        stream.set_write_timeout(Some(self.idle))?;
        stream.set_nodelay(true)?;
        Ok(stream)
    }

    /// Sends a JSON POST and returns the whole body.
    pub fn post(&self, path: &str, body: &str) -> Result<String, String> {
        let mut stream = self.begin(path, body)?;
        let mut reader = BufReader::new(&mut stream);
        let headers = read_headers(&mut reader)?;
        let answer = read_body(&mut reader, &headers)?;
        check_status(&headers, &answer)?;
        Ok(answer)
    }

    /// Sends a JSON POST and hands back the socket, for a caller reading a
    /// stream of events off it.
    pub fn stream(&self, path: &str, body: &str) -> Result<Events, String> {
        let stream = self.begin(path, body)?;
        let mut reader = BufReader::new(stream);
        let headers = read_headers(&mut reader)?;
        if check_status(&headers, "").is_err() {
            // Read what it said before reporting it. A refusal carries its
            // reason in the body, and the code alone sends a caller looking in
            // the wrong place.
            let body = read_body(&mut reader, &headers).unwrap_or_default();
            check_status(&headers, &body)?;
        }
        Ok(Events { reader })
    }

    /// A plain GET, for asking whether anything is there.
    pub fn get(&self, path: &str) -> Result<String, String> {
        let mut stream = self.connect().map_err(|error| error.to_string())?;
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.host
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        let mut reader = BufReader::new(&mut stream);
        let headers = read_headers(&mut reader)?;
        let answer = read_body(&mut reader, &headers)?;
        check_status(&headers, &answer)?;
        Ok(answer)
    }

    fn begin(&self, path: &str, body: &str) -> Result<TcpStream, String> {
        let mut stream = self.connect().map_err(|error| error.to_string())?;
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Accept: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host,
            body.len(),
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| error.to_string())?;
        stream.flush().map_err(|error| error.to_string())?;
        Ok(stream)
    }
}

/// The `data:` lines of a server-sent event stream, one at a time.
pub struct Events {
    reader: BufReader<TcpStream>,
}

impl Events {
    /// The next event's payload, or `None` once the stream ends.
    ///
    /// Blank lines separate events and are skipped; `[DONE]` is the end of the
    /// stream rather than a payload.
    pub fn event(&mut self) -> Result<Option<String>, String> {
        loop {
            let mut line = String::new();
            let read = self
                .reader
                .read_line(&mut line)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                return Ok(None);
            }
            let line = line.trim_end_matches(['\r', '\n']);
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            if payload == "[DONE]" {
                return Ok(None);
            }
            if !payload.is_empty() {
                return Ok(Some(payload.to_owned()));
            }
        }
    }
}

/// Refuses anything that is not a success.
///
/// Read from the status line, which is the only place it is stated. Without
/// this a `503 Loading model` — what `llama-server` answers while a large
/// model is still coming off disk — read as a stream that ended having
/// produced nothing: every request "completed", every verdict passed, and no
/// token was ever generated. A backend saying no must not look like a backend
/// saying nothing.
fn check_status(headers: &[String], body: &str) -> Result<(), String> {
    let status = headers.first().map(String::as_str).unwrap_or_default();
    let code = status
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    if (200..300).contains(&code) {
        return Ok(());
    }
    // The backend's own words when it gave them, the status line when it did
    // not. Either is more use than the code alone.
    let detail = crate::chat::failure(body).unwrap_or_else(|| status.to_owned());
    Err(format!("backend answered {code}: {detail}"))
}

/// Status line and headers, lower-cased so a caller can look one up.
fn read_headers(reader: &mut impl BufRead) -> Result<Vec<String>, String> {
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("backend closed before answering".into());
        }
        let line = line.trim_end_matches(['\r', '\n']).to_ascii_lowercase();
        if line.is_empty() {
            return Ok(headers);
        }
        headers.push(line);
    }
}

/// Reads a body, honouring the two framings `llama-server` uses.
fn read_body(reader: &mut impl BufRead, headers: &[String]) -> Result<String, String> {
    if headers.iter().any(|header| header.contains("chunked")) {
        return read_chunked(reader);
    }
    if let Some(length) = headers
        .iter()
        .find_map(|header| header.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
    {
        let mut body = vec![0u8; length];
        reader
            .read_exact(&mut body)
            .map_err(|error| error.to_string())?;
        return String::from_utf8(body).map_err(|error| error.to_string());
    }
    // Neither framing: the body runs to the close, which `Connection: close`
    // makes well defined.
    let mut body = String::new();
    reader
        .read_to_string(&mut body)
        .map_err(|error| error.to_string())?;
    Ok(body)
}

fn read_chunked(reader: &mut impl BufRead) -> Result<String, String> {
    let mut body = String::new();
    loop {
        let mut size = String::new();
        reader
            .read_line(&mut size)
            .map_err(|error| error.to_string())?;
        let size = usize::from_str_radix(size.trim(), 16).map_err(|error| error.to_string())?;
        if size == 0 {
            return Ok(body);
        }
        let mut chunk = vec![0u8; size];
        reader
            .read_exact(&mut chunk)
            .map_err(|error| error.to_string())?;
        body.push_str(&String::from_utf8_lossy(&chunk));
        let mut discard = String::new();
        let _ = reader.read_line(&mut discard);
    }
}

#[cfg(test)]
mod tests;
