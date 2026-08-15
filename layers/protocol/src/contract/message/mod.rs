//! Exhaustive P4 message union.

use crate::{ExecutionDone, ExecutionRequest, ExecutionToken};

/// One named slice of what a load actually reserved.
///
/// The category is the adapter's word, not P4's. P4 carries the pair and never
/// reads the name — knowing that a model divides into KV cache and FFN blocks
/// would be knowing the shape of one family of backends, and a backend built
/// differently would have nothing to put there.
#[derive(Clone, Debug, PartialEq)]
pub struct Allocation {
    pub category: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    IngressSubmit {
        controller_id: String,
        ingress_id: String,
        request_id: String,
        session_id: String,
        node_id: String,
        deployment_id: String,
        binding_id: String,
        runtime_generation: u64,
        max_tokens: u32,
        temperature: f32,
        prompt: String,
        options: String,
    },
    IngressAccepted {
        ingress_id: String,
        request_id: String,
        session_id: String,
    },
    InventoryQuery {
        controller_id: String,
        request_id: String,
    },
    HardwareReport {
        agent_id: String,
        report_id: String,
        snapshot: String,
    },
    AdapterRegister {
        adapter_id: String,
        adapter_kind: String,
        endpoint: String,
        descriptor: String,
    },
    AdapterRegistered {
        adapter_id: String,
        detail: String,
    },
    NodeCreate {
        controller_id: String,
        operation_id: String,
        node_id: String,
        adapter_id: String,
        node_spec: String,
    },
    NodeCreated {
        operation_id: String,
        node_id: String,
        adapter_id: String,
        state: String,
        detail: String,
    },
    ModelLoad {
        controller_id: String,
        node_id: String,
        operation_id: String,
        deployment_id: String,
        binding_id: String,
        model: String,
        plan_revision: String,
        /// Bounded JSON. `load_options` carries backend-neutral load policy;
        /// the selected concrete adapter filters supported fields.
        stage_plan: String,
    },
    ModelBound {
        operation_id: String,
        node_id: String,
        deployment_id: String,
        binding_id: String,
        runtime_generation: u64,
        state: String,
        detail: String,
    },
    ModelUnload {
        controller_id: String,
        node_id: String,
        operation_id: String,
        deployment_id: String,
        binding_id: String,
    },
    ModelUnbound {
        operation_id: String,
        node_id: String,
        deployment_id: String,
        binding_id: String,
        detail: String,
    },
    Execute(ExecutionRequest),
    Token(ExecutionToken),
    Done(ExecutionDone),
    Cancel {
        request_id: String,
        reason: String,
    },
    HealthCheck {
        controller_id: String,
        node_id: String,
        request_id: String,
    },
    Health {
        request_id: String,
        node_id: String,
        ready: bool,
        detail: String,
    },
    LoadProgress {
        operation_id: String,
        node_id: String,
        percent: u32,
        detail: String,
    },
    DraftReport {
        operation_id: String,
        node_id: String,
        /// Sum of `allocations`, carried so a reader that ignores the
        /// adapter's vocabulary still has one comparable figure.
        total_bytes: u64,
        allocations: Vec<Allocation>,
        detail: String,
    },
    Error {
        request_id: String,
        detail: String,
    },
}
