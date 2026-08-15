//! P4B1 v6 protocol surface. See `apps/p4/docs/api.md#wire-contract`.

mod catalog;
mod codec;
mod contract;
mod task;

pub use catalog::{MessageClass, MessageKind, QueueClass};
pub use codec::{
    RoutedMessage, decode_message, decode_routed_message, encode_message, encode_routed_message,
    read_message, read_routed_message, write_message, write_routed_message,
};
pub use contract::{
    Allocation, ExecutionDone, ExecutionRequest, ExecutionToken, Message, PROTOCOL, Phase,
    ProtocolError, VERSION,
};
pub use task::{Participant, ParticipantRole, TaskDirection, TaskEnvelope, TaskError, TaskKind};
