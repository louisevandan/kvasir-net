//! The deployment-submission contract: how one independent request enters a
//! backend and how its outcomes leave, replacing `Work`/`Hop`'s batching
//! shape at the boundary a backend actually implements.
//!
//! `Work::Hop` names a *window* the node itself assembled; this module names
//! one *submission*, and nothing above the boundary decides how many run
//! together. That decision, if a backend makes one at all, is entirely
//! behind `Client::submit` -- see docs/deployment-adapter-contract.md for why
//! the batching P4 used to do is being removed rather than reimplemented
//! here, and for what was deliberately left out of this contract.
//!
//! Four modules carry it: `command` is what a caller sends, `event` is what
//! comes back, `wire` is the JSON encoding a Rust client and a TypeScript
//! server must produce and accept identically, and `reader` is the
//! structural oracle that checks a recorded event stream against the
//! contract's invariants without caring what produced it.

pub mod command;
pub mod event;
pub mod reader;
pub mod registry;
pub mod wire;

pub use command::{Cancel, SubmissionId, Submit};
pub use event::{
    Accepted, DeploymentEvent, Produced, Rejected, RejectedReason, Settled, SettledReason,
};
pub use reader::Violation;
pub use registry::{DispatchError, Registry};

pub use crate::work::DeploymentId;

/// Why `Client::try_submit` could not enqueue a submission at all.
///
/// Distinct from every backend-level outcome -- `Accepted`, `Rejected`
/// (`Full` included), `Produced`, `Settled` -- which always arrives later on
/// the `Sink` rather than through this return value. This type exists only
/// for "the client itself could not take this," e.g. it has already been
/// closed; it is never how a caller learns whether the backend admitted the
/// work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnqueueError(pub String);

impl std::fmt::Display for EnqueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for EnqueueError {}

/// Where a client raises what it hears back about a submission.
///
/// Mirrors `EventSink` (`crate::event::EventSink`) in shape and in the same
/// reason it takes no return value: an event has already happened by the
/// time it is raised, so nothing a caller could return would change it.
pub trait Sink: Send + Sync {
    fn raise(&self, event: DeploymentEvent);
}

/// What a caller drives to submit and cancel work against one deployment.
///
/// This is the trait a P4 llama client implements
/// (`apps/p4/layers/adapters/llamacpp/deployment`); the P4 broker holds one
/// `Arc<dyn Client>` and calls it directly -- there is no `Work`/`Hop`
/// bridge in between, and building one is exactly the structure this
/// contract replaces.
///
/// The `Sink` this client reports to is not a parameter of either method
/// here: whatever constructs a `Client` implementation hands it one
/// `Arc<dyn Sink>` up front, at construction, and every event for every
/// submission that implementation ever handles arrives there for its whole
/// lifetime. A per-call sink cannot work here -- a `&dyn Sink` borrowed for
/// one call does not outlive the call, so a client that returns
/// immediately (which both methods below require) would have nothing left
/// to raise `Produced` or `Settled` on by the time either happens.
pub trait Client: Send + Sync {
    /// Enqueues one submission, or reports why it could not be enqueued at
    /// all. This is not an admission verdict: whether the backend accepts,
    /// rejects (`Full` included), or eventually settles this submission is
    /// unknown at the time this returns, and arrives later on the sink. A
    /// resend of a `submission_id` this client has already accepted, is
    /// running, or has already settled must not start a second execution --
    /// see `Submit`'s own doc for why a client cannot tell a resend apart
    /// from a duplicate without remembering ids it has settled -- but it is
    /// still `Ok(())`, not an error.
    fn try_submit(&self, submit: Submit) -> Result<(), EnqueueError>;

    /// Ends a submission early. Idempotent and infallible: a `submission_id`
    /// this client does not recognise, whether never submitted or already
    /// settled, is a no-op rather than an error.
    fn cancel(&self, submission_id: SubmissionId);
}
