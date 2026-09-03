use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NodeAddress {
    pub agent: String,
    pub node: String,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    First,
    Middle,
    Last,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SessionCommand {
    pub load_generation: u64,
    pub session_id: String,
    pub role: NodeRole,
    pub next: Option<NodeAddress>,
    pub first: NodeAddress,
}

impl SessionCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0
            || self.session_id.is_empty()
            || self.first.agent.is_empty()
            || self.first.node.is_empty()
            || self.first.generation == 0
        {
            return Err("session identity and first node are required");
        }
        match self.role {
            NodeRole::First | NodeRole::Middle
                if self.next.as_ref().is_none_or(|next| {
                    next.agent.is_empty() || next.node.is_empty() || next.generation == 0
                }) =>
            {
                Err("non-tail session requires a next node")
            }
            NodeRole::Last if self.next.is_some() => Err("tail session cannot carry a next node"),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LoadCommand {
    pub load_generation: u64,
    pub binary: String,
    pub endpoint: String,
    pub plan: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: Vec<(String, String)>,
    pub n_batch: usize,
    pub n_ubatch: usize,
    pub context_size: usize,
    pub total_context_size: usize,
    pub sequence_capacity: u32,
    #[serde(default = "default_ready_timeout")]
    pub ready_timeout_ms: u64,
    #[serde(default = "default_io_timeout")]
    pub io_timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct UnloadCommand {
    pub load_generation: u64,
}

impl UnloadCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0 {
            return Err("unload generation must be non-zero");
        }
        Ok(())
    }
}

fn default_ready_timeout() -> u64 {
    600_000
}
fn default_io_timeout() -> u64 {
    600_000
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct InferenceCommand {
    pub load_generation: u64,
    pub session_id: String,
    pub request_id: String,
    #[serde(default)]
    pub tokens: Vec<i32>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub options: String,
    /// The durable conversation this request belongs to.
    ///
    /// `session_id` names a pipeline session and `request_id` names one turn;
    /// neither identifies "the same conversation" across a restart, which is
    /// what a persisted KV record has to be keyed by. Optional on the wire so
    /// an OUTER that never persists need not mint one, but when present it
    /// must parse as `sk1:<owner>/<conversation>` - see [`SessionKey`].
    ///
    /// [`SessionKey`]: crate::v2::SessionKey
    #[serde(default)]
    pub session_key: Option<String>,
    pub max_tokens: u32,
}

impl InferenceCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0
            || self.session_id.is_empty()
            || self.request_id.is_empty()
            || self.max_tokens == 0
        {
            return Err("inference identity and max_tokens are required");
        }
        if self.tokens.is_empty() == self.prompt.as_ref().is_none_or(String::is_empty) {
            return Err("exactly one non-empty prompt or token vector is required");
        }
        // A malformed key is refused here rather than carried: it would reach
        // the store as a path component, and two conversations that collide
        // there cannot be told apart afterwards by comparing bytes.
        if let Some(raw) = &self.session_key {
            crate::v2::SessionKey::parse(raw).map_err(crate::v2::SessionKeyError::as_str)?;
        }
        Ok(())
    }

    /// The validated conversation identity, if this request carries one.
    pub fn parsed_session_key(&self) -> Option<crate::v2::SessionKey> {
        self.session_key
            .as_deref()
            .and_then(|raw| crate::v2::SessionKey::parse(raw).ok())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct OutcomePayload {
    pub load_generation: u64,
    pub session_id: String,
    pub request_id: String,
    pub sequence_id: u32,
    pub token: i32,
    pub text: String,
    pub position: u32,
    pub stop: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PhysicalBatchObservation {
    pub execution_id: u64,
    pub rows: usize,
    pub prefill_rows: usize,
    pub decode_rows: usize,
    pub verify_rows: usize,
    pub replay_rows: usize,
    pub request_count: usize,
    pub sequence_count: usize,
    pub requests: Vec<BatchRequestObservation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BatchRequestObservation {
    pub request_id: String,
    pub prefill_rows: usize,
    pub decode_rows: usize,
    pub verify_rows: usize,
    pub replay_rows: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct BatchObservation {
    pub observation_id: String,
    pub load_generation: u64,
    pub session_id: String,
    pub logical_rows: usize,
    pub physical_batches: Vec<PhysicalBatchObservation>,
    pub mixed_physical_batches: usize,
    /// How long this node's own stage server held the batch: the
    /// LogicalBatch -> PhysicalResult round trip, which is this node's layers
    /// and nothing downstream.
    #[serde(default)]
    pub stage_ms: u64,
    /// How long this node had nothing to submit before planning this batch,
    /// measured from the moment the previous batch's stage call returned. A
    /// first node that is keeping up shows a small number here; a large one
    /// says the node was waiting, and `idle_gated` says whether it was
    /// waiting on work or on the coalescing threshold.
    #[serde(default)]
    pub idle_ms: u64,
    /// How many times the coalescing threshold turned the drive loop away
    /// during that idle gap. Zero means the node had no work to plan.
    #[serde(default)]
    pub idle_gated: u64,
    /// Token rows the ready set held at the instant this batch was planned -
    /// every remaining prompt token and every ready decode token, not a
    /// request count. Compare with the batch's own rows: a positive gap is
    /// the scheduler, not the arrival pattern, deciding the width.
    #[serde(default)]
    pub ready_rows: usize,
    /// Requests that were eligible at that instant, which is the quantity
    /// the coalescing gate compares its threshold against.
    #[serde(default)]
    pub ready_sequences: usize,
}

/// What one node did with one batch, on a wall clock.
///
/// Batch widths cannot say whether the pipeline overlaps: a first node that
/// submits rarely could be starved, gated, or simply slow, and nothing the
/// first node reports can show a middle node working at the same time. Every
/// node emits one of these per batch, keyed by the execution ids the batch
/// carries - the same ids on every stage - so the drive can lay the stages
/// side by side and count how many executions are open at once. Wall clock
/// rather than a monotonic one because the stages are separate processes;
/// the comparison is only as good as their clocks agree, which on one host
/// is well under a millisecond.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct StageSpan {
    pub load_generation: u64,
    pub session_id: String,
    /// The execution ids of the physical batches this span covers.
    pub execution_ids: Vec<u64>,
    pub rows: usize,
    /// The batch reached this node (a first node: the plan was started).
    pub ingress_unix_ms: u64,
    /// The node handed the batch to its stage server.
    pub start_unix_ms: u64,
    /// The stage server returned.
    pub end_unix_ms: u64,
    /// The result left this node for the next one, or for the outer.
    pub forward_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReleaseCommand {
    pub load_generation: u64,
    pub session_id: String,
    pub sequences: Vec<ReleaseSequence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReleaseSequence {
    pub key: String,
    pub id: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReleasedPayload {
    pub load_generation: u64,
    pub session_id: String,
    pub released: usize,
}

impl ReleasedPayload {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0 || self.session_id.is_empty() || self.released == 0 {
            return Err("release completion requires load, session and count");
        }
        Ok(())
    }
}

impl ReleaseCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0
            || self.session_id.is_empty()
            || self.sequences.is_empty()
            || self.sequences.iter().any(|value| value.key.is_empty())
        {
            return Err("release requires a session and sequence identities");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SettlementCommand {
    pub load_generation: u64,
    pub session_id: String,
    pub sequences: Vec<SettlementSequence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct SettlementSequence {
    pub key: String,
    pub id: u32,
    pub retain_from: u32,
    #[serde(default)]
    pub replay_tokens: Vec<i32>,
    pub replay_position: u32,
    /// Provider-neutral continuation produced only by the terminal concrete
    /// adapter after every pipeline stage has applied this settlement.
    #[serde(default)]
    pub proposal: Vec<i32>,
}

impl SettlementCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0
            || self.session_id.is_empty()
            || self.sequences.is_empty()
            || self.sequences.iter().any(|value| {
                value.key.is_empty()
                    || (!value.replay_tokens.is_empty() && !value.proposal.is_empty())
                    || if value.replay_tokens.is_empty() {
                        value.replay_position != 0
                    } else {
                        u32::try_from(value.replay_tokens.len())
                            .ok()
                            .and_then(|count| value.replay_position.checked_add(count))
                            != Some(value.retain_from)
                    }
            })
        {
            return Err("settlement requires load, session and sequence identities");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ErrorPayload {
    pub code: String,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReplySpec {
    pub ingress_agent: String,
    pub channel: String,
    pub connection_generation: u64,
    pub correlation_id: String,
    pub deadline_unix_ms: Option<u64>,
}
