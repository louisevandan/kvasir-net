use super::*;
use crate::foundation::transport::{P4Handler, ResponseCollector, ResponseSink};
use p4_protocol::{ExecutionDone, ExecutionToken, Message};
use std::sync::Arc;

struct ScriptedAdapter;

impl P4Handler for ScriptedAdapter {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> Result<()> {
        match message {
            Message::NodeCreate {
                operation_id,
                node_id,
                adapter_id,
                ..
            } => responses.emit(Message::NodeCreated {
                operation_id,
                node_id,
                adapter_id,
                state: "ready".into(),
                detail: "in-memory adapter node ready".into(),
            }),
            Message::ModelLoad {
                operation_id,
                node_id,
                deployment_id,
                binding_id,
                ..
            } => responses.emit(Message::ModelBound {
                operation_id,
                node_id,
                deployment_id,
                binding_id,
                runtime_generation: 1,
                state: "ready".into(),
                detail: "in-memory adapter binding ready".into(),
            }),
            Message::ModelUnload {
                operation_id,
                node_id,
                deployment_id,
                binding_id,
                ..
            } => responses.emit(Message::ModelUnbound {
                operation_id,
                node_id,
                deployment_id,
                binding_id,
                detail: "in-memory adapter binding removed".into(),
            }),
            Message::Execute(request) => {
                responses.emit(Message::Token(ExecutionToken {
                    controller_id: request.controller_id.clone(),
                    node_id: request.node_id.clone(),
                    request_id: request.request_id.clone(),
                    session_id: request.session_id.clone(),
                    phase: request.phase,
                    position: request.position,
                    index: 0,
                    text: "streamed".into(),
                }))?;
                responses.emit(Message::Done(ExecutionDone {
                    controller_id: request.controller_id,
                    node_id: request.node_id,
                    request_id: request.request_id,
                    session_id: request.session_id,
                    reason: "length".into(),
                    generated_tokens: 1,
                }))
            }
            _ => unreachable!("test adapter received unsupported P4 message"),
        }
    }
}

/// An agent with one co-resident adapter, one created node, and one ready
/// binding at generation 1.
fn bound_agent(max_inflight: u32) -> AgentProcessor {
    let agent = AgentProcessor::new();
    agent
        .register_in_memory_adapter(
            "adapter-a".into(),
            "test".into(),
            "{}".into(),
            Arc::new(ScriptedAdapter),
        )
        .unwrap();
    let mut created = ResponseCollector::new();
    agent
        .handle(
            Message::NodeCreate {
                controller_id: "controller-a".into(),
                operation_id: "create-a".into(),
                node_id: "node-a".into(),
                adapter_id: "adapter-a".into(),
                node_spec: format!(r#"{{"p4_max_inflight":{max_inflight}}}"#),
            },
            &mut created,
        )
        .unwrap();
    assert!(matches!(
        created.terminal(),
        Some(Message::NodeCreated { .. })
    ));
    let mut loaded = ResponseCollector::new();
    agent.handle(load_message("deployment-a"), &mut loaded).unwrap();
    assert!(matches!(loaded.terminal(), Some(Message::ModelBound { .. })));
    agent
}

fn load_message(deployment_id: &str) -> Message {
    Message::ModelLoad {
        controller_id: "controller-a".into(),
        node_id: "node-a".into(),
        operation_id: "load-a".into(),
        deployment_id: deployment_id.into(),
        binding_id: "binding-a".into(),
        model: "model.gguf".into(),
        plan_revision: "test".into(),
        stage_plan: "{}".into(),
    }
}

fn unload_message(deployment_id: &str) -> Message {
    Message::ModelUnload {
        controller_id: "controller-a".into(),
        node_id: "node-a".into(),
        operation_id: "unload-a".into(),
        deployment_id: deployment_id.into(),
        binding_id: "binding-a".into(),
    }
}

fn ingress_message() -> Message {
    ingress_for("controller-a", 1)
}

fn ingress_for(controller_id: &str, runtime_generation: u64) -> Message {
    Message::IngressSubmit {
        controller_id: controller_id.into(),
        ingress_id: "ingress-a".into(),
        request_id: "request-a".into(),
        session_id: String::new(),
        node_id: "node-a".into(),
        deployment_id: "deployment-a".into(),
        binding_id: "binding-a".into(),
        runtime_generation,
        max_tokens: 1,
        temperature: 0.7,
        prompt: "hello".into(),
        options: "{}".into(),
    }
}

fn error_detail(responses: ResponseCollector) -> String {
    match responses.into_messages().pop() {
        Some(Message::Error { detail, .. }) => detail,
        other => panic!("expected an ERROR, got {other:?}"),
    }
}

#[test]
fn co_resident_agent_adapter_and_execution_use_one_in_memory_handler_path() {
    let agent = bound_agent(1);
    let mut ingress = ResponseCollector::new();
    agent.handle(ingress_message(), &mut ingress).unwrap();
    assert!(matches!(
        ingress.into_messages().as_slice(),
        [
            Message::IngressAccepted { .. },
            Message::Token(_),
            Message::Done(_)
        ]
    ));
}

#[test]
fn saturated_ingress_is_rejected_before_acceptance() {
    let agent = bound_agent(1);
    let slot = agent
        .state
        .read()
        .unwrap()
        .nodes
        .get("node-a")
        .cloned()
        .unwrap();
    let held = admission::execution(&slot).expect("hold the only permit");

    let mut responses = ResponseCollector::new();
    agent.handle(ingress_message(), &mut responses).unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("admission is full"), "{detail}");
    drop(held);
}

