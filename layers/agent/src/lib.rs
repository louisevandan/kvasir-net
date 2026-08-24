//! The agent: a socket program with one main queue, workers that empty it, and
//! nodes that hold the long work.
//!
//! Two decisions carry the whole design. A worker asks whether a message's
//! address is ours — if not it forwards the frame whole, and a peer and OUTER
//! are the same case. If it is ours, the agent consumes it or a node does, and
//! a node-bound message is moved to that node's queue so the worker is
//! released rather than made to wait on work that runs at GPU speed.
//!
//! Nothing here returns a response. Handlers are procedures whose only output
//! is a message on a queue, and a requester registers a continuation instead of
//! waiting for one.

pub mod agent;
pub mod continuation;
pub mod event_broker;
pub mod event_node;
pub mod node;
pub mod queue;
pub mod transport;
pub mod worker;
