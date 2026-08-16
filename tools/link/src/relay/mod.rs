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
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// How much is moved at once. Small enough that latency is felt per frame
/// rather than per burst, which is how a real link behaves for this traffic.
const CHUNK: usize = 16 * 1024;

/// Accepts connections and joins each to `target` through `link`.
///
/// Runs until the listener closes, so a caller holds it as a task.
pub async fn serve(listener: TcpListener, target: String, link: Impairment) {
    let carried = Arc::new(AtomicU64::new(0));
    loop {
        let Ok((inbound, _)) = listener.accept().await else {
            return;
        };
        let target = target.clone();
        let carried = Arc::clone(&carried);
        tokio::spawn(async move {
            let Ok(outbound) = TcpStream::connect(&target).await else {
                return;
            };
            let _ = inbound.set_nodelay(true);
            let _ = outbound.set_nodelay(true);
            join(inbound, outbound, link, carried).await;
        });
    }
}

/// Total bytes a relay has carried, for a scenario that wants to assert the
/// traffic actually went through it rather than around it.
pub fn carried(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

async fn join(inbound: TcpStream, outbound: TcpStream, link: Impairment, carried: Arc<AtomicU64>) {
    let (client_read, client_write) = inbound.into_split();
    let (server_read, server_write) = outbound.into_split();
    let upstream = tokio::spawn(pump(
        client_read,
        server_write,
        link,
        Arc::clone(&carried),
        0,
    ));
    let downstream = tokio::spawn(pump(server_read, client_write, link, carried, 1));
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
) {
    let mut buffer = vec![0u8; CHUNK];
    let mut sequence = direction;
    loop {
        let read = match from.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
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
