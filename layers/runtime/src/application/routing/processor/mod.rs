//! Controller ingress and remote compatibility routing. See `apps/p4/docs/architecture.md`.

use crate::domain::agent::AgentProcessor;
use crate::foundation::transport::{
    P4Handler, ResponseSink, Result, SharedHandler, SharedTransport, in_memory, reject, tcp,
};
use p4_protocol::{ExecutionRequest, Message, Phase};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct ControllerProcessor(ControllerMode);
pub struct NodeProcessor(NodeMode);

enum ControllerMode {
    Remote(RouteProcessor),
    Local {
        node: SharedTransport,
        sessions: AtomicU64,
    },
}

enum NodeMode {
    Static(RouteProcessor),
    Dynamic(AgentProcessor),
}

struct RouteProcessor {
    role: Role,
    targets: HashMap<String, SharedTransport>,
}

#[derive(Clone, Copy)]
enum Role {
    Controller,
    Node,
}

impl ControllerProcessor {
    pub fn remote(routes: HashMap<String, String>) -> Self {
        Self(ControllerMode::Remote(RouteProcessor::endpoints(
            Role::Controller,
            routes,
        )))
    }

    pub fn local(node: SharedHandler) -> Self {
        Self(ControllerMode::Local {
            node: in_memory(node),
            sessions: AtomicU64::new(1),
        })
    }
}

impl NodeProcessor {
    pub fn adapters(routes: HashMap<String, String>) -> Self {
        Self(NodeMode::Static(RouteProcessor::endpoints(
            Role::Node,
            routes,
        )))
    }

    pub fn dynamic() -> Self {
        Self(NodeMode::Dynamic(AgentProcessor::new()))
    }
}

impl P4Handler for ControllerProcessor {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        match &self.0 {
            ControllerMode::Remote(routes) => routes.handle(message, responses),
            ControllerMode::Local { node, sessions } => match message {
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
                    let session_id = if session_id.is_empty() {
                        format!(
                            "{controller_id}-session-{}",
                            sessions.fetch_add(1, Ordering::Relaxed)
                        )
                    } else {
                        session_id
                    };
                    responses.emit(Message::IngressAccepted {
                        ingress_id,
                        request_id: request_id.clone(),
                        session_id: session_id.clone(),
                    })?;
                    node.dispatch(
                        Message::Execute(ExecutionRequest {
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
                        }),
                        responses,
                    )
                }
                other => node.dispatch(other, responses),
            },
        }
    }
}

impl P4Handler for NodeProcessor {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        match &self.0 {
            NodeMode::Static(routes) => routes.handle(message, responses),
            NodeMode::Dynamic(agent) => agent.handle(message, responses),
        }
    }
}

impl RouteProcessor {
    fn endpoints(role: Role, routes: HashMap<String, String>) -> Self {
        let targets = routes
            .into_iter()
            .map(|(id, endpoint)| (id, tcp(endpoint)))
            .collect();
        Self { role, targets }
    }

    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        let (node_id, request_id) = route_key(&message, self.role)?;
        let Some(target) = self.targets.get(&node_id) else {
            return reject(
                responses,
                Message::Error {
                    request_id,
                    detail: format!("no route for node {node_id}"),
                },
            );
        };
        target.dispatch(message, responses)
    }
}

fn route_key(message: &Message, role: Role) -> Result<(String, String)> {
    let value = match message {
        Message::Execute(v) => (v.node_id.clone(), v.request_id.clone()),
        Message::NodeCreate {
            node_id,
            operation_id,
            ..
        }
        | Message::ModelLoad {
            node_id,
            operation_id,
            ..
        }
        | Message::ModelUnload {
            node_id,
            operation_id,
            ..
        } => (node_id.clone(), operation_id.clone()),
        Message::HealthCheck {
            node_id,
            request_id,
            ..
        } => (node_id.clone(), request_id.clone()),
        _ => {
            return Err(
                format!("{:?} router cannot route this P4 message", role_name(role)).into(),
            );
        }
    };
    Ok(value)
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Controller => "controller",
        Role::Node => "node",
    }
}
