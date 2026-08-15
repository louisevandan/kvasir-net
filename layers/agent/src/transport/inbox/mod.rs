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
use p4_protocol::Address;
use p4_protocol::frame::{self, Frame};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
            // The lane is full. Answering on the socket that is already open
            // is the fastest backpressure there is, and it keeps the reader
            // from being the thing that blocks.
            refuse(&mut stream, refused.0).await;
        }
    }
}

async fn refuse(stream: &mut TcpStream, frame: Frame) {
    let Some(mut envelope) = frame.envelope.to_reply() else {
        return;
    };
    // The refusal goes back the way it came, so it needs no route of its own.
    envelope.target = Address::tcp("0.0.0.0", 1);
    let Ok(bytes) = frame::encode(&envelope, b"agent lane is full") else {
        return;
    };
    let _ = stream.write_all(&bytes).await;
}

#[cfg(test)]
mod tests;
