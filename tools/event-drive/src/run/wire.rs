use p4_protocol::event::{Event, decode, encode};
use std::io;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_FRAME: usize = 2 * 1024 * 1024 * 1024;

pub struct EventWire<R, W> {
    reader: R,
    writer: W,
}

impl<R, W> EventWire<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    pub async fn send(&mut self, event: Event) -> io::Result<()> {
        let bytes = encode(&event).map_err(io::Error::other)?;
        let size = u32::try_from(bytes.len()).map_err(|_| io::Error::other("event too large"))?;
        self.writer.write_u32_le(size).await?;
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await
    }

    pub async fn receive(&mut self, deadline: Instant) -> io::Result<Event> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "event timeout"));
        }
        tokio::time::timeout(remaining, self.read())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "event timeout"))?
    }

    async fn read(&mut self) -> io::Result<Event> {
        let size = self.reader.read_u32_le().await? as usize;
        if size == 0 || size > MAX_FRAME {
            return Err(io::Error::other("invalid event size"));
        }
        let mut bytes = vec![0; size];
        self.reader.read_exact(&mut bytes).await?;
        decode(&bytes).map_err(io::Error::other)
    }
}
