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
#[serde(deny_unknown_fields)]
pub struct SessionCommand {
    pub load_generation: u64,
    pub session_id: String,
    /// Ordered logical pipeline, declared by OUTER before any stage work.
    /// Device placement and native model cuts are separate contracts.
    pub stages: Vec<NodeAddress>,
    /// The recipient's index, checked against its complete endpoint at install.
    pub stage_index: usize,
}

impl SessionCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0 || self.session_id.is_empty() || self.session_id.contains('\0')
        {
            return Err("session load and identity are required");
        }
        // The current staged execution path requires distinct head and tail.
        // A one-stage engine path is not created by accepting an empty next.
        if self.stages.len() < 2 || self.stage_index >= self.stages.len() {
            return Err("session requires an ordered pipeline and a valid local index");
        }
        let mut identities = std::collections::HashSet::new();
        for stage in &self.stages {
            let address = stage
                .agent
                .parse::<p4_protocol::Address>()
                .map_err(|_| "session stage address is invalid")?;
            if stage.node.is_empty() || stage.node.contains('\0') || stage.generation == 0 {
                return Err("session stage identity is invalid");
            }
            // Two generations of one node cannot be two simultaneous stages.
            if !identities.insert((address.to_string(), stage.node.as_str())) {
                return Err("session repeats a node identity");
            }
        }
        Ok(())
    }

    /// Only called for a validated, installed command.
    pub fn role(&self) -> NodeRole {
        if self.stage_index == 0 {
            NodeRole::First
        } else if self.stage_index + 1 == self.stages.len() {
            NodeRole::Last
        } else {
            NodeRole::Middle
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
            || self.session_id.contains('\0')
            || self.request_id.contains('\0')
            || self
                .session_id
                .len()
                .checked_add(1)
                .and_then(|n| n.checked_add(self.request_id.len()))
                .is_none_or(|n| n > 4096)
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
#[serde(deny_unknown_fields)]
pub struct PhysicalBatchObservation {
    pub execution_id: u64,
    pub rows: usize,
    pub prefill_rows: usize,
    pub decode_rows: usize,
    pub verify_rows: usize,
    pub replay_rows: usize,
    pub request_count: usize,
    pub sequence_count: usize,
    /// Recipient-owned detail only; other counters remain physical-global.
    pub owned_requests: Vec<BatchRequestObservation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BatchRequestObservation {
    pub request_id: String,
    pub submission_event_id: String,
    pub sequence_id: u32,
    pub incarnation: u64,
    /// Accepted per-request chain index, never a telemetry-derived total.
    pub request_issue_index: u64,
    pub rows: Vec<super::IssuedRow>,
    pub prefill_rows: usize,
    pub decode_rows: usize,
    pub verify_rows: usize,
    pub replay_rows: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BatchObservation {
    /// Selection-time node snapshot. Absent on older producers; not a credit
    /// grant, stage completion, or proof that an idle device could execute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<SchedulingSnapshot>,
    pub observation_id: String,
    pub load_generation: u64,
    pub session_id: String,
    pub logical_ordinal: u64,
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
    /// during that idle gap. Zero does not exclude other issue blockers.
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<super::scheduler::pipeline::PipelineSelection>,
    pub ordinary_limits: super::scheduler::OrdinaryLimits,
    pub ordinary_limits_applied: bool,
    pub min_batch_rows: usize,
    pub max_issue_rows: usize,
    pub max_open_batches: usize,
    pub prefill_fragments: u32,
    pub open_batches_before_issue: usize,
    pub pending_admission: usize,
    pub blocked_outstanding: usize,
    pub no_ready_input: usize,
    pub eligible_prefill: usize,
    pub eligible_decode: usize,
    pub eligible_atomic: usize,
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
#[serde(deny_unknown_fields)]
pub struct StageSpan {
    pub load_generation: u64,
    pub session_id: String,
    /// The execution ids of the physical batches this span covers.
    pub execution_ids: Vec<u64>,
    pub executions: Vec<StageExecutionObservation>,
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
#[serde(deny_unknown_fields)]
pub struct StageExecutionObservation {
    pub execution_id: u64,
    pub owned_requests: Vec<StageRequestObservation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StageRequestObservation {
    pub request_id: String,
    pub sequence_id: u32,
    pub incarnation: u64,
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
    pub incarnation: u64,
    pub operation_id: u64,
}

impl ReleaseCommand {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.load_generation == 0
            || self.session_id.is_empty()
            || self.sequences.is_empty()
            || self.session_id.contains('\0')
            || self.sequences.iter().any(|value| {
                value.key.is_empty()
                    || value.incarnation == 0
                    || value.operation_id == 0
                    || !value.key.starts_with(&format!("{}\0", self.session_id))
            })
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
    pub incarnation: u64,
    pub operation_id: u64,
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
            || self.session_id.contains('\0')
            || self.sequences.is_empty()
            || self.sequences.iter().any(|value| {
                value.key.is_empty()
                    || value.incarnation == 0
                    || value.operation_id == 0
                    || !value.key.starts_with(&format!("{}\0", self.session_id))
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

#[cfg(test)]
mod observation_wire_tests {
    use super::*;

    fn observation() -> serde_json::Value {
        serde_json::json!({
            "observation_id":"s:2:11", "load_generation":7, "session_id":"s",
            "logical_ordinal":2, "logical_rows":4, "mixed_physical_batches":0,
            "physical_batches":[{
                "execution_id":11,"rows":4,"prefill_rows":4,"decode_rows":0,
                "verify_rows":0,"replay_rows":0,"request_count":2,"sequence_count":2,
                "owned_requests":[{
                    "request_id":"a","submission_event_id":"sent-a","sequence_id":0,
                    "incarnation":3,"request_issue_index":1,
                    "rows":[{"phase":"prefill","position":0},{"phase":"prefill","position":1}],
                    "prefill_rows":2,"decode_rows":0,"verify_rows":0,"replay_rows":0
                }]
            }]
        })
    }

    #[test]
    fn observation_wire_separates_owned_detail_from_physical_global_counts() {
        let wire = observation();
        let value: BatchObservation = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(value.logical_rows, 4);
        assert_eq!(value.physical_batches[0].rows, 4);
        assert_eq!(value.physical_batches[0].owned_requests[0].rows.len(), 2);
        for path in [
            vec!["unexpected"],
            vec!["physical_batches", "0", "unexpected"],
            vec!["physical_batches", "0", "owned_requests", "0", "unexpected"],
        ] {
            let mut changed = wire.clone();
            let mut cursor = &mut changed;
            for component in &path[..path.len() - 1] {
                cursor = match component.parse::<usize>() {
                    Ok(index) => &mut cursor[index],
                    Err(_) => &mut cursor[*component],
                };
            }
            cursor[*path.last().unwrap()] = true.into();
            assert!(serde_json::from_value::<BatchObservation>(changed).is_err());
        }
        let mut old = wire;
        old.as_object_mut().unwrap().remove("logical_ordinal");
        assert!(serde_json::from_value::<BatchObservation>(old).is_err());
    }

    #[test]
    fn legacy_request_totals_cannot_replace_exact_owner_rows_or_issue_identity() {
        for field in [
            "submission_event_id",
            "sequence_id",
            "incarnation",
            "request_issue_index",
            "rows",
        ] {
            let mut changed = observation();
            changed["physical_batches"][0]["owned_requests"][0]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(
                serde_json::from_value::<BatchObservation>(changed).is_err(),
                "missing {field}"
            );
        }
        let mut old = observation();
        let detail = old["physical_batches"][0]
            .as_object_mut()
            .unwrap()
            .remove("owned_requests")
            .unwrap();
        old["physical_batches"][0]["requests"] = detail;
        assert!(serde_json::from_value::<BatchObservation>(old).is_err());
    }

    #[test]
    fn stage_wire_requires_execution_owners_and_forbids_invented_submission_field() {
        let wire = serde_json::json!({
            "load_generation":7,"session_id":"s","execution_ids":[11],"rows":4,
            "executions":[{"execution_id":11,"owned_requests":[{"request_id":"a","sequence_id":0,"incarnation":3}]}],
            "ingress_unix_ms":10,"start_unix_ms":11,"end_unix_ms":12,"forward_unix_ms":13
        });
        let value: StageSpan = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
        let mut old = wire.clone();
        old.as_object_mut().unwrap().remove("executions");
        assert!(serde_json::from_value::<StageSpan>(old).is_err());
        let mut invented = wire;
        invented["executions"][0]["owned_requests"][0]["submission_event_id"] =
            "not-known-downstream".into();
        assert!(serde_json::from_value::<StageSpan>(invented).is_err());
    }
}
