//! Inference request and streamed response records.

use crate::Phase;

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionRequest {
    pub controller_id: String,
    pub node_id: String,
    pub deployment_id: String,
    pub binding_id: String,
    pub runtime_generation: u64,
    pub request_id: String,
    pub session_id: String,
    pub phase: Phase,
    pub position: u32,
    pub max_tokens: u32,
    pub temperature: f32,
    pub prompt: String,
    /// Opaque JSON object. Concrete adapters select only fields they support.
    pub options: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionToken {
    pub controller_id: String,
    pub node_id: String,
    pub request_id: String,
    pub session_id: String,
    pub phase: Phase,
    pub position: u32,
    pub index: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionDone {
    pub controller_id: String,
    pub node_id: String,
    pub request_id: String,
    pub session_id: String,
    pub reason: String,
    pub generated_tokens: u32,
}
