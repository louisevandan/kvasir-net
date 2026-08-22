//! The real transport: one TCP connection to `apps/llama`'s submission
//! stream, split into independent read and write halves, opened with the
//! HTTP Upgrade handshake the server requires before it will read or write a
//! single JSON line (`super::upgrade`).
//!
//! This is deliberately the least interesting file in the crate otherwise.
//! Everything that decides what "connected" means for a submission --
//! ledger state, generation fencing, reconnect/replay -- lives in `client.rs`
//! and does not know this type exists; it only knows the
//! `TransportWriter`/`TransportReader`/`TransportFactory` traits.

use super::line_codec::{read_event, write_command};
use super::upgrade;
use super::{TransportFactory, TransportReader, TransportWriter};
use crate::contract::Generation;
use crate::contract::{Command, Event};
use p4_adapter::deployment::wire;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::sync::Mutex;

pub struct TcpWriter(TcpStream);

impl TransportWriter for TcpWriter {
    fn send(&mut self, command: &Command) -> io::Result<()> {
        write_command(&mut self.0, command)
    }
}

pub struct TcpReader(BufReader<TcpStream>);

impl TransportReader for TcpReader {
    fn recv(&mut self) -> io::Result<Option<Event>> {
        read_event(&mut self.0)
    }
}

/// Connects to a fixed address on every call, performing the HTTP Upgrade
/// before handing back the two halves. `apps/llama`'s process supervision
/// owns restarting the server itself; this factory's only job is to open a
/// fresh, upgraded socket each time the client asks.
pub struct TcpTransportFactory {
    addr: SocketAddr,
    deployment_id: String,
    /// What the server said this deployment's generation was on the most
    /// recent handshake. Re-read on every reconnect, so a deployment
    /// reloaded while this client was away is noticed rather than fenced
    /// against forever.
    reported_generation: Mutex<Option<Generation>>,
}

impl TcpTransportFactory {
    pub fn new(addr: SocketAddr) -> Self {
        Self::for_deployment(addr, String::new())
    }

    /// Names the deployment in the handshake, which is what lets the server
    /// answer with that deployment's generation rather than the client
    /// having to invent one.
    pub fn for_deployment(addr: SocketAddr, deployment_id: String) -> Self {
        Self {
            addr,
            deployment_id,
            reported_generation: Mutex::new(None),
        }
    }

    /// The generation the server reported, or `None` if it reported none.
    pub fn reported_generation(&self) -> Option<Generation> {
        *self
            .reported_generation
            .lock()
            .expect("reported generation lock")
    }
}

impl TransportFactory for TcpTransportFactory {
    fn reported_generation(&self) -> Option<Generation> {
        TcpTransportFactory::reported_generation(self)
    }

    fn connect(&self) -> io::Result<(Box<dyn TransportWriter>, Box<dyn TransportReader>)> {
        let stream = TcpStream::connect(self.addr)?;
        stream.set_nodelay(true)?;
        let read_half = stream.try_clone()?;
        let mut writer = stream;
        let mut reader = BufReader::new(read_half);
        let generation = upgrade::perform(
            &mut writer,
            &mut reader,
            &self.addr.to_string(),
            wire::PROTOCOL,
            &self.deployment_id,
        )?;
        *self
            .reported_generation
            .lock()
            .expect("reported generation lock") = generation;
        Ok((Box::new(TcpWriter(writer)), Box::new(TcpReader(reader))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_to_a_closed_port_fails_rather_than_hangs() {
        // Port 0 never accepts a connection; this only proves the factory
        // surfaces a real `io::Error` instead of panicking or blocking
        // forever, which is what the client's reconnect loop depends on.
        let factory = TcpTransportFactory::new("127.0.0.1:0".parse().unwrap());
        assert!(factory.connect().is_err());
    }
}
