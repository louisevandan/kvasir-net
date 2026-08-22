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
use crate::contract::{Command, Event};
use p4_adapter::deployment::wire;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};

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
}

impl TcpTransportFactory {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

impl TransportFactory for TcpTransportFactory {
    fn connect(&self) -> io::Result<(Box<dyn TransportWriter>, Box<dyn TransportReader>)> {
        let stream = TcpStream::connect(self.addr)?;
        stream.set_nodelay(true)?;
        let read_half = stream.try_clone()?;
        let mut writer = stream;
        let mut reader = BufReader::new(read_half);
        upgrade::perform(
            &mut writer,
            &mut reader,
            &self.addr.to_string(),
            wire::PROTOCOL,
        )?;
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
