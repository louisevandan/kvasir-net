//! Self-describing-event llama.cpp adapter primitives.

mod build_identity;
mod capsule;
mod commands;
mod completion;
mod control_identity;
pub(crate) mod issue_witness;
mod logical;
mod node;
pub mod record;
mod scheduler;
mod session_key;
#[cfg(test)]
mod session_key_wire_tests;

pub use build_identity::{BuildDisagreement, BuildIdentity, UNIDENTIFIED, agree};
pub use capsule::{
    CapsuleError, CapsuleSet, GeneratedToken, Invocation, PhysicalCapsule, PhysicalOutcome,
    RowOwner, Tensor, TensorDescriptor,
};
pub use commands::{
    BatchObservation, BatchRequestObservation, InferenceCommand, LoadCommand, NodeAddress,
    NodeRole, OutcomePayload, PhysicalBatchObservation, ReleaseCommand, ReleaseSequence, ReplySpec,
    SchedulingSnapshot, SessionCommand, SettlementCommand, SettlementSequence,
    StageExecutionObservation, StageRequestObservation, StageSpan, UnloadCommand,
};
pub use completion::{ApprovedOutputPayload, ReleaseMember, ReleaseReceipt};
pub use issue_witness::{
    IssueAuthority, IssueWitness, IssuedExecution, IssuedRow, IssuedWork, IssuedWorkProof,
};
pub use logical::{LogicalBatch, LogicalBatchError, LogicalRow};
pub use node::LlamaNodeAdapter;
#[cfg(test)]
pub(crate) use scheduler::PREFILL_PATIENCE;
pub use scheduler::{Allocation, Demand, OrdinaryLimits, Phase, Scheduler, SchedulerError};
pub use session_key::{SessionKey, SessionKeyError};

pub const LOAD_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.load-v3+json";
pub const LOADED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.loaded-v3+json";
pub const UNLOAD_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.unload-v3+json";
pub const UNLOADED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.unloaded-v3+json";
pub const SESSION_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.session-v4+json";
pub const SESSION_READY_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.session-ready-v4+json";
pub const PREFILL_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.prefill-v3+json";
pub const DECODE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.decode-v3+json";
pub const PHYSICAL_BATCH_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.physical-batch-v4";
pub const TAIL_BATCH_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.tail-batch-v4";
pub const RELEASE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.release-v4+json";
pub const RELEASED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.released-v4+json";
pub const SETTLE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.settle-v4+json";
pub const SETTLED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.settled-v4+json";
pub const OUTPUT_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.output-v5+json";
pub const RELEASE_RECEIPT_CONTENT_TYPE: &str =
    "application/vnd.p4.llamacpp.release-receipt-v1+json";
pub const BATCH_OBSERVATION_CONTENT_TYPE: &str =
    "application/vnd.p4.llamacpp.batch-observation-v4+json";
pub const STAGE_SPAN_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.stage-span-v4+json";
pub const ERROR_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.error-v2+json";

#[cfg(test)]
mod scheduler_mixed_tests;
#[cfg(test)]
mod simulator;
#[cfg(test)]
mod simulator_tests;
#[cfg(test)]
mod tests;
