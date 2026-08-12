//! Adapter registration, NodeSlot lifecycle, and terminal-response forwarding.

use super::{Adapter, AgentProcessor, Binding, NodeSlot};
use crate::foundation::transport::{
    ResponseSink, Result, SharedHandler, SharedTransport, in_memory, reject, tcp, terminal,
};
use p4_protocol::Message;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tokio::sync::Semaphore;

impl AgentProcessor {
    pub(super) fn register(
        &self,
        responses: &mut dyn ResponseSink,
        adapter_id: String,
        kind: String,
        endpoint: String,
        descriptor: String,
    ) -> Result<()> {
        if adapter_id.is_empty() || kind.is_empty() || endpoint.to_socket_addrs().is_err() {
            return reject(
                responses,
                Message::Error {
                    request_id: adapter_id,
                    detail: "adapter registration needs ID, kind, and valid endpoint".into(),
                },
            );
        }
        self.state
            .write()
            .map_err(|_| "agent registry lock poisoned")?
            .adapters
            .insert(
                adapter_id.clone(),
                Adapter {
                    kind,
                    transport: tcp(endpoint.clone()),
                    endpoint: Some(endpoint),
                    descriptor,
                },
            );
        responses.emit(Message::AdapterRegistered {
            adapter_id,
            detail: "registered".into(),
        })
    }

    /// Installs a co-resident concrete adapter without creating a loopback socket.
    /// See `apps/p4/docs/internals.md#transport-neutral-dispatch`.
    pub fn register_in_memory_adapter(
        &self,
        adapter_id: String,
        kind: String,
        descriptor: String,
        handler: SharedHandler,
    ) -> Result<()> {
        if adapter_id.is_empty() || kind.is_empty() {
            return Err("in-memory adapter registration needs ID and kind".into());
        }
        self.state
            .write()
            .map_err(|_| "agent registry lock poisoned")?
            .adapters
            .insert(
                adapter_id,
                Adapter {
                    kind,
                    transport: in_memory(handler),
                    endpoint: None,
                    descriptor,
                },
            );
        Ok(())
    }

    pub(super) fn inventory(
        &self,
        responses: &mut dyn ResponseSink,
        request_id: String,
    ) -> Result<()> {
        let state = self
            .state
            .read()
            .map_err(|_| "agent registry lock poisoned")?;
        let snapshot = crate::domain::hardware::snapshot(&state);
        responses.emit(Message::HardwareReport {
            agent_id: self.agent_id.clone(),
            report_id: request_id,
            snapshot,
        })
    }

    pub(super) fn create_node(
        &self,
        responses: &mut dyn ResponseSink,
        controller_id: String,
        operation_id: String,
        node_id: String,
        adapter_id: String,
        node_spec: String,
    ) -> Result<()> {
        let adapter = self
            .state
            .read()
            .map_err(|_| "agent registry lock poisoned")?
            .adapters
            .get(&adapter_id)
            .cloned();
        let Some(adapter) = adapter else {
            return reject(
                responses,
                Message::Error {
                    request_id: operation_id,
                    detail: format!("adapter {adapter_id} is not registered"),
                },
            );
        };
        if let Some(existing) = self
            .state
            .read()
            .map_err(|_| "agent registry lock poisoned")?
            .nodes
            .get(&node_id)
            .cloned()
        {
            if existing.controller_id != controller_id {
                return reject(
                    responses,
                    Message::Error {
                        request_id: operation_id,
                        detail: format!("node {node_id} is owned by another controller"),
                    },
                );
            }
            if existing.adapter_id != adapter_id {
                return reject(
                    responses,
                    Message::Error {
                        request_id: operation_id,
                        detail: format!("node {node_id} is already attached to another adapter"),
                    },
                );
            }
        }
        let max_inflight = max_inflight(&node_spec);
        let response = forward_capture(
            responses,
            &adapter.transport,
            Message::NodeCreate {
                controller_id: controller_id.clone(),
                operation_id: operation_id.clone(),
                node_id: node_id.clone(),
                adapter_id: adapter_id.clone(),
                node_spec,
            },
        )?;
        if matches!(response, Message::NodeCreated { state, .. } if state == "ready") {
            self.state
                .write()
                .map_err(|_| "agent registry lock poisoned")?
                .nodes
                .insert(
                    node_id,
                    NodeSlot {
                        controller_id,
                        adapter_id,
                        transport: adapter.transport,
                        endpoint: adapter.endpoint,
                        max_inflight,
                        admission: Arc::new(Semaphore::new(max_inflight as usize)),
                        bindings: HashMap::new(),
                    },
                );
        }
        Ok(())
    }

