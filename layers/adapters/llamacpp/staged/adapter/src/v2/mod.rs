//! Self-describing-event llama.cpp adapter primitives.

mod build_identity;
mod capsule;
mod commands;
mod logical;
mod node;
pub mod record;
mod scheduler;
mod session_key;
#[cfg(test)]
mod session_key_wire_tests;

pub use capsule::{
    CapsuleError, CapsuleSet, GeneratedToken, Invocation, PhysicalCapsule, PhysicalOutcome,
    RowOwner, Tensor, TensorDescriptor,
};
pub use commands::{
    BatchObservation, BatchRequestObservation, InferenceCommand, LoadCommand, NodeAddress,
    NodeRole, OutcomePayload, PhysicalBatchObservation, ReleaseCommand, ReleaseSequence,
    ReleasedPayload, ReplySpec, SessionCommand, SettlementCommand, SettlementSequence, StageSpan,
    UnloadCommand,
};
pub use logical::{LogicalBatch, LogicalBatchError, LogicalRow};
pub use build_identity::{BuildDisagreement, BuildIdentity, UNIDENTIFIED, agree};
pub use session_key::{SessionKey, SessionKeyError};
pub use node::LlamaNodeAdapter;
pub use scheduler::{Allocation, Demand, Phase, Scheduler, SchedulerError};

pub const LOAD_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.load-v3+json";
pub const LOADED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.loaded-v3+json";
pub const UNLOAD_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.unload-v3+json";
pub const UNLOADED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.unloaded-v3+json";
pub const SESSION_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.session-v3+json";
pub const SESSION_READY_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.session-ready-v3+json";
pub const PREFILL_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.prefill-v3+json";
pub const DECODE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.decode-v3+json";
pub const PHYSICAL_BATCH_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.physical-batch-v3";
pub const TAIL_BATCH_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.tail-batch-v3";
pub const RELEASE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.release-v3+json";
pub const RELEASED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.released-v3+json";
pub const SETTLE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.settle-v3+json";
pub const SETTLED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.settled-v3+json";
pub const OUTPUT_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.output-v3+json";
pub const BATCH_OBSERVATION_CONTENT_TYPE: &str =
    "application/vnd.p4.llamacpp.batch-observation-v3+json";
pub const STAGE_SPAN_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.stage-span-v3+json";
pub const ERROR_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.error-v2+json";

#[cfg(test)]
mod scheduler_mixed_tests;
#[cfg(test)]
mod simulator;
#[cfg(test)]
mod simulator_tests;
#[cfg(test)]
mod tests;
