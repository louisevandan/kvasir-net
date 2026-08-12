use super::*;
use p4_protocol::{Message, ParticipantRole};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc as std_mpsc;

struct InventoryHandler(std_mpsc::Sender<TaskEnvelope>);
struct RecordOnly(std_mpsc::Sender<TaskEnvelope>);

struct CompetingHandler {
    entered: AtomicUsize,
    release: std_mpsc::Sender<()>,
    released: Mutex<std_mpsc::Receiver<()>>,
}

#[test]
fn default_agent_prefill_lane_holds_at_least_one_thousand_tasks() {
    let config = TaskQueueConfig::for_parallelism(48);
    assert_eq!(config.prefill.items, 1024);
    assert_eq!(config.control.items, 4096);
    assert_eq!(config.decode.items, 4096);
    assert_eq!(config.response.items, 4096);
}

impl TaskHandler for InventoryHandler {
    fn handle(&self, task: TaskEnvelope, context: &TaskContext) -> TaskResult {
        self.0.send(task.clone()).unwrap();
        if matches!(task.message, Message::InventoryQuery { .. }) {
            context.response(
                &task,
                Message::HardwareReport {
                    agent_id: task.target.agent_id.clone(),
                    report_id: task.correlation_id.clone(),
                    snapshot: "{}".into(),
                },
            )?;
        }
        Ok(())
    }
}

impl TaskHandler for RecordOnly {
    fn handle(&self, task: TaskEnvelope, _context: &TaskContext) -> TaskResult {
        self.0.send(task).unwrap();
        Ok(())
    }
}

impl TaskHandler for CompetingHandler {
    fn handle(&self, _task: TaskEnvelope, _context: &TaskContext) -> TaskResult {
        if self.entered.fetch_add(1, Ordering::SeqCst) == 0 {
            self.released
                .lock()
                .map_err(|_| TaskQueueError::Invalid("test release lock poisoned".into()))?
                .recv_timeout(std::time::Duration::from_secs(1))
                .map_err(|_| {
                    TaskQueueError::Invalid("same-correlation task was worker-sharded".into())
                })?;
        } else {
            self.release
                .send(())
                .map_err(|_| TaskQueueError::Invalid("test release receiver closed".into()))?;
        }
        Ok(())
    }
}

#[test]
fn independent_same_correlation_tasks_compete_for_available_workers() {
    runtime().block_on(async {
        let (release, released) = std_mpsc::channel();
        let handler = Arc::new(CompetingHandler {
            entered: AtomicUsize::new(0),
            release,
            released: Mutex::new(released),
        });
        let queue = TaskQueue::start("compete", tiny_config(), handler).unwrap();
        for _ in 0..2 {
            let task = queue
                .task(
                    participant(ParticipantRole::External, "client"),
                    participant(ParticipantRole::Controller, "controller"),
                    Message::InventoryQuery {
                        controller_id: "controller".into(),
                        request_id: "same-correlation".into(),
                    },
                )
                .unwrap();
            queue.submit(task).unwrap();
        }
        wait_for_finished(&queue, 2).await;
        assert_eq!(queue.stats().completed, 2);
        assert_eq!(queue.stats().failed, 0);
    });
}

#[test]
fn request_and_response_are_two_independent_queue_tasks() {
    runtime().block_on(async {
        let (observed, receiver) = std_mpsc::channel();
        let queue =
            TaskQueue::start("test", tiny_config(), Arc::new(InventoryHandler(observed))).unwrap();
        let request = queue
            .task(
                participant(ParticipantRole::External, "client"),
                participant(ParticipantRole::Controller, "controller"),
                Message::InventoryQuery {
                    controller_id: "controller".into(),
                    request_id: "request-1".into(),
                },
            )
            .unwrap();
        queue.submit(request.clone()).unwrap();
        let first = receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let second = receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(first.task_id, request.task_id);
        assert_eq!(
            second.causation_id.as_deref(),
            Some(request.task_id.as_str())
        );
        assert_eq!(second.kind, p4_protocol::TaskKind::Response);
        assert!(second.is_local_bypass());
        wait_for_completed(&queue, 2).await;
        assert_eq!(queue.stats().completed, 2);
    });
}

