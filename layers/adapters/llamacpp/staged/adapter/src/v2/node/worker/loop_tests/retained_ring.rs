//! Actual retained broker/node/llama adapter/Worker loop. NativeStage below
//! substitutes only native computation and owns an independent KV oracle.
use super::*;
use crate::v2::node::RetainedLlamaNodeAdapter;
use p4_adapter::node_adapter::{OwnedPoll, RetainedNodeAdapter, completion_mailbox_with_limits};
use p4_agent_core::event_broker::{DispatchError, RetainedEventBroker};
use p4_agent_core::event_node::{RetainedEventNode, RetainedEventNodeFailure};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll as TaskPoll;

type Run = Pin<Box<dyn Future<Output = Result<(), RetainedEventNodeFailure>> + Send>>;
struct Ring {
    broker: Arc<RetainedEventBroker>,
    nodes: Vec<Arc<RetainedLlamaNodeAdapter>>,
    runs: Vec<Run>,
    stores: Vec<Arc<CompletionMailbox>>,
    native: Vec<Arc<Mutex<NativeTrace>>>,
    outer: Arc<CompletionMailbox>,
    received: Vec<Event>,
}
impl Ring {
    fn new() -> Self {
        let queue = || completion_mailbox_with_limits(1, 16, 64 << 20).unwrap();
        let (agent_tx, agent) = queue();
        let (outer_tx, outer) = queue();
        let (outbound_tx, outbound) = queue();
        let broker = Arc::new(RetainedEventBroker::new(
            address(),
            agent_tx,
            outer_tx,
            outbound_tx,
            512,
        ));
        let mut nodes = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        let mut stores = vec![agent, outbound, outer.clone()];
        let mut natives = Vec::new();
        for index in 0..2 {
            let (sender, receiver) = mpsc::sync_channel(1);
            let (publisher, mailbox) = queue();
            let snapshot = Arc::new(Mutex::new("empty".into()));
            let shutdown = Arc::new(AtomicBool::new(false));
            let native = Arc::new(Mutex::new(NativeTrace::default()));
            let mut worker = Worker::new(
                endpoint(index),
                receiver,
                publisher,
                snapshot.clone(),
                shutdown.clone(),
            )
            .with_stage_for_test(Box::new(NativeStage {
                role: if index == 0 {
                    NodeRole::First
                } else {
                    NodeRole::Last
                },
                next_execution: 1,
                trace: native.clone(),
                chain: None,
                speculative: None,
                issue_fault: None,
            }))
            .unwrap();
            // This post-LOAD fixture owns a 64 MiB completion store. Keep the
            // injected profile identical to that store instead of inheriting
            // the broad 512 MiB unit-fixture ceiling; a real LOAD would reject
            // that mismatch before any session or native work.
            let profile = worker
                .state
                .resource_profile
                .as_mut()
                .expect("injected native installs a resource profile");
            profile.max_requests = 16;
            profile.max_completion_payload_bytes = 64 << 20;
            profile.max_completion_retained_bytes = 64 << 20;
            profile.max_edge_retained_bytes = 64 << 20;
            // Documented post-LOAD fixture only. All sessions, input, decode,
            // physical results and release acknowledgements use the real actors.
            worker.state.load_generation = 1;
            worker.state.physical_receives =
                crate::v2::node::physical_receive::PhysicalReceiveLedger::new(1).unwrap();
            worker.state.batch_capacity = BATCH_CAPACITY;
            worker.state.physical_capacity = PHYSICAL_CAPACITY;
            worker.state.context_size = 256;
            worker.state.sequence_capacity = SEQUENCE_CAPACITY;
            worker.state.free_sequences = (0..SEQUENCE_CAPACITY).collect();
            worker.state.max_atomic_sequences = 1;
            worker.state.equal_sequence_ubatch = false;
            worker.state.atomic_batch_exclusive = false;
            worker.state.min_batch_rows = 0;
            worker.state.max_open_batches = 2;
            worker.state.max_issue_rows = 0;
            worker.state.prefill_fragments = 1;
            let adapter = Arc::new(RetainedLlamaNodeAdapter::spawn_worker(
                sender,
                mailbox.clone(),
                snapshot,
                shutdown,
                worker,
            ));
            let (route_tx, route_rx) = queue();
            broker
                .register_node(format!("loop-{index}"), 1, route_tx)
                .unwrap();
            runs.push(Box::pin(
                RetainedEventNode::new(adapter.clone(), route_rx.clone(), broker.clone()).run(),
            ));
            stores.extend([mailbox, route_rx]);
            nodes.push(adapter);
            natives.push(native);
        }
        Self {
            broker,
            nodes,
            runs,
            stores,
            native: natives,
            outer,
            received: Vec::new(),
        }
    }
    async fn step(&mut self) {
        for run in &mut self.runs {
            let result = poll_fn(|cx| TaskPoll::Ready(run.as_mut().poll(cx))).await;
            assert!(result.is_pending(), "owned node stopped: {result:?}");
        }
        while let OwnedPoll::Event(completion) = self.outer.try_take_owned() {
            assert_eq!(event_wire(completion.event().clone()), *completion.event());
            assert_ne!(
                completion.event().envelope.payload_content_type,
                ERROR_CONTENT_TYPE,
                "{:?}",
                completion.event()
            );
            self.received.push(completion.event().clone());
            completion.retire(); // Bounded test recorder copy, not a product raw bridge.
            assert!(self.received.len() < 1024);
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    async fn until(&mut self, label: &str, predicate: impl Fn(&Self) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !predicate(self) {
            assert!(
                Instant::now() < end,
                "{label}; snapshots={:?}",
                self.nodes.iter().map(|a| a.snapshot()).collect::<Vec<_>>()
            );
            self.step().await;
        }
    }
    async fn submit(&mut self, event: Event) {
        let mut pending = Some(event);
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match self.broker.dispatch_ingress(pending.take().unwrap()) {
                Ok(_) => return,
                Err(failure) if matches!(failure.error, DispatchError::Full(_)) => {
                    pending = Some(*failure.event)
                }
                Err(failure) => panic!("{failure:?}"),
            }
            assert!(Instant::now() < end, "owned ingress remained Full");
            self.step().await;
        }
    }
}
impl Drop for Ring {
    fn drop(&mut self) {
        // Polling futures own adapter references. Release them before joining
        // worker threads; their inner Drop signals publication cancellation.
        self.runs.clear();
        self.nodes.clear();
    }
}

#[test]
fn owned_worker_two_stage_actual_ring_completes_prefill_decode_release_and_unload() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            for golden in [true, false] {
                let mut ring = Ring::new();
                for index in 0..2 {
                    let session = SessionCommand {
                        load_generation: 1,
                        session_id: "loop-session".into(),
                        stages: (0..2).map(node_address).collect(),
                        stage_index: index,
                    };
                    ring.submit(event_wire(event(
                        index,
                        &format!("owned-session-{index}"),
                        SESSION_CONTENT_TYPE,
                        serde_json::to_vec(&session).unwrap(),
                    )))
                    .await;
                }
                ring.until("sessions ready", |r| {
                    r.received
                        .iter()
                        .filter(|e| e.envelope.payload_content_type == SESSION_READY_CONTENT_TYPE)
                        .count()
                        == 2
                })
                .await;
                let commands: Vec<_> = if golden {
                    vec![request("one", 7, 5)]
                } else {
                    (0..4)
                        .map(|i| request(&format!("owned-request-{i}"), 7 + i * 3, 5))
                        .collect()
                };
                let submissions: Vec<_> = commands
                    .iter()
                    .enumerate()
                    .map(|(i, c)| event_wire(submission_event(c, i as u64 + 1, default_route())))
                    .collect();
                for event in &submissions {
                    ring.submit(event.clone()).await;
                }
                ring.until("all generated tokens and stage releases", |r| {
                    r.received
                        .iter()
                        .filter(|e| e.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
                        .count()
                        == commands.len() * 5
                        && r.received
                            .iter()
                            .filter(|e| {
                                e.envelope.payload_content_type == RELEASE_RECEIPT_CONTENT_TYPE
                            })
                            .map(|e| {
                                serde_json::from_slice::<ReleaseReceipt>(&e.payload)
                                    .unwrap()
                                    .members
                                    .len()
                            })
                            .sum::<usize>()
                            == commands.len()
                        && r.native
                            .iter()
                            .all(|n| n.lock().unwrap().releases.len() == commands.len())
                })
                .await;
                let outputs: Vec<_> = ring
                    .received
                    .iter()
                    .filter(|e| e.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
                    .cloned()
                    .collect();
                for command in &commands {
                    let values: Vec<OutcomePayload> = outputs
                        .iter()
                        .map(|e| serde_json::from_slice(&e.payload).unwrap())
                        .filter(|o: &OutcomePayload| o.request_id == command.request_id)
                        .collect();
                    assert_eq!(values.len(), 5);
                    for (i, value) in values.iter().enumerate() {
                        assert_eq!(value.token, 1000 + i as i32);
                        assert_eq!(value.position as usize, command.tokens.len() + i);
                        assert_eq!(value.text, format!("token-{} ", 1000 + i));
                        assert_eq!(value.stop.as_deref(), (i == 4).then_some("length"));
                    }
                }
                for native in &ring.native {
                    let native = native.lock().unwrap();
                    assert!(native.live.is_empty());
                    assert!(native.releases.values().all(|&n| n == 1));
                    for command in &commands {
                        let key = request_key("loop-session", &command.request_id);
                        let writes = native
                            .written
                            .iter()
                            .find(|((_, k, _), _)| k == &key)
                            .unwrap()
                            .1;
                        let expected: Vec<_> = command
                            .tokens
                            .iter()
                            .copied()
                            .chain(1000..1004)
                            .enumerate()
                            .map(|(p, t)| (p as u32, t))
                            .collect();
                        assert_eq!(
                            *writes, expected,
                            "every input and sampled decode position reaches every stage once"
                        );
                    }
                }
                ring.until("input/completion stores retire", |r| {
                    r.stores
                        .iter()
                        .all(|s| s.storage_snapshot().retained_count == 0)
                })
                .await;
                for index in 0..2 {
                    let command = UnloadCommand { load_generation: 1 };
                    ring.submit(event(
                        index,
                        &format!("owned-unload-{index}"),
                        UNLOAD_CONTENT_TYPE,
                        serde_json::to_vec(&command).unwrap(),
                    ))
                    .await;
                }
                ring.until("idle unload replies", |r| {
                    r.received
                        .iter()
                        .filter(|e| e.envelope.payload_content_type == UNLOADED_CONTENT_TYPE)
                        .count()
                        == 2
                })
                .await;
                ring.until("all final claims retire", |r| {
                    r.stores
                        .iter()
                        .all(|s| s.storage_snapshot().retained_bytes == 0)
                })
                .await;
                if golden {
                    // Exact pre-existing reviewed wire/OUTER contract. Do not mint a
                    // replacement golden from the new implementation's own output.
                    output_contract::assert_live_matches(
                        "ordinary-2",
                        &submissions,
                        &outputs,
                        &ring.received,
                    );
                    output_contract::assert_live_prefill_counts("ordinary-2", &ring.received);
                } else {
                    let workload: Vec<_> = commands
                        .iter()
                        .map(|c| (c.request_id.as_str(), c.tokens.len()))
                        .collect();
                    output_contract::assert_live_prefill_workload(
                        &endpoint(0),
                        &workload,
                        &ring.received,
                    );
                }
                for store in &ring.stores {
                    assert_eq!(store.storage_snapshot().reserved_queue_slots, 0);
                }
            }
        });
}
