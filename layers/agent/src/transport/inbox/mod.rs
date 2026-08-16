//! The listener. It reads frames and puts them on the queue.
//!
//! That is the whole of it. No handler runs here, no body is decoded, and no
//! connection gets a thread of its own to do work in — the previous listener
//! spawned a thread per connection and called the handler inline, which put
//! every handler's duration inside the read loop.
//!
//! A frame's length comes from its header, so a reader knows when it has a
//! whole frame without decoding any of it.

use crate::queue::main::Sender;

use p4_protocol::frame::{self, Frame};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

const HEADER_BYTES: usize = 16;

/// Accepts connections until the listener is dropped.
///
/// `connections` bounds how many sockets are held at once, separately from how
/// deep the queue is and how much is in flight.
pub async fn serve(listener: TcpListener, queue: Sender, connections: usize) {
    let permits = Arc::new(Semaphore::new(connections));
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            // At the connection ceiling. Closing immediately is the honest
            // answer; holding the socket unread would look like a hang.
            drop(stream);
            continue;
        };
        let queue = queue.clone();
        tokio::spawn(async move {
            let _permit = permit;
            read_frames(stream, queue).await;
        });
    }
}

async fn read_frames(mut stream: TcpStream, queue: Sender) {
    let mut refusals = Refusals::default();
    let _ = stream.set_nodelay(true);
    loop {
        let mut header = [0u8; HEADER_BYTES];
        if stream.read_exact(&mut header).await.is_err() {
            return;
        }
        let Ok(total) = frame::frame_len(&header) else {
            return;
        };
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&header);
        bytes.resize(total, 0);
        if stream.read_exact(&mut bytes[HEADER_BYTES..]).await.is_err() {
            return;
        }
        let Ok(frame) = frame::decode(&bytes) else {
            return;
        };
        if let Err(refused) = queue.offer(frame) {
            // The lane is full, and this connection carries traffic one way —
            // the sender answers on its own connection back to us and never
            // reads this one, so there is nowhere in band to say so. Inventing
            // a target to reply to would send the refusal somewhere nobody is
            // listening, which loses it and looks like a leak.
            //
            // So it is counted and said out loud, and the sender learns from
            // its deadline. A lane that fills is a sizing fact, and one that
            // fills quietly is the thing worth preventing.
            refusals.record(&refused.0);
        }
    }
}

/// Counts refusals and says so, without saying so on every frame.
#[derive(Default)]
struct Refusals {
    total: usize,
}

impl Refusals {
    fn record(&mut self, frame: &Frame) {
        self.total += 1;
        // The first, then decade by decade. A lane that overflows tends to
        // keep overflowing, and a line per frame would bury the run it is
        // reporting on.
        if self.total == 1 || self.total.is_power_of_two() {
            eprintln!(
                "P4_AGENT_LANE_FULL lane={:?} route={} refused={}",
                frame.envelope.lane, frame.envelope.route, self.total
            );
        }
    }
}

#[cfg(test)]
mod tests;
