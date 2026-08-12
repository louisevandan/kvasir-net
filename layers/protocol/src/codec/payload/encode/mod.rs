//! P4 message payload encoding.

use super::super::fields::{put_text, put_u32, texts};
use super::super::kind::*;
use crate::{Message, ProtocolError};

pub(crate) fn encode_payload(message: &Message) -> Result<(u8, Vec<u8>), ProtocolError> {
    let mut value = Vec::with_capacity(256);
    let kind = match message {
        Message::IngressSubmit {
            controller_id,
            ingress_id,
            request_id,
            session_id,
            node_id,
            deployment_id,
            binding_id,
            runtime_generation,
            max_tokens,
            temperature,
            prompt,
            options,
        } => {
            texts(
                &mut value,
                &[
                    controller_id,
                    ingress_id,
                    request_id,
                    session_id,
                    node_id,
                    deployment_id,
                    binding_id,
                ],
            )?;
            value.extend_from_slice(&runtime_generation.to_le_bytes());
            put_u32(&mut value, *max_tokens);
            value.extend_from_slice(&temperature.to_le_bytes());
            put_text(&mut value, prompt)?;
            put_text(&mut value, options)?;
            INGRESS_SUBMIT
        }
        Message::IngressAccepted {
            ingress_id,
            request_id,
            session_id,
        } => {
            texts(&mut value, &[ingress_id, request_id, session_id])?;
            INGRESS_ACCEPTED
        }
        Message::InventoryQuery {
            controller_id,
            request_id,
        } => {
            texts(&mut value, &[controller_id, request_id])?;
            INVENTORY_QUERY
        }
        Message::HardwareReport {
            agent_id,
            report_id,
            snapshot,
        } => {
            texts(&mut value, &[agent_id, report_id, snapshot])?;
            HARDWARE_REPORT
        }
        Message::AdapterRegister {
            adapter_id,
            adapter_kind,
            endpoint,
            descriptor,
        } => {
            texts(
                &mut value,
                &[adapter_id, adapter_kind, endpoint, descriptor],
            )?;
            ADAPTER_REGISTER
        }
        Message::AdapterRegistered { adapter_id, detail } => {
            texts(&mut value, &[adapter_id, detail])?;
            ADAPTER_REGISTERED
        }
        Message::NodeCreate {
            controller_id,
            operation_id,
            node_id,
            adapter_id,
            node_spec,
        } => {
            texts(
                &mut value,
                &[controller_id, operation_id, node_id, adapter_id, node_spec],
            )?;
            NODE_CREATE
        }
        Message::NodeCreated {
            operation_id,
            node_id,
            adapter_id,
            state,
            detail,
        } => {
            texts(
                &mut value,
                &[operation_id, node_id, adapter_id, state, detail],
            )?;
            NODE_CREATED
        }
        Message::ModelLoad {
            controller_id,
            node_id,
            operation_id,
            deployment_id,
            binding_id,
            model,
            plan_revision,
            stage_plan,
        } => {
            texts(
                &mut value,
                &[
                    controller_id,
                    node_id,
                    operation_id,
                    deployment_id,
                    binding_id,
                    model,
                    plan_revision,
                    stage_plan,
                ],
            )?;
            MODEL_LOAD
        }
        Message::ModelBound {
            operation_id,
            node_id,
            deployment_id,
            binding_id,
            runtime_generation,
            state,
            detail,
        } => {
            texts(
                &mut value,
                &[operation_id, node_id, deployment_id, binding_id],
            )?;
            value.extend_from_slice(&runtime_generation.to_le_bytes());
            texts(&mut value, &[state, detail])?;
            MODEL_BOUND
        }
        Message::ModelUnload {
            controller_id,
            node_id,
            operation_id,
            deployment_id,
            binding_id,
        } => {
            texts(
                &mut value,
                &[
                    controller_id,
                    node_id,
                    operation_id,
                    deployment_id,
                    binding_id,
                ],
            )?;
            MODEL_UNLOAD
        }
        Message::ModelUnbound {
            operation_id,
            node_id,
            deployment_id,
            binding_id,
            detail,
        } => {
            texts(
                &mut value,
                &[operation_id, node_id, deployment_id, binding_id, detail],
            )?;
            MODEL_UNBOUND
        }
        Message::Execute(v) => {
            texts(
                &mut value,
                &[
                    &v.controller_id,
                    &v.node_id,
                    &v.deployment_id,
                    &v.binding_id,
                    &v.request_id,
                    &v.session_id,
                ],
            )?;
            value.extend_from_slice(&v.runtime_generation.to_le_bytes());
            value.push(v.phase.code());
            put_u32(&mut value, v.position);
            put_u32(&mut value, v.max_tokens);
            value.extend_from_slice(&v.temperature.to_le_bytes());
            put_text(&mut value, &v.prompt)?;
            put_text(&mut value, &v.options)?;
            EXECUTE
        }
        Message::Token(v) => {
            texts(
                &mut value,
                &[&v.controller_id, &v.node_id, &v.request_id, &v.session_id],
            )?;
            value.push(v.phase.code());
            put_u32(&mut value, v.position);
            put_u32(&mut value, v.index);
            put_text(&mut value, &v.text)?;
            TOKEN
        }
        Message::Done(v) => {
            texts(
                &mut value,
                &[
                    &v.controller_id,
                    &v.node_id,
                    &v.request_id,
                    &v.session_id,
                    &v.reason,
                ],
            )?;
            put_u32(&mut value, v.generated_tokens);
            DONE
        }
        Message::Cancel { request_id, reason } => {
            texts(&mut value, &[request_id, reason])?;
            CANCEL
        }
        Message::HealthCheck {
            controller_id,
            node_id,
            request_id,
        } => {
            texts(&mut value, &[controller_id, node_id, request_id])?;
            HEALTH_CHECK
        }
        Message::Health {
            request_id,
            node_id,
            ready,
            detail,
        } => {
            texts(&mut value, &[request_id, node_id])?;
            value.push(u8::from(*ready));
            put_text(&mut value, detail)?;
            HEALTH
        }
        Message::LoadProgress {
            operation_id,
            node_id,
            percent,
            detail,
        } => {
            texts(&mut value, &[operation_id, node_id])?;
            put_u32(&mut value, *percent);
            put_text(&mut value, detail)?;
            LOAD_PROGRESS
        }
        Message::DraftReport {
            operation_id,
            node_id,
            model_bytes,
            kv_bytes,
            layer_bytes,
            ffn_bytes,
            detail,
        } => {
            texts(&mut value, &[operation_id, node_id])?;
            for n in [model_bytes, kv_bytes, layer_bytes, ffn_bytes] {
                value.extend_from_slice(&n.to_le_bytes());
            }
            put_text(&mut value, detail)?;
            DRAFT_REPORT
        }
        Message::Error { request_id, detail } => {
            texts(&mut value, &[request_id, detail])?;
            ERROR
        }
    };
    Ok((kind, value))
}
