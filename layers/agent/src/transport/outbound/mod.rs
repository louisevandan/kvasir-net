//! Sending a frame to an address.
//!
//! One connection per peer, kept open and shared by everything going there.
//! The previous transport opened a socket per call and read until the work
//! finished, which put a connect and a teardown on every hop and held a worker
//! for the duration; a chain of any length paid that per link.
//!
//! Nothing here waits for a reply. A frame goes out and the call returns; what
//! comes back arrives as a fresh frame on the listener, like any other.

use p4_protocol::Address;
use p4_protocol::frame::{self, Frame};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{Mutex, mpsc};

/// How many frames may wait for one peer before sending refuses.
///
/// Bounded per peer rather than globally, so one unreachable machine cannot
/// consume the memory every other peer would need.
const PER_PEER_DEPTH: usize = 4096;

#[derive(Clone)]
pub struct Peers {
    inner: Arc<Mutex<HashMap<Address, mpsc::Sender<Frame>>>>,
}

impl Default for Peers {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Peers {
    /// Hands a frame to the connection for its target, opening one if this is
    /// the first traffic to that address.
    ///
    /// Returns the frame back when the peer's queue is full, so the caller can
    /// answer the route rather than let it hang.
    pub async fn send(&self, frame: Frame) -> Result<(), Frame> {
        let target = frame.envelope.target.clone();
        let sender = self.connection(&target).await;
        sender.try_send(frame).map_err(|error| match error {
            mpsc::error::TrySendError::Full(frame) => frame,
            mpsc::error::TrySendError::Closed(frame) => frame,
        })
    }

    async fn connection(&self, target: &Address) -> mpsc::Sender<Frame> {
        let mut peers = self.inner.lock().await;
        if let Some(existing) = peers.get(target) {
            if !existing.is_closed() {
                return existing.clone();
            }
        }
        let (sender, receiver) = mpsc::channel(PER_PEER_DEPTH);
        peers.insert(target.clone(), sender.clone());
        tokio::spawn(pump(target.clone(), receiver));
        sender
    }

    /// How many peers this agent is holding a connection to.
    pub async fn connected(&self) -> usize {
        self.inner.lock().await.len()
    }
}

/// Drains one peer's queue onto one socket.
///
/// Reconnects on failure rather than ending, because a peer restarting is
/// ordinary in a fleet and a dead sender would silently strand every later
/// frame for that address. Frames already written are not replayed: they may
/// have arrived, and a duplicate hop is worse than a lost one the deadline
/// will answer for.
async fn pump(target: Address, mut frames: mpsc::Receiver<Frame>) {
    let mut live: Option<Live> = None;
    while let Some(frame) = frames.recv().await {
        let Ok(bytes) = frame::encode(&frame.envelope, &frame.body) else {
            continue;
        };
        for attempt in 0..2 {
            // A peer that went away leaves a socket that still accepts a write
            // into its buffer, so the first frame after a restart is lost
            // unless the connection is known to be gone before it is used.
            if live.as_ref().is_none_or(Live::is_gone) {
                live = connect(&target).await;
            }
            let Some(connection) = live.as_mut() else {
                break;
            };
            if connection.writer.write_all(&bytes).await.is_ok() {
                break;
            }
            live = None;
            if attempt == 1 {
                eprintln!("P4_AGENT_SEND_FAILED target={target}");
            }
        }
    }
}

/// One connection, and whether the far end is still there.
struct Live {
    writer: OwnedWriteHalf,
    gone: Arc<AtomicBool>,
}

impl Live {
    fn is_gone(&self) -> bool {
        self.gone.load(Ordering::Relaxed)
    }
}

async fn connect(target: &Address) -> Option<Live> {
    let stream = TcpStream::connect((target.host.as_str(), target.port))
        .await
        .ok()?;
    let _ = stream.set_nodelay(true);
    let (mut reader, writer) = stream.into_split();
    let gone = Arc::new(AtomicBool::new(false));

    // These connections carry traffic one way: a peer answers on its own
    // connection back to us, never on this one. So anything readable here is
    // the far end closing, and noticing it is what keeps a restarted peer from
    // swallowing the next frame.
    let watch = Arc::clone(&gone);
    tokio::spawn(async move {
        let mut scratch = [0u8; 64];
        loop {
            match reader.read(&mut scratch).await {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
        watch.store(true, Ordering::Relaxed);
    });

    Some(Live { writer, gone })
}

#[cfg(test)]
mod tests;
