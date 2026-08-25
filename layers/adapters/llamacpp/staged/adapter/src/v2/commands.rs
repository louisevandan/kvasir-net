use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NodeAddress {
    pub agent: String,
    pub node: String,
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
        {
            return Err("session identity and first node are required");
        }
        match self.role {
            NodeRole::First | NodeRole::Middle
                if self
                    .next
                    .as_ref()
                    .is_none_or(|next| next.agent.is_empty() || next.node.is_empty()) =>
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
        Ok(())
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
}

impl SettlementCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0
            || self.session_id.is_empty()
            || self.sequences.is_empty()
            || self.sequences.iter().any(|value| {
                value.key.is_empty()
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