    pub(super) fn load_model(
        &self,
        responses: &mut dyn ResponseSink,
        controller_id: String,
        node_id: String,
        operation_id: String,
        deployment_id: String,
        binding_id: String,
        model: String,
        plan_revision: String,
        stage_plan: String,
    ) -> Result<()> {
        let slot = self.node(&controller_id, &node_id, &operation_id)?;
        let permit = match Arc::clone(&slot.admission).try_acquire_many_owned(slot.max_inflight) {
            Ok(permit) => permit,
            Err(_) => return self.busy(responses, &operation_id, &node_id),
        };
        let response = forward_capture(
            responses,
            &slot.transport,
            Message::ModelLoad {
                controller_id,
                node_id: node_id.clone(),
                operation_id: operation_id.clone(),
                deployment_id: deployment_id.clone(),
                binding_id: binding_id.clone(),
                model,
                plan_revision,
                stage_plan,
            },
        )?;
        if let Message::ModelBound {
            state,
            runtime_generation,
            ..
        } = response
        {
            if state == "ready" {
                self.state
                    .write()
                    .map_err(|_| "agent registry lock poisoned")?
                    .nodes
                    .get_mut(&node_id)
                    .ok_or("node disappeared")?
                    .bindings
                    .insert(
                        binding_id,
                        Binding {
                            deployment_id,
                            generation: runtime_generation,
                        },
                    );
            }
        }
        drop(permit);
        Ok(())
    }

    pub(super) fn unload_model(
        &self,
        responses: &mut dyn ResponseSink,
        controller_id: String,
        node_id: String,
        operation_id: String,
        deployment_id: String,
        binding_id: String,
    ) -> Result<()> {
        let slot = self.node(&controller_id, &node_id, &operation_id)?;
        let permit = match Arc::clone(&slot.admission).try_acquire_many_owned(slot.max_inflight) {
            Ok(permit) => permit,
            Err(_) => return self.busy(responses, &operation_id, &node_id),
        };
        let response = forward_capture(
            responses,
            &slot.transport,
            Message::ModelUnload {
                controller_id,
                node_id: node_id.clone(),
                operation_id: operation_id.clone(),
                deployment_id,
                binding_id: binding_id.clone(),
            },
        )?;
        if matches!(response, Message::ModelUnbound { .. }) {
            self.state
                .write()
                .map_err(|_| "agent registry lock poisoned")?
                .nodes
                .get_mut(&node_id)
                .ok_or("node disappeared")?
                .bindings
                .remove(&binding_id);
        }
        drop(permit);
        Ok(())
    }

    pub(super) fn health(
        &self,
        responses: &mut dyn ResponseSink,
        controller_id: String,
        node_id: String,
        request_id: String,
    ) -> Result<()> {
        let slot = self.node(&controller_id, &node_id, &request_id)?;
        forward_capture(
            responses,
            &slot.transport,
            Message::HealthCheck {
                controller_id,
                node_id,
                request_id,
            },
        )
        .map(|_| ())
    }
}

pub(super) fn max_inflight(node_spec: &str) -> u32 {
    serde_json::from_str::<serde_json::Value>(node_spec)
        .ok()
        .and_then(|value| {
            value
                .get("p4_max_inflight")
                .and_then(serde_json::Value::as_u64)
        })
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| (1..=1024).contains(value))
        .unwrap_or(1)
}

pub(super) fn forward_capture(
    responses: &mut dyn ResponseSink,
    transport: &SharedTransport,
    message: Message,
) -> Result<Message> {
    let mut forwarded = ForwardCaptureSink {
        downstream: responses,
        terminal: None,
    };
    transport.dispatch(message, &mut forwarded)?;
    forwarded
        .terminal
        .ok_or_else(|| "P4 transport completed without a terminal response".into())
}

struct ForwardCaptureSink<'a> {
    downstream: &'a mut dyn ResponseSink,
    terminal: Option<Message>,
}

impl ResponseSink for ForwardCaptureSink<'_> {
    fn emit(&mut self, message: Message) -> Result<()> {
        if terminal(&message) {
            self.terminal = Some(message.clone());
        }
        self.downstream.emit(message)
    }
}
