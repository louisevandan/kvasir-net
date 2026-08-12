//! Shared P4 handler, response sink, and transport boundary.
//! See `apps/p4/docs/internals.md#transport-neutral-dispatch`.

use p4_protocol::{Message, read_message, write_message};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub type SharedHandler = Arc<dyn P4Handler>;
pub type SharedTransport = Arc<dyn P4Transport>;

/// Receives one logical P4 response stream without knowing its transport.
pub trait ResponseSink {
    fn emit(&mut self, message: Message) -> Result<()>;
}

/// Processes one P4 command. It is intentionally independent of sockets.
pub trait P4Handler: Send + Sync {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()>;
}

/// Delivers one P4 command to a handler either directly or through a wire codec.
pub trait P4Transport: Send + Sync {
    fn dispatch(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()>;
}

pub struct InMemoryTransport {
    handler: SharedHandler,
}

impl InMemoryTransport {
    pub fn new(handler: SharedHandler) -> Self {
        Self { handler }
    }
}

impl P4Transport for InMemoryTransport {
    fn dispatch(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        self.handler.handle(message, responses)
    }
}

pub struct TcpTransport {
    endpoint: String,
}

impl TcpTransport {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }
}

impl P4Transport for TcpTransport {
    fn dispatch(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        let mut target = TcpStream::connect(&self.endpoint)?;
        target.set_nodelay(true)?;
        write_message(&mut target, &message)?;
        loop {
            let response = read_message(&mut target)?;
            let done = terminal(&response);
            responses.emit(response)?;
            if done {
                return Ok(());
            }
        }
    }
}

pub struct ResponseCollector {
    messages: Vec<Message>,
}

impl ResponseCollector {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn terminal(&self) -> Option<&Message> {
        self.messages.iter().rev().find(|message| terminal(message))
    }

    pub fn into_messages(self) -> Vec<Message> {
        self.messages
    }
}

impl ResponseSink for ResponseCollector {
    fn emit(&mut self, message: Message) -> Result<()> {
        self.messages.push(message);
        Ok(())
    }
}

pub fn in_memory(handler: SharedHandler) -> SharedTransport {
    Arc::new(InMemoryTransport::new(handler))
}

pub fn tcp(endpoint: impl Into<String>) -> SharedTransport {
    Arc::new(TcpTransport::new(endpoint))
}

/// Owns only TCP framing. Business handling always runs through `P4Handler`.
pub fn serve(listen: String, name: &'static str, handler: SharedHandler) -> Result<()> {
    let listener = TcpListener::bind(&listen)?;
    println!("{name}_READY listen={}", listener.local_addr()?);
    for incoming in listener.incoming() {
        let handler = Arc::clone(&handler);
        match incoming {
            Ok(mut client) => {
                thread::spawn(move || {
                    let result = dispatch_tcp(&mut client, handler.as_ref());
                    if let Err(error) = result {
                        if !error
                            .downcast_ref::<p4_protocol::ProtocolError>()
                            .is_some_and(p4_protocol::ProtocolError::is_peer_closed)
                        {
                            eprintln!("{name}_ERROR {error}");
                        }
                    }
                });
            }
            Err(error) => eprintln!("{name}_ACCEPT_ERROR {error}"),
        }
    }
    Ok(())
}

pub fn dispatch_tcp(stream: &mut TcpStream, handler: &dyn P4Handler) -> Result<()> {
    stream.set_nodelay(true)?;
    let message = read_message(stream)?;
    dispatch_message(stream, handler, message)
}

pub(crate) fn dispatch_message(
    stream: &mut TcpStream,
    handler: &dyn P4Handler,
    message: Message,
) -> Result<()> {
    handler.handle(message, &mut TcpResponseSink(stream))
}

pub fn reject(responses: &mut dyn ResponseSink, error: Message) -> Result<()> {
    responses.emit(error)
}

pub fn terminal(message: &Message) -> bool {
    matches!(
        message,
        Message::Done(_)
            | Message::Error { .. }
            | Message::Health { .. }
            | Message::HardwareReport { .. }
            | Message::AdapterRegistered { .. }
            | Message::NodeCreated { .. }
            | Message::ModelBound { .. }
            | Message::ModelUnbound { .. }
    )
}

struct TcpResponseSink<'a>(&'a mut TcpStream);

impl ResponseSink for TcpResponseSink<'_> {
    fn emit(&mut self, message: Message) -> Result<()> {
        write_message(self.0, &message).map_err(Into::into)
    }
}

#[cfg(test)]
mod tests;