#[test]
fn prefill_has_a_bounded_byte_budget() {
    runtime().block_on(async {
        let (observed, _receiver) = std_mpsc::channel();
        let mut config = tiny_config();
        config.prefill.bytes = 32;
        let queue = TaskQueue::start("test", config, Arc::new(InventoryHandler(observed))).unwrap();
        let request = p4_protocol::ExecutionRequest {
            controller_id: "controller".into(),
            node_id: "node".into(),
            deployment_id: "deployment".into(),
            binding_id: "binding".into(),
            runtime_generation: 1,
            request_id: "request".into(),
            session_id: "session".into(),
            phase: p4_protocol::Phase::Prefill,
            position: 0,
            max_tokens: 1,
            temperature: 0.0,
            prompt: "a meaningful prompt larger than the budget".into(),
            options: "{}".into(),
        };
        let task = queue
            .task(
                participant(ParticipantRole::Controller, "controller"),
                participant(ParticipantRole::Node, "node"),
                Message::Execute(request),
            )
            .unwrap();
        assert!(matches!(
            queue.submit(task),
            Err(TaskQueueError::Full(QueueClass::Prefill, _))
        ));
        assert_eq!(queue.stats().rejected, 1);
    });
}

#[test]
fn one_generic_worker_layer_accepts_all_four_p4_directions() {
    runtime().block_on(async {
        let (observed, receiver) = std_mpsc::channel();
        let queue =
            TaskQueue::start("four", tiny_config(), Arc::new(RecordOnly(observed))).unwrap();
        let controller = participant(ParticipantRole::Controller, "controller");
        let node_a = participant(ParticipantRole::Node, "node-a");
        let node_b = participant(ParticipantRole::Node, "node-b");
        let mut external = participant(ParticipantRole::External, "external");
        external.agent_id = "remote-agent".into();
        let messages = [
            queue
                .task(
                    external,
                    controller.clone(),
                    Message::InventoryQuery {
                        controller_id: "controller".into(),
                        request_id: "inventory".into(),
                    },
                )
                .unwrap(),
            queue
                .task(
                    controller.clone(),
                    node_a.clone(),
                    Message::HealthCheck {
                        controller_id: "controller".into(),
                        node_id: "node-a".into(),
                        request_id: "health".into(),
                    },
                )
                .unwrap(),
            queue
                .task(
                    node_a.clone(),
                    node_b,
                    Message::Execute(execute(p4_protocol::Phase::Decode)),
                )
                .unwrap(),
            queue
                .task(
                    node_a,
                    controller,
                    Message::Token(p4_protocol::ExecutionToken {
                        controller_id: "controller".into(),
                        node_id: "node-a".into(),
                        request_id: "token".into(),
                        session_id: "session".into(),
                        phase: p4_protocol::Phase::Decode,
                        position: 1,
                        index: 0,
                        text: "x".into(),
                    }),
                )
                .unwrap(),
        ];
        for task in messages {
            queue.submit(task).unwrap();
        }
        let mut directions = Vec::new();
        for _ in 0..4 {
            directions.push(
                receiver
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .unwrap()
                    .direction,
            );
        }
        directions.sort_by_key(|value| format!("{value:?}"));
        assert_eq!(directions.len(), 4);
        assert!(directions.contains(&p4_protocol::TaskDirection::ExternalController));
        assert!(directions.contains(&p4_protocol::TaskDirection::ControllerNode));
        assert!(directions.contains(&p4_protocol::TaskDirection::NodeNode));
        assert!(directions.contains(&p4_protocol::TaskDirection::NodeController));
    });
}

fn execute(phase: p4_protocol::Phase) -> p4_protocol::ExecutionRequest {
    p4_protocol::ExecutionRequest {
        controller_id: "controller".into(),
        node_id: "node-b".into(),
        deployment_id: "deployment".into(),
        binding_id: "binding".into(),
        runtime_generation: 1,
        request_id: "execute".into(),
        session_id: "session".into(),
        phase,
        position: 1,
        max_tokens: 1,
        temperature: 0.0,
        prompt: "state".into(),
        options: "{}".into(),
    }
}

fn participant(role: ParticipantRole, id: &str) -> Participant {
    Participant {
        agent_id: "agent-a".into(),
        role,
        instance_id: id.into(),
    }
}

fn tiny_config() -> TaskQueueConfig {
    let lane = LaneConfig {
        items: 8,
        bytes: 1 << 20,
        workers: 2,
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

async fn wait_for_completed(queue: &TaskQueue, expected: u64) {
    for _ in 0..100 {
        if queue.stats().completed >= expected {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("queue did not complete {expected} tasks");
}

async fn wait_for_finished(queue: &TaskQueue, expected: u64) {
    for _ in 0..200 {
        let stats = queue.stats();
        if stats.completed + stats.failed >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("queue did not finish {expected} tasks");
}