#[test]
fn another_controller_cannot_execute_on_an_owned_node() {
    let agent = bound_agent(1);
    let mut responses = ResponseCollector::new();
    agent
        .handle(ingress_for("controller-b", 1), &mut responses)
        .unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("is not owned by controller controller-b"), "{detail}");
}

#[test]
fn a_stale_runtime_generation_cannot_execute() {
    let agent = bound_agent(1);
    let mut responses = ResponseCollector::new();
    agent
        .handle(ingress_for("controller-a", 99), &mut responses)
        .unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("has no ready binding"), "{detail}");
}

#[test]
fn another_controller_cannot_claim_an_existing_node() {
    let agent = bound_agent(1);
    let mut responses = ResponseCollector::new();
    agent
        .handle(
            Message::NodeCreate {
                controller_id: "controller-b".into(),
                operation_id: "create-b".into(),
                node_id: "node-a".into(),
                adapter_id: "adapter-a".into(),
                node_spec: "{}".into(),
            },
            &mut responses,
        )
        .unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("is not owned by controller controller-b"), "{detail}");
}

#[test]
fn unloading_another_deployment_is_refused_and_keeps_the_binding() {
    let agent = bound_agent(1);
    let mut responses = ResponseCollector::new();
    agent
        .handle(unload_message("deployment-other"), &mut responses)
        .unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("belongs to deployment deployment-a"), "{detail}");

    let mut ingress = ResponseCollector::new();
    agent.handle(ingress_message(), &mut ingress).unwrap();
    assert!(
        matches!(
            ingress.into_messages().first(),
            Some(Message::IngressAccepted { .. })
        ),
        "a refused unload must leave the binding executable"
    );
}

#[test]
fn unloading_the_recorded_deployment_removes_the_binding() {
    let agent = bound_agent(1);
    let mut responses = ResponseCollector::new();
    agent
        .handle(unload_message("deployment-a"), &mut responses)
        .unwrap();
    assert!(matches!(
        responses.terminal(),
        Some(Message::ModelUnbound { .. })
    ));

    let mut ingress = ResponseCollector::new();
    agent.handle(ingress_message(), &mut ingress).unwrap();
    let detail = error_detail(ingress);
    assert!(detail.contains("has no ready binding"), "{detail}");
}

#[test]
fn an_execution_holds_the_slot_against_a_concurrent_unload() {
    let agent = bound_agent(1);
    let slot = agent
        .state
        .read()
        .unwrap()
        .nodes
        .get("node-a")
        .cloned()
        .unwrap();
    let held = admission::execution(&slot).expect("hold the only permit");

    let mut responses = ResponseCollector::new();
    agent
        .handle(unload_message("deployment-a"), &mut responses)
        .unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("admission is full"), "{detail}");
    drop(held);
}

#[test]
fn a_node_cannot_be_created_on_an_unregistered_adapter() {
    let agent = AgentProcessor::new();
    let mut responses = ResponseCollector::new();
    agent
        .handle(
            Message::NodeCreate {
                controller_id: "controller-a".into(),
                operation_id: "create-a".into(),
                node_id: "node-a".into(),
                adapter_id: "adapter-missing".into(),
                node_spec: "{}".into(),
            },
            &mut responses,
        )
        .unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("is not registered"), "{detail}");
}

#[test]
fn an_adapter_cannot_move_its_endpoint_while_slots_are_attached() {
    let agent = bound_agent(1);
    let mut responses = ResponseCollector::new();
    agent
        .handle(
            Message::AdapterRegister {
                adapter_id: "adapter-a".into(),
                adapter_kind: "test".into(),
                endpoint: "127.0.0.1:19999".into(),
                descriptor: "{}".into(),
            },
            &mut responses,
        )
        .unwrap();
    let detail = error_detail(responses);
    assert!(detail.contains("attached node"), "{detail}");

    let mut ingress = ResponseCollector::new();
    agent.handle(ingress_message(), &mut ingress).unwrap();
    assert!(
        matches!(
            ingress.into_messages().first(),
            Some(Message::IngressAccepted { .. })
        ),
        "a refused re-registration must leave the original route intact"
    );
}
