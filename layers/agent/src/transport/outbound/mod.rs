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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{Mutex, mpsc};

/// How many frames may wait for one peer before sending refuses.
///
/// Bounded per peer rather than globally, so one unreachable machine cannot
/// consume the memory every other peer would need.
const PER_PEER_DEPTH: usize = 4096;

/// How long a peer may be silent before its connection and its entry are let
/// go.
///
/// Without this the map is append-only in the number of addresses ever seen.
/// A fleet has few, so it looked bounded; a caller whose address changes — a
/// restarted OUTER, an ephemeral port, anything behind a rotating gateway —
/// makes it grow for as long as the process lives, along with a task and a
/// socket each. That is the shape of leak that only shows up in the run nobody
/// restarts.
const IDLE: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct Peers {
    inner: Arc<Mutex<HashMap<Address, mpsc::Sender<Frame>>>>,
    idle: Duration,
}

impl Default for Peers {
    fn default() -> Self {
        Self::with_idle(IDLE)
    }
}

impl Peers {
    /// The same thing with a chosen idle window, so a test can watch a peer
    /// retire without waiting a minute for it.
    pub fn with_idle(idle: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            idle,
        }
    }
}

impl Peers {
    /// Hands a frame to the connection for its target, opening one if this is
    /// the first traffic to that address.
    ///
    /// Returns the frame back when the peer's queue is full, so the caller can
    /// answer the route rather than let it hang.
    /// Waits for room rather than refusing.
    ///
    /// Refusing looked safe and was not. A frame here is usually already a
    /// reply, and a reply has no reply address of its own — so there was
    /// nothing to answer with, and the frame went out silently. That reads
    /// exactly like a lost route, and it is one.
    ///
    /// Waiting is the backpressure this design already relies on everywhere
    /// else: a full peer queue holds the dispatcher, which fills the lanes,
    /// which holds the node's outbox, which slows the node at its next hop.
    /// The chain ends at the thing producing the work, which is where it
    /// belongs. A peer that is genuinely gone is answered by the deadline, and
    /// its pump keeps trying to reconnect meanwhile.
    pub async fn send(&self, frame: Frame) -> Result<(), Frame> {
        let target = frame.envelope.target.clone();
        let sender = self.connection(&target).await;
        match sender.send(frame).await {
            Ok(()) => Ok(()),
            // The pump retired between being handed out and being used. That
            // means idle, not gone, so the frame gets a fresh connection —
            // turning it into a refusal would let idleness lose work.
            Err(returned) => {
                let sender = self.connection(&target).await;
                sender.send(returned.0).await.map_err(|error| error.0)
            }
        }
    }

    async fn connection(&self, target: &Address) -> mpsc::Sender<Frame> {
        let mut peers = self.inner.lock().await;
        if let Some(existing) = peers.get(target)
            && !existing.is_closed()
        {
            return existing.clone();
        }
        let (sender, receiver) = mpsc::channel(PER_PEER_DEPTH);
        peers.insert(target.clone(), sender.clone());
        tokio::spawn(pump(
            target.clone(),
            receiver,
            Arc::downgrade(&self.inner),
            sender.clone(),
            self.idle,
        ));
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
async fn pump(
    target: Address,
    mut frames: mpsc::Receiver<Frame>,
    peers: Weak<Mutex<HashMap<Address, mpsc::Sender<Frame>>>>,
    mine: mpsc::Sender<Frame>,
    idle: Duration,
) {
    let mut live: Option<Live> = None;
    loop {
        let frame = match tokio::time::timeout(idle, frames.recv()).await {
            Ok(Some(frame)) => frame,
            // The map was dropped, or this peer was replaced.
            Ok(None) => return,
            Err(_) => {
                if retire(&peers, &target, &mine).await {
                    return;
                }
                continue;
            }
        };
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

/// Lets a silent peer go, and says whether it did.
///
/// Done under the map's lock so it cannot race a caller taking the sender out
/// of the map: a caller either gets it before the removal and finds a closed
/// channel — which `send` answers by reconnecting — or arrives after and
/// builds a fresh one. Two guards keep a busy peer from retiring under its own
/// traffic: the entry must still be this pump's channel, and that channel must
/// be empty.
async fn retire(
    peers: &Weak<Mutex<HashMap<Address, mpsc::Sender<Frame>>>>,
    target: &Address,
    mine: &mpsc::Sender<Frame>,
) -> bool {
    let Some(peers) = peers.upgrade() else {
        return true;
    };
    let mut peers = peers.lock().await;
    match peers.get(target) {
        Some(current) if current.same_channel(mine) && mine.capacity() == PER_PEER_DEPTH => {
            peers.remove(target);
            true
        }
        // Already replaced by a newer pump: this one is redundant either way.
        Some(_) => false,
        None => true,
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
