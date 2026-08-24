//! Self-describing-event llama.cpp adapter primitives.

mod capsule;
mod commands;
mod logical;
mod node;
mod scheduler;

pub use capsule::{
    CapsuleError, CapsuleSet, Invocation, PhysicalCapsule, PhysicalOutcome, RowOwner, Tensor,
    TensorDescriptor,
};
pub use commands::{
    BatchObservation, BatchRequestObservation, InferenceCommand, LoadCommand, NodeAddress,
    NodeRole, OutcomePayload, PhysicalBatchObservation, ReleaseCommand, ReleaseSequence, ReplySpec,
    SessionCommand,
};
pub use logical::{LogicalBatch, LogicalBatchError, LogicalRow};
pub use node::LlamaNodeAdapter;
pub use scheduler::{Allocation, Demand, Phase, Scheduler, SchedulerError};

pub const LOAD_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.load-v2+json";
pub const UNLOAD_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.unload-v2+json";
pub const SESSION_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.session-v2+json";
pub const PREFILL_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.prefill-v2+json";
pub const DECODE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.decode-v2+json";
pub const PHYSICAL_BATCH_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.physical-batch-v2";
pub const TAIL_BATCH_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.tail-batch-v2";
pub const RELEASE_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.release-v2+json";
pub const RELEASED_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.released-v2+json";
pub const OUTPUT_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.output-v2+json";
pub const BATCH_OBSERVATION_CONTENT_TYPE: &str =
    "application/vnd.p4.llamacpp.batch-observation-v2+json";
pub const ERROR_CONTENT_TYPE: &str = "application/vnd.p4.llamacpp.error-v2+json";

#[cfg(test)]
mod tests;
