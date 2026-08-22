//! A thin client for llama.cpp's v2 submission stream (`SEALED-CONTRACT.md`),
//! and the glue that registers it as a `p4_adapter::Adapter`.
//!
//! "Thin" is load-bearing: this crate does not start native processes, does
//! not walk a rank list, does not compute batches, does not interpret hidden
//! state. From P4's side, the whole distributed deployment behind it is one
//! logical backend endpoint (`Distribution::Internal`). Everything that
//! decides how many UBATCH slots exist or which rank a sequence lands on
//! stays in `apps/llama/native/linker-node`'s existing scheduler, reused
//! unchanged behind this client.
//!
//! Module map, in dependency order:
//! - [`contract`]: re-exports of the canonical `p4_adapter::deployment` wire
//!   vocabulary (`Submit`/`Cancel` -> `Accepted`/`Rejected`/`Produced`/
//!   `Settled`), plus the one local wire-envelope convenience that module
//!   does not define -- see that module's doc comment.
//! - [`transport`]: the connection seam (`TransportWriter`/`TransportReader`/
//!   `TransportFactory`), a real TCP implementation that performs the HTTP
//!   Upgrade the llama-path server requires, and a fake used only by tests.
//! - [`ledger`]: per-submission dedup, generation fencing, ordinal
//!   continuity, and reconnect replay bookkeeping.
//! - [`client`]: [`client::DeploymentClient`], which holds one connection
//!   across every submission, drives the ledger and transport together, and
//!   implements `p4_adapter::deployment::Client` so the P4 broker can call it
//!   directly -- there is no `Work`/`Hop` bridge here, deliberately: that
//!   bridge is the structure this contract replaces, not a shape for this
//!   crate to reproduce.
//!
//! This checkpoint's proof surface is `client`'s and `ledger`'s own tests
//! against `transport::fake` (no GPU, no `apps/llama` process, no network
//! socket) plus `tests/cross_wire.rs`, which stands up the real llama v2
//! submission-stream server in a child process and drives it with this real
//! client over a real TCP socket.

pub mod client;
pub mod contract;
pub mod ledger;
pub mod settled_after_lease_release;
pub mod transport;

#[cfg(test)]
mod test_support;

pub use client::DeploymentClient;
pub use contract::EnqueueError;
