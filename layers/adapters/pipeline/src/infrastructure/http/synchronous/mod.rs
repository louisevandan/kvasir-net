use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

pub fn json(
    endpoint: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Value, String), Box<dyn std::error::Error>> {
    let mut response = open(endpoint, method, path, body, "application/json")?;
    let mut text = String::new();
    response.body.read_to_string(&mut text)?;
    let parsed = serde_json::from_str(&text).unwrap_or(Value::Null);
    Ok((response.status, parsed, text))
}

struct Response {
    status: u16,
    body: Box<dyn BufRead>,
}

fn open(
    endpoint: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
    accept: &str,
) -> Result<Response, Box<dyn std::error::Error>> {
    let (host, port) = endpoint_parts(endpoint)?;
    let encoded = body.map(Value::to_string).unwrap_or_default();
    let mut stream = TcpStream::connect((host.as_str(), port))?;
    stream.set_nodelay(true)?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: {accept}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        encoded.len()
    );
    stream.write_all(head.as_bytes())?;
    if !encoded.is_empty() {
        stream.write_all(encoded.as_bytes())?;
    }
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("missing HTTP status")?
        .parse()?;
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(
                key.trim().to_ascii_lowercase(),
                value.trim().to_ascii_lowercase(),
            );
        }
    }
    let body: Box<dyn BufRead> = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.contains("chunked"))
    {
        Box::new(BufReader::new(ChunkedReader::new(reader)))
    } else {
        Box::new(reader)
    };
    Ok(Response { status, body })
}

fn endpoint_parts(value: &str) -> Result<(String, u16), Box<dyn std::error::Error>> {
    let value = value.strip_prefix("http://").unwrap_or(value);
    let (host, port) = value.rsplit_once(':').ok_or("endpoint must be host:port")?;
    Ok((host.into(), port.parse()?))
}

struct ChunkedReader<R: BufRead> {
    inner: R,
    remaining: usize,
    done: bool,
}
impl<R: BufRead> ChunkedReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            remaining: 0,
            done: false,
        }
    }
}
impl<R: BufRead> Read for ChunkedReader<R> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.done || out.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut line = String::new();
            self.inner.read_line(&mut line)?;
            self.remaining = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size")
                })?;
            if self.remaining == 0 {
                self.done = true;
                return Ok(0);
            }
        }
        let count = self.remaining.min(out.len());
        self.inner.read_exact(&mut out[..count])?;
        self.remaining -= count;
        if self.remaining == 0 {
            let mut ending = [0; 2];
            self.inner.read_exact(&mut ending)?;
        }
        Ok(count)
    }
}
