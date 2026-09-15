use p4_protocol::event::hop::{self, HopFrame, ReceiptStatus};
use p4_protocol::event::{Event, decode, encode};
use std::collections::{HashMap, VecDeque};
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
    finishing: bool,
    hop: Option<HopState>,
}

struct SentEvent {
    digest: hop::EventDigest,
    event: Event,
}

struct HopState {
    sender_id: String,
    generation: u64,
    next_attempt: u64,
    max_outstanding: usize,
    handshaken: bool,
    peer_sender_id: Option<String>,
    peer_generation: Option<u64>,
    sent: HashMap<u64, SentEvent>,
    received: HashMap<u64, hop::EventDigest>,
    pending: VecDeque<Event>,
    rejected_local: Vec<Event>,
    confirmed: Vec<String>,
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
            finishing: false,
            hop: None,
        }
    }

    pub fn acknowledged(
        reader: R,
        writer: W,
        sender_id: String,
        generation: u64,
    ) -> io::Result<Self> {
        if sender_id.is_empty() || generation == 0 {
            return Err(io::Error::other(
                "acknowledged EventWire requires identity and generation",
            ));
        }
        Ok(Self {
            reader,
            writer,
            buffer: Vec::new(),
            cursor: 0,
            finishing: false,
            hop: Some(HopState {
                sender_id,
                generation,
                next_attempt: 1,
                max_outstanding: 256,
                handshaken: false,
                sent: HashMap::new(),
                received: HashMap::new(),
                pending: VecDeque::new(),
                rejected_local: Vec::new(),
                confirmed: Vec::new(),
                peer_sender_id: None,
                peer_generation: None,
            }),
        })
    }

    pub fn acknowledged_mode(&self) -> bool {
        self.hop.is_some()
    }

    pub fn take_confirmed(&mut self) -> Vec<String> {
        self.hop
            .as_mut()
            .map(|state| std::mem::take(&mut state.confirmed))
            .unwrap_or_default()
    }

    pub async fn send(&mut self, event: Event) -> io::Result<()> {
        if self.finishing {
            return Err(io::Error::other("event connection is finishing"));
        }
        if self.hop.is_some() {
            self.ensure_hop().await?;
            while self.hop.as_ref().expect("hop mode").sent.len()
                >= self.hop.as_ref().expect("hop mode").max_outstanding
            {
                self.progress_hop().await?;
            }
            let bytes = match encode(&event) {
                Ok(value) => value,
                Err(error) => {
                    self.hop
                        .as_mut()
                        .expect("hop mode")
                        .rejected_local
                        .push(event);
                    return Err(io::Error::other(error));
                }
            };
            let digest = hop::event_digest(&bytes);
            let attempt = self.hop.as_ref().expect("hop mode").next_attempt;
            let frame = hop::encode(&HopFrame::Data {
                attempt,
                digest,
                event: bytes,
            })
            .map_err(io::Error::other)?;
            let state = self.hop.as_mut().expect("hop mode");
            state.next_attempt = state
                .next_attempt
                .checked_add(1)
                .ok_or_else(|| io::Error::other("hop attempt identity exhausted"))?;
            state.sent.insert(attempt, SentEvent { digest, event });
            return self.write_body(&frame).await;
        }
        let bytes = encode(&event).map_err(io::Error::other)?;
        let size = u32::try_from(bytes.len()).map_err(|_| io::Error::other("event too large"))?;
        self.writer.write_u32_le(size).await?;
        self.writer.write_all(&bytes).await?;
        self.writer.flush().await
    }

    /// Retire this socket's routes after the caller has consumed its expected
    /// outputs. FINISH is not cancellation, node drain, or remote settlement.
    pub async fn finish(&mut self, deadline: Instant) -> io::Result<()> {
        if self.finishing {
            return Err(io::Error::other("event connection already finishing"));
        }
        if self.hop.is_some() {
            self.ensure_hop().await?;
            while !self.hop.as_ref().expect("hop mode").sent.is_empty()
                || !self.hop.as_ref().expect("hop mode").received.is_empty()
            {
                let remaining = deadline.saturating_duration_since(Instant::now());
                tokio::time::timeout(remaining, self.progress_hop())
                    .await
                    .map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::TimedOut,
                            "hop receipt timeout before connection finish",
                        )
                    })??;
            }
            if !self.hop.as_ref().expect("hop mode").pending.is_empty() {
                return Err(io::Error::other(
                    "unconsumed events before connection finish",
                ));
            }
        }
        if self.cursor != self.buffer.len() {
            return Err(io::Error::other(
                "unconsumed events before connection finish",
            ));
        }
        self.finishing = true;
        let remaining = deadline.saturating_duration_since(Instant::now());
        tokio::time::timeout(remaining, async {
            self.writer.write_u32_le(0).await?;
            self.writer.flush().await
        })
        .await
        .map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "connection finish write timeout")
        })??;
        self.buffer.clear();
        self.cursor = 0;
        loop {
            if self.buffer.len() >= HEADER {
                let size = u32::from_le_bytes(
                    self.buffer[..HEADER]
                        .try_into()
                        .expect("four-byte finish prefix"),
                ) as usize;
                if size == 0 {
                    if self.buffer.len() != HEADER {
                        return Err(io::Error::other(format!(
                            "bytes after connection finish ACK: buffered_bytes={}",
                            self.buffer.len()
                        )));
                    }
                    return Ok(());
                }
                if size > MAX_FRAME {
                    return Err(io::Error::other(format!(
                        "unexpected output before connection finish ACK exceeds frame bound: declared_bytes={size} buffered_bytes={}",
                        self.buffer.len()
                    )));
                }
                let frame_bytes = HEADER + size;
                if self.buffer.len() >= frame_bytes {
                    return Err(io::Error::other(format!(
                        "unexpected output before connection finish ACK: frame_bytes={frame_bytes} buffered_bytes={}",
                        self.buffer.len()
                    )));
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let read = match tokio::time::timeout(remaining, self.reader.read_buf(&mut self.buffer))
                .await
            {
                Ok(result) => result?,
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "connection finish response timeout: buffered_bytes={}",
                            self.buffer.len()
                        ),
                    ));
                }
            };
            if read == 0 {
                let message = if self.buffer.is_empty() {
                    "connection ended without finish ACK".into()
                } else {
                    format!(
                        "connection ended during finish response: buffered_bytes={}",
                        self.buffer.len()
                    )
                };
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, message));
            }
        }
    }

    /// The next event, or `TimedOut` at the deadline with the buffer intact.
    pub async fn receive(&mut self, deadline: Instant) -> io::Result<Event> {
        if self.hop.is_some() {
            self.ensure_hop().await?;
            loop {
                if let Some(event) = self.hop.as_mut().expect("hop mode").pending.pop_front() {
                    return Ok(event);
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                tokio::time::timeout(remaining, self.progress_hop())
                    .await
                    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "event timeout"))??;
            }
        }
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

    async fn ensure_hop(&mut self) -> io::Result<()> {
        if self.hop.as_ref().is_none_or(|state| state.handshaken) {
            return Ok(());
        }
        let state = self.hop.as_ref().expect("hop mode");
        let hello = hop::encode(&HopFrame::Hello {
            sender_id: state.sender_id.clone(),
            connection_generation: state.generation,
            max_outstanding: state.max_outstanding as u32,
            max_receipt_bytes: 64 * 1024 * 1024,
        })
        .map_err(io::Error::other)?;
        self.write_body(&hello).await?;
        let body = self.read_body().await?;
        let frame = hop::decode(&body)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("peer does not support acknowledged hop transport"))?;
        match frame {
            HopFrame::HelloAck {
                accepted_connection_generation,
                sender_id,
                connection_generation,
                max_outstanding,
                ..
            } if accepted_connection_generation
                == self.hop.as_ref().expect("hop mode").generation =>
            {
                let state = self.hop.as_mut().expect("hop mode");
                state.max_outstanding = state.max_outstanding.min(max_outstanding as usize).max(1);
                state.handshaken = true;
                state.peer_sender_id = Some(sender_id);
                state.peer_generation = Some(connection_generation);
                Ok(())
            }
            HopFrame::HelloAck { .. } => Err(io::Error::other("hop hello ACK generation mismatch")),
            _ => Err(io::Error::other("peer did not acknowledge hop hello")),
        }
    }

    async fn progress_hop(&mut self) -> io::Result<()> {
        let body = self.read_body().await?;
        let frame = hop::decode(&body)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("legacy Event after hop hello"))?;
        match frame {
            HopFrame::Receipt {
                attempt,
                digest,
                status,
                detail,
            } => {
                let state = self.hop.as_mut().expect("hop mode");
                let sent = state
                    .sent
                    .get(&attempt)
                    .ok_or_else(|| io::Error::other("receipt has no outstanding attempt"))?;
                if sent.digest != digest {
                    return Err(io::Error::other("receipt digest conflict"));
                }
                if status != ReceiptStatus::AcceptedExact {
                    return Err(io::Error::other(format!(
                        "hop receipt refused Event: {status:?}: {detail}"
                    )));
                }
                let confirmed = state
                    .sent
                    .remove(&attempt)
                    .expect("checked outstanding receipt");
                state.confirmed.push(confirmed.event.envelope.event_id);
                let state = self.hop.as_ref().expect("hop mode");
                let ack = hop::encode(&HopFrame::ReceiptAck {
                    sender_id: state.sender_id.clone(),
                    connection_generation: state.generation,
                    attempt,
                    digest,
                })
                .map_err(io::Error::other)?;
                self.write_body(&ack).await
            }
            HopFrame::Data {
                attempt,
                digest,
                event: bytes,
            } => {
                let existing = self
                    .hop
                    .as_ref()
                    .expect("hop mode")
                    .received
                    .get(&attempt)
                    .copied();
                if let Some(existing) = existing {
                    let status = if existing == digest {
                        ReceiptStatus::AcceptedExact
                    } else {
                        ReceiptStatus::Conflict
                    };
                    let receipt = hop::encode(&HopFrame::Receipt {
                        attempt,
                        digest,
                        status,
                        detail: if status == ReceiptStatus::Conflict {
                            "attempt digest differs".into()
                        } else {
                            String::new()
                        },
                    })
                    .map_err(io::Error::other)?;
                    return self.write_body(&receipt).await;
                }
                let event = decode(&bytes).map_err(io::Error::other)?;
                let state = self.hop.as_mut().expect("hop mode");
                state.received.insert(attempt, digest);
                state.pending.push_back(event);
                let receipt = hop::encode(&HopFrame::Receipt {
                    attempt,
                    digest,
                    status: ReceiptStatus::AcceptedExact,
                    detail: String::new(),
                })
                .map_err(io::Error::other)?;
                self.write_body(&receipt).await
            }
            HopFrame::ReceiptAck {
                sender_id,
                connection_generation,
                attempt,
                digest,
            } => {
                let state = self.hop.as_mut().expect("hop mode");
                if state.peer_sender_id.as_deref() == Some(sender_id.as_str())
                    && state.peer_generation == Some(connection_generation)
                    && state
                        .received
                        .get(&attempt)
                        .is_some_and(|value| *value == digest)
                {
                    state.received.remove(&attempt);
                }
                Ok(())
            }
            HopFrame::Query {
                attempt, digest, ..
            } => {
                let known = self
                    .hop
                    .as_ref()
                    .expect("hop mode")
                    .received
                    .get(&attempt)
                    .copied();
                let status = match known {
                    Some(value) if value == digest => ReceiptStatus::AcceptedExact,
                    Some(_) => ReceiptStatus::Conflict,
                    None => ReceiptStatus::Unknown,
                };
                let result = hop::encode(&HopFrame::QueryResult {
                    attempt,
                    digest,
                    status,
                    detail: if status == ReceiptStatus::Unknown {
                        "receipt is not pinned".into()
                    } else if status == ReceiptStatus::Conflict {
                        "attempt digest differs".into()
                    } else {
                        String::new()
                    },
                })
                .map_err(io::Error::other)?;
                self.write_body(&result).await
            }
            HopFrame::QueryResult { .. } => Err(io::Error::other("unsolicited hop query result")),
            HopFrame::Hello { .. } | HopFrame::HelloAck { .. } => {
                Err(io::Error::other("hop hello repeated after handshake"))
            }
        }
    }

    async fn write_body(&mut self, bytes: &[u8]) -> io::Result<()> {
        let size = u32::try_from(bytes.len()).map_err(|_| io::Error::other("event too large"))?;
        if bytes.len() > MAX_FRAME {
            return Err(io::Error::other("event too large"));
        }
        self.writer.write_u32_le(size).await?;
        self.writer.write_all(bytes).await?;
        self.writer.flush().await
    }

    async fn read_body(&mut self) -> io::Result<Vec<u8>> {
        loop {
            if let Some(body) = self.take_body()? {
                return Ok(body);
            }
            let read = self.reader.read_buf(&mut self.buffer).await?;
            if read == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "event stream ended mid-frame",
                ));
            }
        }
    }

    fn take_body(&mut self) -> io::Result<Option<Vec<u8>>> {
        if self.cursor > 0 && (self.cursor >= RECLAIM || self.cursor == self.buffer.len()) {
            self.buffer.drain(..self.cursor);
            self.cursor = 0;
        }
        let available = self.buffer.len() - self.cursor;
        if available < HEADER {
            return Ok(None);
        }
        let size = u32::from_le_bytes(
            self.buffer[self.cursor..self.cursor + HEADER]
                .try_into()
                .expect("four-byte frame prefix"),
        ) as usize;
        if size == 0 || size > MAX_FRAME {
            return Err(io::Error::other("invalid event size"));
        }
        if available < HEADER + size {
            return Ok(None);
        }
        let start = self.cursor + HEADER;
        let body = self.buffer[start..start + size].to_vec();
        self.cursor = start + size;
        Ok(Some(body))
    }

    /// One whole frame if the buffer holds one, leaving the remainder in place.
    fn take_frame(&mut self) -> io::Result<Option<Event>> {
        self.take_body()?
            .map(|body| decode(&body).map_err(io::Error::other))
            .transpose()
    }
}

#[cfg(test)]
mod tests;
