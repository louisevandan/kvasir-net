use super::*;
use crate::TaskQueue;
use crate::foundation::transport::{ResponseSink, Result as P4Result};
use crate::{LaneConfig, TaskQueueConfig};
use p4_protocol::{ExecutionDone, ExecutionToken};
use std::time::Duration;

struct ScriptedAdapter;

impl crate::P4Handler for ScriptedAdapter {
    fn handle(&self, message: Message, responses: &mut dyn ResponseSink) -> P4Result<()> {
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
                detail: "ready".into(),
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
                detail: "bound".into(),
            }),
            Message::Execute(request) => {
                for index in 0..request.max_tokens {
                    responses.emit(Message::Token(ExecutionToken {
                        controller_id: request.controller_id.clone(),
                        node_id: request.node_id.clone(),
                        request_id: request.request_id.clone(),
                        session_id: request.session_id.clone(),
                        phase: request.phase.clone(),
                        position: request.position,
                        index,
                        text: index.to_string(),
                    }))?;
                }
                responses.emit(Message::Done(ExecutionDone {
                    controller_id: request.controller_id,
                    node_id: request.node_id,
                    request_id: request.request_id,
                    session_id: request.session_id,
                    reason: "stop".into(),
                    generated_tokens: request.max_tokens,
                }))
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn local_controller_node_and_node_controller_use_the_task_queue() {
    runtime().block_on(async {
        let processor = Arc::new(AgentProcessor::new());
        processor
            .register_in_memory_adapter(
                "adapter".into(),
                "test".into(),
                "{}".into(),
                Arc::new(ScriptedAdapter),
            )
            .unwrap();
        let agent_id = processor.id().to_owned();
        let handler = Arc::new(AgentTaskHandler::new(
            processor,
            Arc::new(PeerMuxPool::default()),
        ));
        let queue = TaskQueue::start("agent-test", config(), handler.clone()).unwrap();
        let controller = participant(&agent_id, ParticipantRole::Controller, "controller");
        let node = participant(&agent_id, ParticipantRole::Node, "node");

        let mut controller_rx = register(&handler, "create-route", controller.clone());
        submit(
            &queue,
            "create-route",
            controller.clone(),
            node.clone(),
            Message::NodeCreate {
                controller_id: "controller".into(),
                operation_id: "create".into(),
                node_id: "node".into(),
                adapter_id: "adapter".into(),
                node_spec: r#"{"p4_max_inflight":4}"#.into(),
            },
        );
        assert!(matches!(
            receive(&mut controller_rx).await,
            Message::NodeCreated { .. }
        ));

        let mut controller_rx = register(&handler, "load-route", controller.clone());
        submit(
            &queue,
            "load-route",
            controller,
            node,
            Message::ModelLoad {
                controller_id: "controller".into(),
                node_id: "node".into(),
                operation_id: "load".into(),
                deployment_id: "deployment".into(),
                binding_id: "binding".into(),
                model: "model".into(),
                plan_revision: "1".into(),
                stage_plan: "{}".into(),
            },
        );
        assert!(matches!(
            receive(&mut controller_rx).await,
            Message::ModelBound { .. }
        ));
    });
}

#[test]
fn ingress_acceptance_and_stream_are_separate_ordered_response_tasks() {
    runtime().block_on(async {
        let processor = Arc::new(AgentProcessor::new());
        processor
            .register_in_memory_adapter(
                "adapter".into(),
                "test".into(),
                "{}".into(),
                Arc::new(ScriptedAdapter),
            )
            .unwrap();
        prepare_node(&processor);
        let agent_id = processor.id().to_owned();
        let handler = Arc::new(AgentTaskHandler::new(
            processor,
            Arc::new(PeerMuxPool::default()),
        ));
        let queue = TaskQueue::start("agent-test", config(), handler.clone()).unwrap();
        let external = participant("remote", ParticipantRole::External, "external");
        let controller = participant(&agent_id, ParticipantRole::Controller, "controller");
        let mut responses = register(&handler, "ingress-route", external.clone());
        submit(&queue, "ingress-route", external, controller, ingress());
        assert!(matches!(
            receive(&mut responses).await,
            Message::IngressAccepted { .. }
        ));
        for expected in 0..32 {
            let Message::Token(token) = receive(&mut responses).await else {
                panic!("response chain produced DONE before token {expected}");
            };
            assert_eq!(token.index, expected);
        }
        assert!(matches!(receive(&mut responses).await, Message::Done(_)));
    });
}

fn prepare_node(processor: &AgentProcessor) {
    let mut sink = crate::ResponseCollector::new();
    processor
        .handle(
            Message::NodeCreate {
                controller_id: "controller".into(),
                operation_id: "create".into(),
                node_id: "node".into(),
                adapter_id: "adapter".into(),
                node_spec: r#"{"p4_max_inflight":4}"#.into(),
            },
            &mut sink,
        )
        .unwrap();
    processor
        .handle(
            Message::ModelLoad {
                controller_id: "controller".into(),
                node_id: "node".into(),
                operation_id: "load".into(),
                deployment_id: "deployment".into(),
                binding_id: "binding".into(),
                model: "model".into(),
                plan_revision: "1".into(),
                stage_plan: "{}".into(),
            },
            &mut sink,
        )
        .unwrap();
}

fn ingress() -> Message {
    Message::IngressSubmit {
        controller_id: "controller".into(),
        ingress_id: "ingress".into(),
        request_id: "request".into(),
        session_id: "".into(),
        node_id: "node".into(),
        deployment_id: "deployment".into(),
        binding_id: "binding".into(),
        runtime_generation: 1,
        max_tokens: 32,
        temperature: 0.0,
        prompt: "test".into(),
        options: "{}".into(),
    }
}

fn register(
    handler: &AgentTaskHandler,
    route_id: &str,
    participant: Participant,
) -> mpsc::Receiver<RoutedMessage> {
    let (sender, receiver) = mpsc::channel(64);
    handler
        .register(route_id.into(), participant, sender)
        .unwrap();
    receiver
}

fn submit(
    queue: &TaskQueue,
    route_id: &str,
    source: Participant,
    target: Participant,
    message: Message,
) {
    let task = queue
        .routed_task(route_id, 0, source, target, message)
        .unwrap();
    assert!(
        task.is_local_bypass() || task.direction == p4_protocol::TaskDirection::ExternalController
    );
    queue.submit(task).unwrap();
}

async fn receive(receiver: &mut mpsc::Receiver<RoutedMessage>) -> Message {
    tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .unwrap()
        .unwrap()
        .message
}

fn participant(agent_id: &str, role: ParticipantRole, instance_id: &str) -> Participant {
    Participant {
        agent_id: agent_id.into(),
        role,
        instance_id: instance_id.into(),
    }
}

fn config() -> TaskQueueConfig {
    let lane = LaneConfig {
        items: 32,
        bytes: 1 << 20,
        workers: 4,
    };
    TaskQueueConfig {
        control: lane.clone(),
        prefill: lane.clone(),
        decode: lane.clone(),
        response: lane,
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
}
