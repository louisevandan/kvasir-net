//! What a client raises about a submission it is running.

use super::command::SubmissionId;

/// The submission was accepted. Its whole lifecycle from here belongs to the
/// client -- see `super::Client::submit`'s own doc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub submission_id: SubmissionId,
}

/// The submission was refused before it ran.
///
/// `Accepted` and `Rejected` are mutually exclusive for a given submission --
/// see `reader::check`, which is what actually enforces that rather than
/// this type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub submission_id: SubmissionId,
    pub reason: RejectedReason,
}

/// Why a submission was refused, as a closed set rather than a message a
/// caller would have to parse.
///
/// `Full` is the one variant that names ordinary backpressure rather than a
/// fault. A consumer distinguishes it from the rest with a match arm against
/// this enum, never a substring search over free text -- see the contract
/// doc's "no string parsing" rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectedReason {
    /// Ordinary backpressure. Retriable; not a defect.
    Full,
    /// This `submission_id` names a submission already running or already
    /// settled under content different from this `Submit`'s.
    Conflict,
    /// The submission itself could not be accepted -- a malformed request,
    /// or a `deployment_generation` this client has never loaded.
    Invalid,
    /// The named deployment is not open for new submissions, generally
    /// because it has been unloaded or superseded.
    DeploymentClosed,
}

/// One unit of output.
///
/// Zero or more per submission, `event_ordinal` contiguous from zero -- see
/// `reader::check`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Produced {
    pub submission_id: SubmissionId,
    pub event_ordinal: u64,
    pub text: String,
    pub generated_tokens: u32,
}

/// The submission finished.
///
/// Exactly one per submission, and nothing may follow it -- see
/// `reader::check`. The contract's own rule, not this type's to enforce:
/// whatever raises `Settled` must have already released any lease it held
/// for this submission before doing so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    pub submission_id: SubmissionId,
    pub reason: SettledReason,
    pub generated_tokens: u32,
}

/// Why a submission settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettledReason {
    Stop,
    Length,
    Canceled,
    Error,
}

/// One event about one submission, in the shape a client raises it and a
/// reader replays it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeploymentEvent {
    Accepted(Accepted),
    Rejected(Rejected),
    Produced(Produced),
    Settled(Settled),
}

impl DeploymentEvent {
    pub fn submission_id(&self) -> &SubmissionId {
        match self {
            Self::Accepted(event) => &event.submission_id,
            Self::Rejected(event) => &event.submission_id,
            Self::Produced(event) => &event.submission_id,
            Self::Settled(event) => &event.submission_id,
        }
    }
}

#[cfg(test)]
mod tests;
