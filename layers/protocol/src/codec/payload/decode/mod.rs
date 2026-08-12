//! P4 message payload decoding.

use super::super::fields::Cursor;
use super::super::kind::*;
use crate::{ExecutionDone, ExecutionRequest, ExecutionToken, Message, Phase, ProtocolError};

pub(crate) fn decode_payload(kind: u8, payload: &[u8]) -> Result<Message, ProtocolError> {
    let mut c = Cursor {
        bytes: payload,
        offset: 0,
    };
    let message = match kind {
        INGRESS_SUBMIT => Message::IngressSubmit {
            controller_id: c.text()?,
            ingress_id: c.text()?,
            request_id: c.text()?,
            session_id: c.text()?,
            node_id: c.text()?,
            deployment_id: c.text()?,
            binding_id: c.text()?,
            runtime_generation: c.u64()?,
            max_tokens: c.u32()?,
            temperature: c.f32()?,
            prompt: c.text()?,
            options: c.text()?,
        },
        INGRESS_ACCEPTED => Message::IngressAccepted {
            ingress_id: c.text()?,
            request_id: c.text()?,
            session_id: c.text()?,
        },
        INVENTORY_QUERY => Message::InventoryQuery {
            controller_id: c.text()?,
            request_id: c.text()?,
        },
        HARDWARE_REPORT => Message::HardwareReport {
            agent_id: c.text()?,
            report_id: c.text()?,
            snapshot: c.text()?,
        },
        ADAPTER_REGISTER => Message::AdapterRegister {
            adapter_id: c.text()?,
            adapter_kind: c.text()?,
            endpoint: c.text()?,
            descriptor: c.text()?,
        },
        ADAPTER_REGISTERED => Message::AdapterRegistered {
            adapter_id: c.text()?,
            detail: c.text()?,
        },
        NODE_CREATE => Message::NodeCreate {
            controller_id: c.text()?,
            operation_id: c.text()?,
            node_id: c.text()?,
            adapter_id: c.text()?,
            node_spec: c.text()?,
        },
        NODE_CREATED => Message::NodeCreated {
            operation_id: c.text()?,
            node_id: c.text()?,
            adapter_id: c.text()?,
            state: c.text()?,
            detail: c.text()?,
        },
        MODEL_LOAD => Message::ModelLoad {
            controller_id: c.text()?,
            node_id: c.text()?,
            operation_id: c.text()?,
            deployment_id: c.text()?,
            binding_id: c.text()?,
            model: c.text()?,
            plan_revision: c.text()?,
            stage_plan: c.text()?,
        },
        MODEL_BOUND => Message::ModelBound {
            operation_id: c.text()?,
            node_id: c.text()?,
            deployment_id: c.text()?,
            binding_id: c.text()?,
            runtime_generation: c.u64()?,
            state: c.text()?,
            detail: c.text()?,
        },
        MODEL_UNLOAD => Message::ModelUnload {
            controller_id: c.text()?,
            node_id: c.text()?,
            operation_id: c.text()?,
            deployment_id: c.text()?,
            binding_id: c.text()?,
        },
        MODEL_UNBOUND => Message::ModelUnbound {
            operation_id: c.text()?,
            node_id: c.text()?,
            deployment_id: c.text()?,
            binding_id: c.text()?,
            detail: c.text()?,
        },
        EXECUTE => Message::Execute(ExecutionRequest {
            controller_id: c.text()?,
            node_id: c.text()?,
            deployment_id: c.text()?,
            binding_id: c.text()?,
            request_id: c.text()?,
            session_id: c.text()?,
            runtime_generation: c.u64()?,
            phase: Phase::parse(c.byte()?)?,
            position: c.u32()?,
            max_tokens: c.u32()?,
            temperature: c.f32()?,
            prompt: c.text()?,
            options: c.text()?,
        }),
        TOKEN => Message::Token(ExecutionToken {
            controller_id: c.text()?,
            node_id: c.text()?,
            request_id: c.text()?,
            session_id: c.text()?,
            phase: Phase::parse(c.byte()?)?,
            position: c.u32()?,
            index: c.u32()?,
            text: c.text()?,
        }),
        DONE => Message::Done(ExecutionDone {
            controller_id: c.text()?,
            node_id: c.text()?,
            request_id: c.text()?,
            session_id: c.text()?,
            reason: c.text()?,
            generated_tokens: c.u32()?,
        }),
        CANCEL => Message::Cancel {
            request_id: c.text()?,
            reason: c.text()?,
        },
        HEALTH_CHECK => Message::HealthCheck {
            controller_id: c.text()?,
            node_id: c.text()?,
            request_id: c.text()?,
        },
        HEALTH => Message::Health {
            request_id: c.text()?,
            node_id: c.text()?,
            ready: c.byte()? == 1,
            detail: c.text()?,
        },
        LOAD_PROGRESS => Message::LoadProgress {
            operation_id: c.text()?,
            node_id: c.text()?,
            percent: c.u32()?,
            detail: c.text()?,
        },
        DRAFT_REPORT => Message::DraftReport {
            operation_id: c.text()?,
            node_id: c.text()?,
            model_bytes: c.u64()?,
            kv_bytes: c.u64()?,
            layer_bytes: c.u64()?,
            ffn_bytes: c.u64()?,
            detail: c.text()?,
        },
        ERROR => Message::Error {
            request_id: c.text()?,
            detail: c.text()?,
        },
        _ => return Err(ProtocolError::new("unknown P4 message kind")),
    };
    if c.offset != payload.len() {
        return Err(ProtocolError::new("trailing P4 payload bytes"));
    }
    Ok(message)
}
