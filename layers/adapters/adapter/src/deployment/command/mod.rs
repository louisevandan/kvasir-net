//! What a caller sends to start or stop one submission.

use crate::work::DeploymentId;

/// Identity for one submission's whole lifecycle, minted by the caller.
pub type SubmissionId = String;

/// One independent unit of work against a deployment.
///
/// Independent because nothing above this boundary batches submissions
/// together any more -- batching, if a backend does it at all, happens
/// entirely on the backend's own side of `Client::submit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submit {
    pub deployment_id: DeploymentId,
    /// The generation this submission was issued against. A client must
    /// treat a submission naming a superseded generation as unconditionally
    /// stale rather than run it against whatever is now loaded.
    pub deployment_generation: u64,
    /// Identity for this submission's whole lifecycle. A resend under the
    /// same id is the same submission, not a new one -- a client that has
    /// already accepted, is running, or has already settled this id must
    /// answer from what it remembers rather than starting again.
    pub submission_id: SubmissionId,
    /// Absolute caller deadline in Unix milliseconds. Zero means none.
    /// Backend clients use this to bound their own backpressure retries;
    /// the P4 broker does not interpret capacity or schedule retries.
    pub deadline_unix_ms: u64,
    /// The request as the ingress vocabulary stated it. Opaque to the broker:
    /// only the selected deployment client may translate it into a backend
    /// API or native wire shape, and this crate reads nothing out of it.
    pub request: serde_json::Value,
}

/// Ends a submission early.
///
/// Naming a submission the client has never heard of, or has already
/// settled, is a no-op rather than an error -- see `super::Client::cancel`.
/// Carries only the id: a cancel is not a new instruction about the
/// deployment or its generation, only about a submission already in flight
/// under one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cancel {
    pub submission_id: SubmissionId,
}

#[cfg(test)]
mod tests;
