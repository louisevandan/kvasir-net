//! Ingress-to-execution credit transition.
//!
//! `INGRESS_ACCEPTED` is emitted only after the gate has authorised the slot
//! and binding and the execution credit is held, so acceptance already means
//! "a stable binding is reserved for this request".
//! See `apps/p4/docs/internals.md#execution-credit`.

use super::lifecycle::forward;
use super::{AgentProcessor, AsyncIngress};
use crate::foundation::transport::{ResponseCollector, ResponseSink, Result};
use p4_protocol::{ExecutionRequest, Message, Phase};
use std::sync::atomic::Ordering;

impl AgentProcessor {
    pub(crate) fn prepare_async_ingress(
        &self,
        message: Message,
    ) -> std::result::Result<AsyncIngress, Message> {
        let Message::IngressSubmit {
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
        } = message
        else {
            return Err(Message::Error {
                request_id: "unknown".into(),
                detail: "async ingress requires INGRESS_SUBMIT".into(),
            });
        };
        let session_id = if session_id.is_empty() {
            self.issue_session(&controller_id)
        } else {
            session_id
        };
        let request = self.execution_request(
            controller_id,
            node_id,
            deployment_id,
            binding_id,
            runtime_generation,
            request_id.clone(),
            session_id.clone(),
            max_tokens,
            temperature,
            prompt,
            options,
        );
        let mut responses = ResponseCollector::new();
        let acquired = self
            .acquire_execution(&mut responses, &request)
            .map_err(|error| Message::Error {
                request_id: request_id.clone(),
                detail: error.to_string(),
            })?;
        let Some((resolved, permit)) = acquired else {
            return Err(responses.into_messages().pop().unwrap_or(Message::Error {
                request_id,
                detail: "execution admission rejected".into(),
            }));
        };
        Ok(AsyncIngress {
            accepted: Message::IngressAccepted {
                ingress_id,
                request_id,
                session_id,
            },
            execution: super::AsyncExecution {
                execute: Message::Execute(request),
                transport: resolved.adapter.transport,
                endpoint: resolved.adapter.endpoint,
                _permit: permit,
            },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn ingress(
        &self,
        responses: &mut dyn ResponseSink,
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
    ) -> Result<()> {
        let session_id = if session_id.is_empty() {
            self.issue_session(&controller_id)
        } else {
            session_id
        };
        let request = self.execution_request(
            controller_id,
            node_id,
            deployment_id,
            binding_id,
            runtime_generation,
            request_id.clone(),
            session_id.clone(),
            max_tokens,
            temperature,
            prompt,
            options,
        );
        let Some((resolved, permit)) = self.acquire_execution(responses, &request)? else {
            return Ok(());
        };
        responses.emit(Message::IngressAccepted {
            ingress_id,
            request_id,
            session_id,
        })?;
        let result = forward::capture(
            responses,
            &resolved.adapter.transport,
            Message::Execute(request),
        )
        .map(|_| ());
        drop(permit);
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn execution_request(
        &self,
        controller_id: String,
        node_id: String,
        deployment_id: String,
        binding_id: String,
        runtime_generation: u64,
        request_id: String,
        session_id: String,
        max_tokens: u32,
        temperature: f32,
        prompt: String,
        options: String,
    ) -> ExecutionRequest {
        ExecutionRequest {
            controller_id,
            node_id,
            deployment_id,
            binding_id,
            runtime_generation,
            request_id,
            session_id,
            phase: Phase::Prefill,
            position: 0,
            max_tokens,
            temperature,
            prompt,
            options,
        }
    }

    fn issue_session(&self, controller_id: &str) -> String {
        format!(
            "{controller_id}-session-{}",
            self.session_sequence.fetch_add(1, Ordering::Relaxed)
        )
    }
}

#[cfg(test)]
mod tests;
