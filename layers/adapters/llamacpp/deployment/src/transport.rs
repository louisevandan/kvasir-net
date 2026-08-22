//! The seam between this client and the wire.
//!
//! `TransportWriter`/`TransportReader` are split rather than one duplex
//! object because the client holds them on two different threads at once: a
//! background thread blocks in `recv` for the life of the connection while
//! `submit`/`cancel` calls reach `send` from whichever thread called them.
//! `std::net::TcpStream::try_clone` gives exactly this split for the real
//! transport, and `p4-llamacpp-staged-adapter`'s process/control link uses
//! the same shape for the same reason.
//!
//! Production code drives `TcpLineTransport`; tests drive an in-memory fake
//! (`fake` module) that never opens a socket. Nothing above this file's
//! trait objects knows or cares which.

use crate::contract::{Command, Event};
use std::io;

pub trait TransportWriter: Send {
    fn send(&mut self, command: &Command) -> io::Result<()>;
}

pub trait TransportReader: Send {
    /// Blocks until the next event, clean end-of-stream (`Ok(None)`), or an
    /// I/O error. Both `Ok(None)` and `Err` mean the same thing to a caller
    /// of this trait -- the connection is gone -- but are kept distinct
    /// because the two carry different detail for a log line.
    fn recv(&mut self) -> io::Result<Option<Event>>;
}

/// Opens a fresh connection on demand. The client calls this once at
/// construction and again on every reconnect; it never assumes the returned
/// halves are the same physical socket as last time.
pub trait TransportFactory: Send + Sync {
    fn connect(&self) -> io::Result<(Box<dyn TransportWriter>, Box<dyn TransportReader>)>;

    /// The deployment generation the most recent `connect` was told, if this
    /// transport's handshake carries one.
    ///
    /// The generation is the backend's to issue, and a reload happens while
    /// this client is disconnected -- so a reconnect is the only moment it
    /// can be learned. A client that goes on fencing against the value it
    /// started with sends work the backend refuses as `deployment_closed`,
    /// for as long as the process lives, with nothing in the client able to
    /// notice.
    fn reported_generation(&self) -> Option<crate::contract::Generation> {
        None
    }
}

pub mod line_codec;
pub mod tcp;
pub mod upgrade;

#[cfg(test)]
pub mod fake;
