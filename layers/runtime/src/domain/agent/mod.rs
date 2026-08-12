//! Agent-owned adapter registry, NodeSlot lifecycle, and hardware inventory.
//!
//! The Agent is the rejection gate between controllers and concrete runtimes.
//! It keeps node/controller/binding state until the binding is unloaded and
//! refuses any request that contradicts it; concrete adapters re-check binding
//! generation but never key on `controller_id`, so ownership lives only here.
//! See `apps/p4/docs/internals.md#agent-state`.

mod admission;
mod authorization;
mod ingress;
mod lifecycle;
mod registry;
#[cfg(test)]
mod tests;

use crate::foundation::transport::{P4Handler, ResponseSink, Result, SharedTransport};
use p4_protocol::Message;
use std::env;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use tokio::sync::OwnedSemaphorePermit;

use registry::node::NodeSlot;
pub(crate) use registry::{Registry, ResolvedNode};

pub struct AgentProcessor {
    agent_id: String,
    session_sequence: AtomicU64,
    state: RwLock<Registry>,
}

pub(crate) struct AsyncIngress {
    pub(crate) accepted: Message,
    pub(crate) execution: AsyncExecution,
}

pub(crate) struct AsyncExecution {
    pub(crate) execute: Message,
    pub(crate) transport: SharedTransport,
    pub(crate) endpoint: Option<String>,
    pub(crate) slot: NodeSlot,
}

impl AgentProcessor {
    pub fn new() -> Self {
        let host = env::var("COMPUTERNAME")
            .or_else(|_| env::var("HOSTNAME"))
            .unwrap_or_else(|_| "unknown-host".into());
        Self {
            agent_id: format!("agent-{host}-{}", std::process::id()),
            session_sequence: AtomicU64::new(1),
            state: RwLock::new(Registry::default()),
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.agent_id
    }

    pub(crate) fn prepare_async_execution(
        &self,
        message: Message,
    ) -> std::result::Result<AsyncExecution, Message> {
        let Message::Execute(request) = &message else {
            return Err(Message::Error {
                request_id: "unknown".into(),
                detail: "async execution requires EXECUTE".into(),
            });
        };
        let resolved = self
            .resolve_execution(request)
            .map_err(|detail| Message::Error {
                request_id: request.request_id.clone(),
                detail,
            })?;
        Ok(AsyncExecution {
            execute: message,
            transport: resolved.adapter.transport,
            endpoint: resolved.adapter.endpoint,
            slot: resolved.slot,
        })
    }

    pub(crate) async fn acquire_async_execution(
        &self,
        execution: &AsyncExecution,
    ) -> std::result::Result<OwnedSemaphorePermit, String> {
        let permit = Arc::clone(&execution.slot.admission)
            .acquire_owned()
            .await
            .map_err(|_| "execution admission closed".to_owned())?;
        let Message::Execute(request) = &execution.execute else {
            unreachable!()
        };
        if let Err(detail) = self.resolve_execution(request) {
            drop(permit);
            return Err(detail);
        }
        Ok(permit)
    }

    pub fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        match message {
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
            } => self.ingress(
                responses,
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
            ),
            Message::AdapterRegister {
                adapter_id,
                adapter_kind,
                endpoint,
                descriptor,
            } => self.register(responses, adapter_id, adapter_kind, endpoint, descriptor),
            Message::InventoryQuery { request_id, .. } => self.inventory(responses, request_id),
            Message::NodeCreate {
                controller_id,
                operation_id,
                node_id,
                adapter_id,
                node_spec,
            } => self.create_node(
                responses,
                controller_id,
                operation_id,
                node_id,
                adapter_id,
                node_spec,
            ),
            Message::ModelLoad {
                controller_id,
                node_id,
                operation_id,
                deployment_id,
                binding_id,
                model,
                plan_revision,
                stage_plan,
            } => self.load_model(
                responses,
                controller_id,
                node_id,
                operation_id,
                deployment_id,
                binding_id,
                model,
                plan_revision,
                stage_plan,
            ),
            Message::ModelUnload {
                controller_id,
                node_id,
                operation_id,
                deployment_id,
                binding_id,
            } => self.unload_model(
                responses,
                controller_id,
                node_id,
                operation_id,
                deployment_id,
                binding_id,
            ),
            Message::Execute(request) => self.execute(responses, Message::Execute(request)),
            Message::HealthCheck {
                node_id,
                request_id,
                controller_id,
            } => self.health(responses, controller_id, node_id, request_id),
            _other => self.refuse(
                responses,
                "unknown",
                "agent cannot process this message",
            ),
        }
    }

    fn execute(&self, responses: &mut dyn ResponseSink, message: Message) -> Result<()> {
        let Message::Execute(request) = message else {
            unreachable!()
        };
        let Some((resolved, permit)) = self.acquire_execution(responses, &request)? else {
            return Ok(());
        };
        let result = lifecycle::forward::capture(
            responses,
            &resolved.adapter.transport,
            Message::Execute(request),
        )
        .map(|_| ());
        drop(permit);
        result
    }

    /// Ownership, binding readiness, then credit — in that order, so a
    /// request that fails the gate never consumes capacity.
    /// See `apps/p4/docs/internals.md#execution-credit`.
    pub(crate) fn acquire_execution(
        &self,
        responses: &mut dyn ResponseSink,
        request: &p4_protocol::ExecutionRequest,
    ) -> Result<Option<(ResolvedNode, OwnedSemaphorePermit)>> {
        let resolved = match self.resolve_execution(request) {
            Ok(resolved) => resolved,
            Err(detail) => {
                return self
                    .refuse(responses, &request.request_id, &detail)
                    .map(|()| None);
            }
        };
        let Some(permit) = admission::execution(&resolved.slot) else {
            return self
                .refuse(
                    responses,
                    &request.request_id,
                    &admission::saturated_detail(&request.node_id),
                )
                .map(|()| None);
        };
        Ok(Some((resolved, permit)))
    }

    fn resolve_execution(
        &self,
        request: &p4_protocol::ExecutionRequest,
    ) -> std::result::Result<ResolvedNode, String> {
        let resolved = self
            .owned(
                &request.controller_id,
                &request.node_id,
                &request.request_id,
            )
            .map_err(|error| error.to_string())?
            .map_err(|denial| denial.detail())?;
        authorization::execution(&resolved, request).map_err(|denial| denial.detail())?;
        Ok(resolved)
    }
}

impl P4Handler for AgentProcessor {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        AgentProcessor::handle(self, message, responses)
    }
}
