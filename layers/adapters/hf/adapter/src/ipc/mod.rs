use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const COMMAND: &str = "application/vnd.p4.hf.command-v2";
pub const RESULT: &str = "application/vnd.p4.hf.result-v2";
pub const HEADER_LIMIT: usize = 65536;
pub const FRAME_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub generation: u64,
    pub epoch: u64,
    pub serial: u64,
    pub kind: String,
    pub request: String,
    pub issue: u64,
    pub position: u64,
}

pub fn unpack(bytes: &[u8]) -> Result<(Value, &[u8]), String> {
    let prefix: [u8; 4] = bytes
        .get(..4)
        .ok_or("missing metadata")?
        .try_into()
        .unwrap();
    let n = u32::from_be_bytes(prefix) as usize;
    if n == 0 || n > HEADER_LIMIT || n + 4 > bytes.len() {
        return Err("invalid metadata length".into());
    }
    let meta: Value = serde_json::from_slice(&bytes[4..4 + n]).map_err(|e| e.to_string())?;
    if !meta.is_object() {
        return Err("metadata must be an object".into());
    }
    Ok((meta, &bytes[4 + n..]))
}

pub fn pack(meta: &Value, body: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let header = serde_json::to_vec(meta).map_err(|e| e.to_string())?;
    if header.len() > HEADER_LIMIT || header.len() + 4 + body.len() > limit {
        return Err("packet exceeds negotiated bound".into());
    }
    let mut out = Vec::with_capacity(4 + header.len() + body.len());
    out.extend_from_slice(&(header.len() as u32).to_be_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(body);
    Ok(out)
}

pub async fn read<R: AsyncRead + Unpin>(
    reader: &mut R,
    limit: usize,
    retained: &mut Vec<u8>,
) -> Result<Vec<u8>, String> {
    retained.clear();
    async fn fill<R: AsyncRead + Unpin>(
        reader: &mut R,
        bytes: &mut Vec<u8>,
        end: usize,
    ) -> Result<(), String> {
        let mut chunk = [0u8; 4096];
        while bytes.len() < end {
            let count = (end - bytes.len()).min(chunk.len());
            let n = reader
                .read(&mut chunk[..count])
                .await
                .map_err(|e| format!("partial frame: {e}"))?;
            if n == 0 {
                return Err("partial frame: EOF".into());
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        Ok(())
    }
    fill(reader, retained, 16).await?;
    let header = &retained[..16];
    if &header[..8] != b"P4HF\x01\0\0\0" {
        return Err("invalid frame magic/version/reserved".into());
    }
    let n = u64::from_be_bytes(header[8..].try_into().unwrap());
    if n == 0 || n > limit as u64 {
        return Err("frame exceeds negotiated bound".into());
    }
    fill(reader, retained, 16 + n as usize).await?;
    Ok(retained[16..].to_vec())
}

pub async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    bytes: &[u8],
    limit: usize,
) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err("outgoing frame exceeds bound".into());
    }
    writer
        .write_all(b"P4HF\x01\0\0\0")
        .await
        .map_err(|e| e.to_string())?;
    writer
        .write_all(&(bytes.len() as u64).to_be_bytes())
        .await
        .map_err(|e| e.to_string())?;
    writer
        .write_all(bytes)
        .await
        .map_err(|e| format!("partial write: {e}"))?;
    writer.flush().await.map_err(|e| e.to_string())
}
