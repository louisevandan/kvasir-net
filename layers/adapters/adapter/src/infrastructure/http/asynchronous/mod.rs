use serde_json::Value;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

type AsyncError = Box<dyn std::error::Error + Send + Sync>;

pub(crate) struct SseResponse {
    reader: BufReader<TcpStream>,
    chunked: bool,
    pending: Vec<u8>,
    done: bool,
}

pub(crate) async fn open_sse(
    endpoint: &str,
    path: &str,
    body: &Value,
) -> Result<SseResponse, AsyncError> {
    let (host, port) = endpoint_parts(endpoint)?;
    let encoded = body.to_string();
    let mut stream = TcpStream::connect((host.as_str(), port)).await?;
    stream.set_nodelay(true)?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: text/event-stream\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        encoded.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(encoded.as_bytes()).await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).await?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("missing HTTP status")?
        .parse()?;
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
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
    if status != 200 {
        return Err(format!("host returned HTTP {status}").into());
    }
    let chunked = headers
        .get("transfer-encoding")
        .is_some_and(|value| value.contains("chunked"));
    Ok(SseResponse {
        reader,
        chunked,
        pending: Vec::new(),
        done: false,
    })
}

impl SseResponse {
    pub(crate) async fn next_data(&mut self) -> Result<Option<String>, AsyncError> {
        loop {
            if let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
                let mut line = self.pending.drain(..=index).collect::<Vec<_>>();
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                let line = std::str::from_utf8(&line)?;
                if let Some(data) = line.strip_prefix("data: ") {
                    return Ok(Some(data.to_string()));
                }
                continue;
            }
            if self.done {
                return Ok(None);
            }
            if self.chunked {
                self.read_chunk().await?;
            } else {
                let mut bytes = Vec::new();
                if self.reader.read_until(b'\n', &mut bytes).await? == 0 {
                    self.done = true;
                }
                self.pending.extend_from_slice(&bytes);
            }
        }
    }

    async fn read_chunk(&mut self) -> Result<(), AsyncError> {
        let mut size_line = String::new();
        self.reader.read_line(&mut size_line).await?;
        let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or(""), 16)?;
        if size == 0 {
            self.done = true;
            return Ok(());
        }
        let start = self.pending.len();
        self.pending.resize(start + size, 0);
        self.reader.read_exact(&mut self.pending[start..]).await?;
        let mut ending = [0u8; 2];
        self.reader.read_exact(&mut ending).await?;
        if ending != *b"\r\n" {
            return Err("invalid HTTP chunk ending".into());
        }
        Ok(())
    }
}

fn endpoint_parts(value: &str) -> Result<(String, u16), AsyncError> {
    let value = value.strip_prefix("http://").unwrap_or(value);
    let (host, port) = value.rsplit_once(':').ok_or("endpoint must be host:port")?;
    Ok((host.into(), port.parse()?))
}

#[cfg(test)]
mod tests;
