//! Controller-facing protocol operations: adapter registration, NodeSlot
//! creation, model binding lifecycle, inventory, and health.
//!
//! Every operation resolves its adapter through the registry rather than a
//! handle cached on the slot, and every controller-facing one passes the
//! authorization gate first.

pub(crate) mod forward;
pub(crate) mod node_spec;

use super::admission;
use super::authorization::{self, Denial};
use super::registry::adapter::{Adapter, RegistrationRefusal, authorize_registration};
use super::registry::node::{NodeSlot, UnbindRefusal};
use super::{AgentProcessor, ResolvedNode};
use crate::foundation::transport::{
    ResponseSink, Result, SharedHandler, in_memory, reject, tcp,
};
use p4_protocol::Message;
use std::net::ToSocketAddrs;

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
            return self.refuse(
                responses,
                &adapter_id,
                &RegistrationRefusal::Malformed.detail(),
            );
        }
        let mut state = self
            .state
            .write()
            .map_err(|_| "agent registry lock poisoned")?;
        let attached = state.attached_nodes(&adapter_id);
        if let Err(refusal) =
            authorize_registration(state.adapters.get(&adapter_id), &endpoint, attached)
        {
            drop(state);
            return self.refuse(responses, &adapter_id, &refusal.detail());
        }
        state.adapters.insert(
            adapter_id.clone(),
            Adapter {
                kind,
                transport: tcp(endpoint.clone()),
                endpoint: Some(endpoint),
                descriptor,
            },
        );
        drop(state);
        responses.emit(Message::AdapterRegistered {
            adapter_id,
            detail: "registered".into(),
        })
    }

    /// Installs a co-resident concrete adapter without creating a loopback
    /// socket. See `apps/p4/docs/internals.md#transport-neutral-dispatch`.
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
        drop(state);
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
        let transport = {
            let state = self
                .state
                .read()
                .map_err(|_| "agent registry lock poisoned")?;
            if let Err(denial) =
                authorization::node_create(&state, &controller_id, &node_id, &adapter_id)
            {
                drop(state);
                return self.deny(responses, &operation_id, &denial);
            }
            state
                .adapters
                .get(&adapter_id)
                .expect("authorization confirmed the adapter")
                .transport
                .clone()
        };
        let max_inflight = node_spec::max_inflight(&node_spec);
        let response = forward::capture(
            responses,
            &transport,
            Message::NodeCreate {
                controller_id: controller_id.clone(),
                operation_id,
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
                .entry(node_id)
                .or_insert_with(|| NodeSlot::new(controller_id, adapter_id, max_inflight));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
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
        let Some((resolved, permit)) =
            self.exclusive(responses, &controller_id, &node_id, &operation_id)?
        else {
            return Ok(());
        };
        let response = forward::capture(
            responses,
            &resolved.adapter.transport,
            Message::ModelLoad {
                controller_id,
                node_id: node_id.clone(),
                operation_id,
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
            && state == "ready"
        {
            self.state
                .write()
                .map_err(|_| "agent registry lock poisoned")?
                .nodes
                .get_mut(&node_id)
                .ok_or("node disappeared")?
                .bind(binding_id, deployment_id, runtime_generation);
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
        let Some((resolved, permit)) =
            self.exclusive(responses, &controller_id, &node_id, &operation_id)?
        else {
            return Ok(());
        };
        let response = forward::capture(
            responses,
            &resolved.adapter.transport,
            Message::ModelUnload {
                controller_id,
                node_id: node_id.clone(),
                operation_id: operation_id.clone(),
                deployment_id: deployment_id.clone(),
                binding_id: binding_id.clone(),
            },
        )?;
        if matches!(response, Message::ModelUnbound { .. }) {
            let mut state = self
                .state
                .write()
                .map_err(|_| "agent registry lock poisoned")?;
            let slot = state.nodes.get_mut(&node_id).ok_or("node disappeared")?;
            if let Err(refusal) = slot.unbind(&binding_id, &deployment_id) {
                drop(state);
                drop(permit);
                return self.refuse(responses, &operation_id, &unbind_detail(&refusal, &binding_id));
            }
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
        let resolved = match self.owned(&controller_id, &node_id, &request_id)? {
            Ok(resolved) => resolved,
            Err(denial) => return self.deny(responses, &request_id, &denial),
        };
        forward::capture(
            responses,
            &resolved.adapter.transport,
            Message::HealthCheck {
                controller_id,
                node_id,
                request_id,
            },
        )
        .map(|_| ())
    }

    /// Resolves an owned slot and takes every permit, so no execution can
    /// overlap the binding transition that follows.
    fn exclusive(
        &self,
        responses: &mut dyn ResponseSink,
        controller_id: &str,
        node_id: &str,
        correlation_id: &str,
    ) -> Result<Option<(ResolvedNode, tokio::sync::OwnedSemaphorePermit)>> {
        let resolved = match self.owned(controller_id, node_id, correlation_id)? {
            Ok(resolved) => resolved,
            Err(denial) => return self.deny(responses, correlation_id, &denial).map(|()| None),
        };
        let Some(permit) = admission::lifecycle(&resolved.slot) else {
            return self
                .refuse(
                    responses,
                    correlation_id,
                    &admission::saturated_detail(node_id),
                )
                .map(|()| None);
        };
        Ok(Some((resolved, permit)))
    }

    pub(super) fn owned(
        &self,
        controller_id: &str,
        node_id: &str,
        correlation_id: &str,
    ) -> Result<std::result::Result<ResolvedNode, Denial>> {
        let state = self
            .state
            .read()
            .map_err(|_| "agent registry lock poisoned")?;
        Ok(authorization::owned_node(
            &state,
            controller_id,
            node_id,
            correlation_id,
        ))
    }

    pub(super) fn deny(
        &self,
        responses: &mut dyn ResponseSink,
        correlation_id: &str,
        denial: &Denial,
    ) -> Result<()> {
        self.refuse(responses, correlation_id, &denial.detail())
    }

    pub(super) fn refuse(
        &self,
        responses: &mut dyn ResponseSink,
        correlation_id: &str,
        detail: &str,
    ) -> Result<()> {
        reject(
            responses,
            Message::Error {
                request_id: correlation_id.into(),
                detail: detail.into(),
            },
        )
    }
}

fn unbind_detail(refusal: &UnbindRefusal, binding_id: &str) -> String {
    match refusal {
        UnbindRefusal::UnknownBinding => {
            format!("binding {binding_id} is not recorded on this node")
        }
        UnbindRefusal::DeploymentMismatch { recorded } => format!(
            "binding {binding_id} belongs to deployment {recorded}; refusing to unbind it for another deployment"
        ),
    }
}
