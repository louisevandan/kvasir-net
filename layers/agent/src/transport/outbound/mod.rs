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
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
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
    let mut socket: Option<TcpStream> = None;
    while let Some(frame) = frames.recv().await {
        let Ok(bytes) = frame::encode(&frame.envelope, &frame.body) else {
            continue;
        };
        for attempt in 0..2 {
            if socket.is_none() {
                socket = connect(&target).await;
            }
            let Some(stream) = socket.as_mut() else {
                break;
            };
            if stream.write_all(&bytes).await.is_ok() {
                break;
            }
            socket = None;
            if attempt == 1 {
                eprintln!("P4_AGENT_SEND_FAILED target={target}");
            }
        }
    }
}

async fn connect(target: &Address) -> Option<TcpStream> {
    let stream = TcpStream::connect((target.host.as_str(), target.port))
        .await
        .ok()?;
    let _ = stream.set_nodelay(true);
    Some(stream)
}

#[cfg(test)]
mod tests;
