//! P4B1 v6: the wire.
//!
//! Two things travel — an envelope every hop reads, and a body only its
//! destination does. That split is what lets a relay forward by copying bytes,
//! a socket reader do nothing but enqueue, and a new kind of message cost a
//! relay nothing.
//!
//! What a body means is not here. It belongs to whoever sends and receives it;
//! see `layers/service` for the standard one.

pub mod envelope;
pub mod error;
pub mod event;
pub mod frame;
pub mod lane;
pub mod return_channel;

pub use envelope::{Address, Chain, Envelope, Link, NodeId, Recipient, Scheme};
pub use error::ProtocolError;
pub use lane::QueueClass;
