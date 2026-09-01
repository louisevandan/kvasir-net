use p4_protocol::event::{Event, decode, encode};
use std::io;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_FRAME: usize = 2 * 1024 * 1024 * 1024;
const HEADER: usize = 4;
/// Parsed bytes tolerated before the buffer is compacted.
const RECLAIM: usize = 1024 * 1024;

/// A framed event stream whose reads survive being timed out.
///
/// The buffer is the whole point. `read_u32_le` and `read_exact` are not
/// cancel-safe: dropped part-way they take the bytes they had already consumed
/// with them, and the next read starts inside a frame it cannot recognise. The
/// drive times out on purpose - once per arrival wave, to go send the next one
/// - so with an unbuffered reader every wave boundary was a chance to desync,
/// after which a garbage length prefix consumed an arbitrary run of events and
/// the stream silently resumed on the far side of them.
///
/// That is what a 2026-09-01 four-node run under continuous arrivals was
/// reporting as "output token positions are not contiguous": the adapter had
/// emitted every position, and the reader had eaten a stretch of them. Reading
/// into a buffer that outlives the cancellation makes a timeout cost nothing
/// but the wait.
pub struct EventWire<R, W> {
    reader: R,
    writer: W,
    buffer: Vec<u8>,
    /// How far into `buffer` the parsed frames reach. Draining from the
    /// front instead would memmove the whole remainder once per frame, which
    /// on a run carrying tens of thousands of events is enough backpressure
    /// to change how the far side batches.
    cursor: usize,
}

impl<R, W> EventWire<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            buffer: Vec::new(),
            cursor: 0,
        }
    }

    pub async fn send(&mut self, event: Event) -> io::Result<()> {
        let bytes = encode(&event).map_err(io::Error::other)?;
        let size = u32::try_from(bytes.len()).map_err(|_| io::Error::other("event too large"))?;
        self.writer.write_u32_le(size).await?;
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await
    }

    /// The next event, or `TimedOut` at the deadline with the buffer intact.
    pub async fn receive(&mut self, deadline: Instant) -> io::Result<Event> {
        loop {
            if let Some(event) = self.take_frame()? {
                return Ok(event);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "event timeout"));
            }
            // read_buf appends to the buffer and is cancel-safe: a cancelled
            // read has either appended what it read or read nothing at all.
            let read = tokio::time::timeout(remaining, self.reader.read_buf(&mut self.buffer))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "event timeout"))??;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "event stream ended mid-frame",
                ));
            }
        }
    }

    /// One whole frame if the buffer holds one, leaving the remainder in place.
    fn take_frame(&mut self) -> io::Result<Option<Event>> {
        // Reclaim only once the parsed prefix is worth reclaiming, so the
        // copy is amortised instead of paid per frame.
        if self.cursor > 0 && (self.cursor >= RECLAIM || self.cursor == self.buffer.len()) {
            self.buffer.drain(..self.cursor);
            self.cursor = 0;
        }
        let available = self.buffer.len() - self.cursor;
        if available < HEADER {
            return Ok(None);
        }
        let size = u32::from_le_bytes([
            self.buffer[self.cursor],
            self.buffer[self.cursor + 1],
            self.buffer[self.cursor + 2],
            self.buffer[self.cursor + 3],
        ]) as usize;
        if size == 0 || size > MAX_FRAME {
            return Err(io::Error::other("invalid event size"));
        }
        if available < HEADER + size {
            return Ok(None);
        }
        let start = self.cursor + HEADER;
        let event = decode(&self.buffer[start..start + size]).map_err(io::Error::other)?;
        self.cursor = start + size;
        Ok(Some(event))
    }
}

#[cfg(test)]
mod tests;
