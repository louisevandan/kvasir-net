use super::*;
use crate::{P4Handler, ResponseCollector, ResponseSink, in_memory};
use p4_protocol::{ExecutionDone, ExecutionToken, Message};

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

#[test]
fn node_admission_defaults_to_one_and_is_bounded() {
    assert_eq!(lifecycle::max_inflight("{}"), 1);
    assert_eq!(lifecycle::max_inflight(r#"{"p4_max_inflight":4}"#), 4);
    assert_eq!(lifecycle::max_inflight(r#"{"p4_max_inflight":1025}"#), 1);
}

#[test]
fn co_resident_agent_adapter_and_execution_use_one_in_memory_handler_path() {
    let agent = AgentProcessor::new();
    agent
        .register_in_memory_adapter(
            "adapter-a".into(),
            "test".into(),
            "{}".into(),
            Arc::new(ScriptedAdapter),
        )
        .unwrap();

    let mut responses = ResponseCollector::new();
    agent
        .handle(
            Message::NodeCreate {
                controller_id: "controller-a".into(),
                operation_id: "create-a".into(),
                node_id: "node-a".into(),
                adapter_id: "adapter-a".into(),
                node_spec: r#"{"p4_max_inflight":1}"#.into(),
            },
            &mut responses,
        )
        .unwrap();
    assert!(matches!(
        responses.terminal(),
        Some(Message::NodeCreated { .. })
    ));

    let mut load = ResponseCollector::new();
    agent
        .handle(
            Message::ModelLoad {
                controller_id: "controller-a".into(),
                node_id: "node-a".into(),
                operation_id: "load-a".into(),
                deployment_id: "deployment-a".into(),
                binding_id: "binding-a".into(),
                model: "model.gguf".into(),
                plan_revision: "test".into(),
                stage_plan: "{}".into(),
            },
            &mut load,
        )
        .unwrap();
    assert!(matches!(load.terminal(), Some(Message::ModelBound { .. })));

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
    let agent = AgentProcessor {
        agent_id: "agent-test".into(),
        session_sequence: AtomicU64::new(1),
        state: RwLock::new(Registry {
            adapters: HashMap::new(),
            nodes: HashMap::from([(
                "node-a".into(),
                NodeSlot {
                    controller_id: "controller-a".into(),
                    adapter_id: "adapter-a".into(),
                    transport: in_memory(Arc::new(ScriptedAdapter)),
                    endpoint: None,
                    max_inflight: 1,
                    admission: Arc::new(Semaphore::new(0)),
                    bindings: HashMap::from([(
                        "binding-a".into(),
                        Binding {
                            deployment_id: "deployment-a".into(),
                            generation: 1,
                        },
                    )]),
                },
            )]),
        }),
    };
    let mut responses = ResponseCollector::new();
    agent.handle(ingress_message(), &mut responses).unwrap();
    assert!(matches!(responses.terminal(), Some(Message::Error { .. })));
}

fn ingress_message() -> Message {
    Message::IngressSubmit {
        controller_id: "controller-a".into(),
        ingress_id: "ingress-a".into(),
        request_id: "request-a".into(),
        session_id: String::new(),
        node_id: "node-a".into(),
        deployment_id: "deployment-a".into(),
        binding_id: "binding-a".into(),
        runtime_generation: 1,
        max_tokens: 1,
        temperature: 0.7,
        prompt: "hello".into(),
        options: "{}".into(),
    }
}
