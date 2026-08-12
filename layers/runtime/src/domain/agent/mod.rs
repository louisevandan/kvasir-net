//! Agent-owned adapter registry, NodeSlot lifecycle, and hardware inventory.
//! See `apps/p4/docs/internals.md#agent-state`.

mod ingress;
mod lifecycle;
#[cfg(test)]
mod tests;

use crate::foundation::transport::{P4Handler, ResponseSink, Result, SharedTransport, reject};
use p4_protocol::Message;
use std::collections::HashMap;
use std::env;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct AgentProcessor {
    agent_id: String,
    session_sequence: AtomicU64,
    state: RwLock<Registry>,
}

#[derive(Default)]
pub(super) struct Registry {
    pub(super) adapters: HashMap<String, Adapter>,
    pub(super) nodes: HashMap<String, NodeSlot>,
}

#[derive(Clone)]
pub(super) struct Adapter {
    pub(super) kind: String,
    pub(super) transport: SharedTransport,
    pub(super) endpoint: Option<String>,
    pub(super) descriptor: String,
}

#[derive(Clone)]
pub(super) struct NodeSlot {
    pub(super) controller_id: String,
    pub(super) adapter_id: String,
    pub(super) transport: SharedTransport,
    pub(super) endpoint: Option<String>,
    pub(super) max_inflight: u32,
    pub(super) admission: Arc<Semaphore>,
    pub(super) bindings: HashMap<String, Binding>,
}

pub(crate) struct AsyncIngress {
    pub(crate) accepted: Message,
    pub(crate) execution: AsyncExecution,
}

pub(crate) struct AsyncExecution {
    pub(crate) execute: Message,
    pub(crate) transport: SharedTransport,
    pub(crate) endpoint: Option<String>,
    pub(crate) _permit: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub(super) struct Binding {
    deployment_id: String,
    generation: u64,
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
        let mut responses = crate::foundation::transport::ResponseCollector::new();
        let acquired = self
            .acquire_execution(&mut responses, request)
            .map_err(|error| Message::Error {
                request_id: request.request_id.clone(),
                detail: error.to_string(),
            })?;
        let Some((slot, permit)) = acquired else {
            return Err(responses.into_messages().pop().unwrap_or(Message::Error {
                request_id: request.request_id.clone(),
                detail: "execution admission rejected".into(),
            }));
        };
        Ok(AsyncExecution {
            execute: message,
            transport: slot.transport,
            endpoint: slot.endpoint,
            _permit: permit,
        })
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
            _other => reject(
                responses,
                Message::Error {
                    request_id: "unknown".into(),
                    detail: "agent cannot process this message".into(),
                },
            ),
        }
    }

    fn execute(&self, responses: &mut dyn ResponseSink, message: Message) -> Result<()> {
        let Message::Execute(request) = message else {
            unreachable!()
        };
        let Some((slot, permit)) = self.acquire_execution(responses, &request)? else {
            return Ok(());
        };
        let result =
            lifecycle::forward_capture(responses, &slot.transport, Message::Execute(request))
                .map(|_| ());
        drop(permit);
        result
    }

    /// See `apps/p4/docs/internals.md#execution-credit`.
    pub(super) fn acquire_execution(
        &self,
        responses: &mut dyn ResponseSink,
        request: &p4_protocol::ExecutionRequest,
    ) -> Result<Option<(NodeSlot, OwnedSemaphorePermit)>> {
        let slot = match self.node(
            &request.controller_id,
            &request.node_id,
            &request.request_id,
        ) {
            Ok(slot) => slot,
            Err(error) => {
                return reject(
                    responses,
                    Message::Error {
                        request_id: request.request_id.clone(),
                        detail: error.to_string(),
                    },
                )
                .map(|()| None);
            }
        };
        let binding = slot.bindings.get(&request.binding_id);
        if !matches!(binding, Some(value) if value.deployment_id == request.deployment_id && value.generation == request.runtime_generation)
        {
            return reject(
                responses,
                Message::Error {
                    request_id: request.request_id.clone(),
                    detail: format!(
                        "node {} has no ready binding {} generation {}",
                        request.node_id, request.binding_id, request.runtime_generation
                    ),
                },
            )
            .map(|()| None);
        }
        let permit = match Arc::clone(&slot.admission).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                self.busy(responses, &request.request_id, &request.node_id)?;
                return Ok(None);
            }
        };
        Ok(Some((slot, permit)))
    }

    fn node(&self, controller_id: &str, node_id: &str, request_id: &str) -> Result<NodeSlot> {
        let slot = self
            .state
            .read()
            .map_err(|_| "agent registry lock poisoned")?
            .nodes
            .get(node_id)
            .cloned()
            .ok_or_else(|| {
                format!("node instance {node_id} is not created; request {request_id}")
            })?;
        if slot.controller_id != controller_id {
            return Err(format!(
                "node instance {node_id} is not owned by controller {controller_id}"
            )
            .into());
        }
        Ok(slot)
    }

    fn busy(
        &self,
        responses: &mut dyn ResponseSink,
        request_id: &str,
        node_id: &str,
    ) -> Result<()> {
        reject(
            responses,
            Message::Error {
                request_id: request_id.into(),
                detail: format!(
                    "node {node_id} admission is full; retry after an active stream completes"
                ),
            },
        )
    }
}

impl P4Handler for AgentProcessor {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        AgentProcessor::handle(self, message, responses)
    }
}
