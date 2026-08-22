//! The sealed Submit/Cancel -> Accepted/Rejected/Produced/Settled contract.
//!
//! This module used to be a local shim: the canonical version of these types
//! now lives at `apps/p4/layers/adapters/adapter/src/deployment/**` (the
//! `p4-adapter` crate's `deployment` module), and this file is nothing more
//! than a thin re-export of it plus the one thing that module deliberately
//! does not define -- a combined wire envelope for "what this client sends,"
//! used only by `transport`/`line_codec` to have one type to hand a
//! `TransportWriter`.
//!
//! `Request` is `serde_json::Value`, not a string: the wire contract's
//! `request` field is a JSON object (`packages/llama_domain/src/common/
//! protocol/pipeline-submission/types.ts`'s `request: unknown`, `p4-adapter`'s
//! own `Submit.request: serde_json::Value`), and the llama-path's request
//! parser (`parseRingChatRequest`) rejects anything that is not one -- a
//! provisional `String` alias here was the whole reason P4 and the llama
//! path could not previously talk to each other on the wire.

pub use p4_adapter::DeploymentId;
pub use p4_adapter::deployment::{
    Accepted, Cancel, DeploymentEvent as Event, EnqueueError, Produced, Rejected,
    RejectedReason as RejectReason, Settled, SettledReason as SettleReason, SubmissionId, Submit,
};

pub type Generation = u64;
pub type Request = serde_json::Value;

/// What this client sends. Named `Command` rather than reusing `Submit`
/// standalone because `Cancel` has to travel the same connection. Purely a
/// wire-envelope convenience local to this crate's transport layer -- the
/// canonical `p4_adapter::deployment::Client` trait takes `Submit` and a bare
/// `SubmissionId` as two separate methods, never this enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Submit(Submit),
    Cancel(Cancel),
}
