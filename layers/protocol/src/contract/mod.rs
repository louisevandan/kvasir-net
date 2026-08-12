//! Runtime-neutral P4 wire records.

mod error;
mod execution;
mod message;
mod phase;
mod wire;

pub use error::ProtocolError;
pub use execution::{ExecutionDone, ExecutionRequest, ExecutionToken};
pub use message::Message;
pub use phase::Phase;
pub use wire::{PROTOCOL, VERSION};
