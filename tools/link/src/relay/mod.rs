//! A TCP relay that carries bytes badly on purpose.
//!
//! Agents are given the relay's address instead of each other's, so every
//! frame between them crosses a link with the declared latency, width and
//! stalls. Nothing in P4 knows it is there, which is the point: a degraded
//! network has to be something the layer meets, not something it is told
//! about.
//!
//! Bytes are never reordered or dropped. TCP does not do either, and a relay
//! that did would be testing a transport this layer does not have.

use crate::impairment::Impairment;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How much is moved at once. Small enough that latency is felt per frame
/// rather than per burst, which is how a real link behaves for this traffic.
const CHUNK: usize = 16 * 1024;

/// Whether the link exists at all.
///
/// A partition is not a slow link — it is no link — and the two fail
/// differently: a slow one delivers late, a cut one delivers never and the
/// deadline has to answer instead. Shared so a scenario can cut and restore
/// while traffic is in flight.
#[derive(Clone)]
pub struct Cut(Arc<AtomicBool>);

impl Default for Cut {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}

impl Cut {
    /// Takes the link away. Connections through it are closed and new ones are
    /// refused, which is what a peer sees when the machine on the far side
    /// stops being reachable.
    pub fn cut(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Puts it back.
    pub fn heal(&self) {
        self.0.store(false, Ordering::Relaxed);
    }

    fn is_cut(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Accepts connections and joins each to `target` through `link`.
///
/// Runs until the listener closes, so a caller holds it as a task.
pub async fn serve(listener: TcpListener, target: String, link: Impairment) {
    serve_with(listener, target, link, Cut::default()).await
}

/// The same, over a link that can be taken away and put back.
pub async fn serve_with(listener: TcpListener, target: String, link: Impairment, cut: Cut) {
    let carried = Arc::new(AtomicU64::new(0));
    loop {
        let Ok((inbound, _)) = listener.accept().await else {
            return;
        };
        if cut.is_cut() {
            drop(inbound);
            continue;
        }
        let target = target.clone();
        let carried = Arc::clone(&carried);
        let cut = cut.clone();
        tokio::spawn(async move {
            let Ok(outbound) = TcpStream::connect(&target).await else {
                return;
            };
            let _ = inbound.set_nodelay(true);
            let _ = outbound.set_nodelay(true);
            join(inbound, outbound, link, carried, cut).await;
        });
    }
}

/// Total bytes a relay has carried, for a scenario that wants to assert the
/// traffic actually went through it rather than around it.
pub fn carried(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

/// Completes once the link is taken away.
///
/// Polled rather than signalled: a cut is rare and a notifier would be more
/// machinery than the thing it watches.
async fn watch(cut: &Cut) {
    while !cut.is_cut() {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

async fn join(
    inbound: TcpStream,
    outbound: TcpStream,
    link: Impairment,
    carried: Arc<AtomicU64>,
    cut: Cut,
) {
    let (client_read, client_write) = inbound.into_split();
    let (server_read, server_write) = outbound.into_split();
    let upstream = tokio::spawn(pump(
        client_read,
        server_write,
        link,
        Arc::clone(&carried),
        0,
        cut.clone(),
    ));
    let downstream = tokio::spawn(pump(server_read, client_write, link, carried, 1, cut));
    // Either direction ending means the connection is over; dropping the other
    // task closes its halves and the far end sees the EOF it is watching for.
    tokio::select! {
        _ = upstream => {}
        _ = downstream => {}
    }
}

async fn pump(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    link: Impairment,
    carried: Arc<AtomicU64>,
    // Each direction counts separately, so the two do not share a jitter
    // sequence and a request is not delayed identically to its reply.
    direction: u64,
    cut: Cut,
) {
    let mut buffer = vec![0u8; CHUNK];
    let mut sequence = direction;
    loop {
        let read = tokio::select! {
            read = from.read(&mut buffer) => match read {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            },
            // Noticing the cut without traffic to carry, so a partition takes
            // the connection down rather than waiting for the next frame.
            _ = watch(&cut) => break,
        };
        if cut.is_cut() {
            break;
        }
        sequence = sequence.wrapping_add(2);
        if !link.is_perfect() {
            let held = link.hold(sequence, read);
            if !held.is_zero() {
                tokio::time::sleep(held).await;
            }
        }
        if to.write_all(&buffer[..read]).await.is_err() {
            break;
        }
        carried.fetch_add(read as u64, Ordering::Relaxed);
    }
    let _ = to.shutdown().await;
}
