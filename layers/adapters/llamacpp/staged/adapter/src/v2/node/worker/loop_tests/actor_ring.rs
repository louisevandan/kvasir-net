//! Actual EventBroker -> EventNode -> LlamaNodeAdapter -> Worker::run cycle.
//! Only native computation and its finite latency are fake. No queue contents,
//! pending releases, capsules, or completion Events are manufactured. This is
//! an actor-level finite workload, not a network/GPU/RSS or all-rings proof.
use super::*;
use crate::v2::node::LlamaNodeAdapter;
use p4_adapter::node_adapter::{NodeAdapter, OfferError};
use p4_agent_core::event_broker::{
    DispatchError, DispatchFailure, DispatchOutcome, EventBroker, EventReceiver, EventSender,
    bounded_queue,
};
use p4_agent_core::event_node::{EventNode, EventNodeFailure};
use p4_protocol::event::Envelope;
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::Condvar;
use std::task::{Context, Poll as TaskPoll};

#[derive(Default)]
struct GateState {
    entered: bool,
    open: bool,
    timed_out: bool,
}

#[derive(Default)]
struct NativeGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl NativeGate {
    fn enter(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        state.entered = true;
        self.changed.notify_all();
        let (mut state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |state| !state.open)
            .unwrap();
        if timeout.timed_out() && !state.open {
            state.timed_out = true;
            return Err("finite actor-ring native gate timed out".into());
        }
        Ok(())
    }

    fn entered(&self) -> bool {
        self.state.lock().unwrap().entered
    }

    fn assert_healthy(&self) {
        assert!(
            !self.state.lock().unwrap().timed_out,
            "actor-ring fixture native gate expired; not a liveness counterexample"
        );
    }

    fn open(&self) {
        self.state.lock().unwrap().open = true;
        self.changed.notify_all();
    }
}

struct GatedNative {
    native: NativeStage,
    gate: Arc<NativeGate>,
    gated: bool,
}

impl ServerControl for GatedNative {
    fn start(&mut self) -> Result<(), String> {
        self.native.start()
    }

    fn wait_ready(&mut self, deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        self.native.wait_ready(deadline)
    }

    fn request(&mut self, frame: Frame) -> Result<Frame, String> {
        // Delay a real native request before delegating unchanged bytes. Never
        // hold NativeTrace/state/adapter locks while waiting on the test gate.
        let pause = !self.gated
            && match self.native.role {
                NodeRole::First => {
                    frame.header.operation == Operation::LogicalBatch
                        && self.native.trace.lock().unwrap().logical_calls == 1
                }
                NodeRole::Last => frame.header.operation == Operation::PhysicalRelease,
                NodeRole::Middle => false,
            };
        if pause {
            self.gated = true;
            self.gate.enter()?;
        }
        self.native.request(frame)
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.native.shutdown()
    }
}

#[derive(Default)]
struct AdapterTrace {
    accepted: Vec<Event>,
    refusals: BTreeMap<String, usize>,
    taken: Vec<Event>,
    node_taken: Vec<Event>,
}

struct ObservedAdapter {
    inner: Arc<LlamaNodeAdapter>,
    trace: Mutex<AdapterTrace>,
}

impl ObservedAdapter {
    fn observe_take(&self, value: Poll, by_event_node: bool) -> Poll {
        if let Poll::Event(event) = &value {
            let mut trace = self.trace.lock().unwrap();
            assert!(trace.taken.len() < 512, "bounded actor-ring capture");
            trace.taken.push(event.clone());
            if by_event_node {
                trace.node_taken.push(event.clone());
            }
        }
        value
    }
}

impl NodeAdapter for ObservedAdapter {
    fn kind(&self) -> &str {
        self.inner.kind()
    }

    fn try_offer(&self, event: Event) -> Result<(), OfferError> {
        let original = event.clone();
        let result = self.inner.try_offer(event);
        let mut trace = self.trace.lock().unwrap();
        match &result {
            Ok(()) => {
                assert!(trace.accepted.len() < 512, "bounded actor-ring capture");
                trace.accepted.push(original);
            }
            Err(OfferError::Full(returned)) => {
                assert_eq!(*returned, original, "Full returns unchanged ownership");
                *trace
                    .refusals
                    .entry(original.envelope.event_id)
                    .or_default() += 1;
            }
            Err(OfferError::Closed(returned)) => {
                assert_eq!(*returned, original, "Closed returns unchanged ownership");
            }
        }
        result
    }

    fn try_take(&self) -> Poll {
        self.observe_take(self.inner.try_take(), false)
    }

    fn poll_take(&self, cx: &mut Context<'_>) -> TaskPoll<Poll> {
        self.inner
            .poll_take(cx)
            .map(|value| self.observe_take(value, true))
    }

    fn peek_completion(&self) -> Option<Envelope> {
        self.inner.peek_completion()
    }

    fn try_take_completion_matching(&self, envelope: &Envelope) -> Poll {
        self.observe_take(self.inner.try_take_completion_matching(envelope), true)
    }

    fn snapshot(&self) -> String {
        self.inner.snapshot()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StateView {
    point: &'static str,
    pending: BTreeMap<String, (u32, u64, u64)>,
    free: Vec<u32>,
    requests: Vec<String>,
}

type NodeFuture = Pin<Box<dyn Future<Output = Result<(), EventNodeFailure>> + Send>>;
type NativeView = (
    usize,
    usize,
    usize,
    BTreeMap<NativeKey, Vec<i32>>,
    BTreeMap<NativeKey, Vec<(u32, i32)>>,
    BTreeMap<NativeKey, usize>,
);

struct Ring {
    runs: Vec<NodeFuture>,
    adapters: Vec<Arc<ObservedAdapter>>,
    native: Vec<Arc<Mutex<NativeTrace>>>,
    gates: Vec<Arc<NativeGate>>,
    states: Vec<Arc<Mutex<Vec<StateView>>>>,
    broker: Arc<EventBroker>,
    senders: Vec<EventSender>,
    outer: EventReceiver,
    _agent: EventReceiver,
    _outbound: EventReceiver,
    received: Vec<Event>,
    submissions: Vec<Event>,
    outer_polls: usize,
    head_frozen_full: bool,
}

impl Ring {
    fn new(capacity: usize) -> Self {
        let (agent_tx, agent) = bounded_queue(capacity);
        let (outer_tx, outer) = bounded_queue(capacity);
        let (outbound_tx, outbound) = bounded_queue(capacity);
        let broker = Arc::new(EventBroker::new(
            address(),
            agent_tx,
            outer_tx,
            outbound_tx,
            512,
        ));
        let mut runs: Vec<NodeFuture> = Vec::new();
        let mut adapters = Vec::new();
        let mut natives = Vec::new();
        let mut gates = Vec::new();
        let mut states = Vec::new();
        let mut senders = Vec::new();
        for index in 0..2 {
            let (input, receiver) = mpsc::sync_channel(capacity);
            let (publisher, mailbox) = completion_mailbox(capacity);
            let snapshot = Arc::new(Mutex::new("empty".into()));
            let shutdown = Arc::new(AtomicBool::new(false));
            let native = Arc::new(Mutex::new(NativeTrace::default()));
            let gate = Arc::new(NativeGate::default());
            let views = Arc::new(Mutex::new(Vec::new()));
            let record = Arc::clone(&views);
            let mut worker = Worker::new(
                endpoint(index),
                receiver,
                publisher,
                Arc::clone(&snapshot),
                Arc::clone(&shutdown),
            )
            .with_stage_for_test(Box::new(GatedNative {
                native: NativeStage {
                    role: if index == 0 {
                        NodeRole::First
                    } else {
                        NodeRole::Last
                    },
                    next_execution: 1,
                    trace: Arc::clone(&native),
                    chain: None,
                    speculative: None,
                    issue_fault: None,
                },
                gate: Arc::clone(&gate),
                gated: false,
            }))
            .unwrap();
            // The only seeded state is the documented post-LOAD fixture. All
            // SESSION, request, native, flight and release state is reached via
            // broker ingress and Worker::run, never a transition test helper.
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
            worker.state.max_open_batches = 1;
            worker.state.max_issue_rows = 0;
            worker.state.prefill_fragments = 1;
            worker.issue_observer = Some(Arc::new(move |point, state| {
                let mut views = record.lock().unwrap();
                assert!(views.len() < 512, "bounded actor-ring state observation");
                views.push(StateView {
                    point,
                    pending: state
                        .pending_releases
                        .iter()
                        .map(|(key, pending)| {
                            (
                                key.clone(),
                                (
                                    pending.sequence.id,
                                    pending.sequence.incarnation,
                                    pending.sequence.operation_id,
                                ),
                            )
                        })
                        .collect(),
                    free: state.free_sequences.iter().copied().collect(),
                    requests: state.requests.keys().cloned().collect(),
                });
            }));
            let thread = std::thread::spawn(move || worker.run());
            let adapter = Arc::new(ObservedAdapter {
                inner: Arc::new(LlamaNodeAdapter {
                    sender: Some(input),
                    mailbox,
                    snapshot,
                    shutting_down: shutdown,
                    worker: Mutex::new(Some(thread)),
                }),
                trace: Mutex::new(AdapterTrace::default()),
            });
            let (tx, rx) = bounded_queue(capacity);
            broker
                .register_node(format!("loop-{index}"), 1, tx.clone())
                .unwrap();
            runs.push(Box::pin(
                EventNode::new(adapter.clone(), rx, Arc::clone(&broker)).run(),
            ));
            adapters.push(adapter);
            natives.push(native);
            gates.push(gate);
            states.push(views);
            senders.push(tx);
        }
        Self {
            runs,
            adapters,
            native: natives,
            gates,
            states,
            broker,
            senders,
            outer,
            _agent: agent,
            _outbound: outbound,
            received: Vec::new(),
            submissions: Vec::new(),
            outer_polls: 0,
            head_frozen_full: false,
        }
    }

    fn drain_outer(&mut self) {
        self.outer_polls += 1;
        while let Ok(event) = self.outer.try_recv() {
            assert_eq!(event_wire(event.clone()), event);
            assert_ne!(
                event.envelope.payload_content_type,
                ERROR_CONTENT_TYPE,
                "unexpected actor error: {}",
                String::from_utf8_lossy(&event.payload)
            );
            assert!(self.received.len() < 512);
            self.received.push(event);
        }
    }

    async fn step(&mut self, nodes: &[usize]) {
        if self.head_frozen_full {
            assert!(!nodes.contains(&0), "the held-R witness must not poll head");
            assert_eq!(self.senders[0].capacity(), 0);
        }
        for gate in &self.gates {
            gate.assert_healthy();
        }
        self.drain_outer();
        for index in nodes {
            let result = poll_fn(|cx| TaskPoll::Ready(self.runs[*index].as_mut().poll(cx))).await;
            assert!(result.is_pending(), "actual EventNode stopped: {result:?}");
            self.drain_outer();
        }
        // Native workers are real OS threads; a bounded scheduler opportunity
        // is needed, not a busy-spin or a fake transition driven by the test.
        tokio::time::sleep(Duration::from_millis(1)).await;
        for gate in &self.gates {
            gate.assert_healthy();
        }
        if self.head_frozen_full {
            assert_eq!(
                self.senders[0].capacity(),
                0,
                "R's actual destination never acquired space during prevention"
            );
        }
    }

    async fn until(&mut self, label: &str, nodes: &[usize], predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !predicate(self) {
            assert!(
                Instant::now() < deadline,
                "actor-ring setup/finish: {label}"
            );
            self.step(nodes).await;
        }
    }

    async fn submit(&mut self, event: Event, nodes: &[usize]) {
        let original = event_wire(event);
        let mut returned = original.clone();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.broker.dispatch(returned) {
                Ok(DispatchOutcome::Enqueued(_)) => {
                    self.submissions.push(original);
                    return;
                }
                Err(DispatchFailure {
                    error: DispatchError::Full(_),
                    event,
                }) => {
                    assert_eq!(*event, original);
                    returned = *event;
                }
                result => panic!("normal broker ingress rejected: {result:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "normal ingress did not find capacity"
            );
            self.step(nodes).await;
        }
    }

    fn accepted(&self, index: usize, event: &Event) -> bool {
        self.adapters[index]
            .trace
            .lock()
            .unwrap()
            .accepted
            .iter()
            .any(|seen| seen == event)
    }

    fn refused(&self, index: usize, event: &Event) -> bool {
        self.adapters[index]
            .trace
            .lock()
            .unwrap()
            .refusals
            .contains_key(&event.envelope.event_id)
    }

    fn taken_type(&self, index: usize, content: &str, correlation: &str) -> bool {
        self.adapters[index]
            .trace
            .lock()
            .unwrap()
            .taken
            .iter()
            .any(|event| {
                event.envelope.payload_content_type == content
                    && event.envelope.correlation_id == correlation
            })
    }

    fn authentic_held_release(&self, commands: &[InferenceCommand]) -> Event {
        assert!(self.head_frozen_full);
        assert_eq!(self.senders[0].capacity(), 0);
        let head = self.adapters[0].trace.lock().unwrap();
        let tail = self.adapters[1].trace.lock().unwrap();
        assert_eq!(head.taken, head.node_taken, "no external recovery yet");
        assert_eq!(tail.taken, tail.node_taken, "no external recovery yet");
        assert_eq!(
            head.node_taken
                .last()
                .unwrap()
                .envelope
                .payload_content_type,
            PHYSICAL_BATCH_CONTENT_TYPE
        );
        let releases: Vec<_> = tail
            .node_taken
            .iter()
            .filter(|event| {
                event.envelope.payload_content_type == RELEASED_CONTENT_TYPE
                    && event.envelope.correlation_id == "R"
            })
            .collect();
        assert_eq!(releases.len(), 1);
        let event = releases[0];
        assert_eq!(event.envelope.source, endpoint(1));
        assert_eq!(event.envelope.target, endpoint(0));
        let release: ReleaseCommand = serde_json::from_slice(&event.payload).unwrap();
        assert_eq!(release.sequences.len(), 1);
        assert_eq!(release.sequences[0].key, request_key("loop-session", "R"));
        let states = self.states[0].lock().unwrap();
        let held = states
            .iter()
            .rev()
            .find(|view| view.point == "blocked_non_ack_held")
            .unwrap();
        let pending = held.pending[&request_key("loop-session", "R")];
        assert_eq!(
            pending,
            (
                release.sequences[0].id,
                release.sequences[0].incarnation,
                release.sequences[0].operation_id
            ),
            "genuine ACK matches exact head-owned release identity"
        );
        assert_eq!(release.load_generation, 1);
        assert_eq!(release.session_id, "loop-session");
        assert!(
            !held.free.contains(&pending.0),
            "R's slot remains pending release"
        );
        assert!(held.requests.contains(&request_key("loop-session", "Q")));
        for command in &commands[2..] {
            assert!(
                !held
                    .requests
                    .contains(&request_key("loop-session", &command.request_id)),
                "held/queued PREFILL must not already have entered request state"
            );
        }
        event.clone()
    }

    fn normal_c1_passed_held_release(&self, control: &Event, release: &Event) -> bool {
        assert!(self.head_frozen_full);
        assert_eq!(self.senders[0].capacity(), 0);
        let trace = self.adapters[1].trace.lock().unwrap();
        assert_eq!(
            trace.taken, trace.node_taken,
            "only actual EventNode dequeue is eligible"
        );
        let Some(index) = trace.node_taken.iter().position(|event| {
            event.envelope.payload_content_type == SESSION_READY_CONTENT_TYPE
                && event.envelope.correlation_id == control.envelope.correlation_id
        }) else {
            return false;
        };
        let response = &trace.node_taken[index];
        assert!(
            trace.node_taken[..index]
                .iter()
                .any(|event| event == release)
        );
        assert_eq!(
            response.envelope.causation_id.as_deref(),
            Some(control.envelope.event_id.as_str())
        );
        assert_eq!(response.envelope.source, endpoint(1));
        assert_eq!(response.envelope.target, control.envelope.source);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&response.payload).unwrap(),
            serde_json::json!({"session_id":"loop-session", "state":"ready", "load_generation":1})
        );
        self.received.iter().any(|event| event == response)
    }

    fn complete(&self, commands: &[InferenceCommand]) -> bool {
        let outputs = self
            .received
            .iter()
            .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
            .count();
        let released = self
            .received
            .iter()
            .filter(|event| {
                event.envelope.payload_content_type == crate::v2::RELEASE_RECEIPT_CONTENT_TYPE
            })
            .map(|event| {
                serde_json::from_slice::<crate::v2::ReleaseReceipt>(&event.payload)
                    .unwrap()
                    .members
                    .len()
            })
            .sum::<usize>();
        outputs == commands.len()
            && released == commands.len()
            && self
                .native
                .iter()
                .all(|native| native.lock().unwrap().releases.len() == commands.len())
    }

    fn native_snapshot(&self) -> Vec<NativeView> {
        self.native
            .iter()
            .map(|native| {
                let native = native.lock().unwrap();
                (
                    native.logical_calls,
                    native.physical_calls,
                    native.sampler_calls,
                    native.live.clone(),
                    native.written.clone(),
                    native.releases.clone(),
                )
            })
            .collect()
    }

    fn assert_finished(&self, commands: &[InferenceCommand]) {
        for gate in &self.gates {
            gate.assert_healthy();
        }
        assert!(self.complete(commands));
        for command in commands {
            let matching: Vec<_> = self
                .received
                .iter()
                .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
                .filter(|event| {
                    serde_json::from_slice::<OutcomePayload>(&event.payload)
                        .unwrap()
                        .request_id
                        == command.request_id
                })
                .collect();
            assert_eq!(matching.len(), 1, "one normal token per request");
            assert_eq!(
                matching[0].envelope.source,
                endpoint(0),
                "head-approved output only"
            );
            let output: OutcomePayload = serde_json::from_slice(&matching[0].payload).unwrap();
            assert_eq!(output.token, 1000);
            assert_eq!(output.position, 1);
            assert_eq!(output.text, "token-1000 ");
            assert_eq!(output.stop.as_deref(), Some("length"));
        }
        for native in &self.native {
            let native = native.lock().unwrap();
            assert!(native.live.is_empty());
            assert_eq!(native.written.len(), commands.len());
            assert!(
                native
                    .written
                    .values()
                    .all(|writes| writes == &vec![(0, 10)])
            );
            assert!(native.releases.values().all(|count| *count == 1));
        }
        let mut emitted = std::collections::BTreeSet::new();
        for adapter in &self.adapters {
            for event in &adapter.trace.lock().unwrap().taken {
                assert_eq!(event_wire(event.clone()), *event);
                assert!(
                    emitted.insert(event.envelope.event_id.clone()),
                    "duplicate publication"
                );
            }
        }
        let ready = self
            .received
            .iter()
            .filter(|event| event.envelope.payload_content_type == SESSION_READY_CONTENT_TYPE)
            .count();
        assert_eq!(
            ready, 8,
            "both initial sessions and six valid retransmissions"
        );
        assert_eq!(
            self.submissions.len(),
            14,
            "six requests plus eight sessions entered the broker"
        );
        let submitted: BTreeMap<_, _> = self
            .submissions
            .iter()
            .map(|event| (event.envelope.event_id.clone(), event))
            .collect();
        assert_eq!(
            submitted.len(),
            self.submissions.len(),
            "input event identities are unique"
        );
        let mut accepted_submissions = BTreeMap::new();
        for adapter in &self.adapters {
            for event in &adapter.trace.lock().unwrap().accepted {
                if let Some(original) = submitted.get(&event.envelope.event_id) {
                    assert_eq!(
                        event, *original,
                        "actual adapter ingress matches whole submitted event"
                    );
                    assert!(
                        accepted_submissions
                            .insert(event.envelope.event_id.clone(), event.clone())
                            .is_none(),
                        "a submission was accepted by the actor more than once"
                    );
                }
            }
        }
        assert_eq!(
            accepted_submissions.len(),
            submitted.len(),
            "every broker input reached its real actor exactly once"
        );
        assert!(self.outer_polls > 0);
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        // Every panic path opens native gates BEFORE any adapter joins. Input
        // senders have no fixture/native clones; adapter Drop can close recv.
        for gate in &self.gates {
            gate.open();
        }
        for adapter in &self.adapters {
            adapter.inner.shutting_down.store(true, Ordering::SeqCst);
        }
        self.runs.clear();
        self.adapters.clear();
    }
}

fn session(index: usize, name: &str) -> Event {
    let command = SessionCommand {
        load_generation: 1,
        session_id: "loop-session".into(),
        stages: (0..2).map(node_address).collect(),
        stage_index: index,
    };
    event_wire(event(
        index,
        name,
        SESSION_CONTENT_TYPE,
        serde_json::to_vec(&command).unwrap(),
    ))
}

async fn scenario(capacity: usize) {
    let commands: Vec<_> = ["R", "Q", "H1", "H2", "H3", "H4"]
        .iter()
        .map(|name| request(name, 1, 1))
        .collect();
    let inputs: Vec<_> = commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            event_wire(submission_event(command, index as u64 + 1, default_route()))
        })
        .collect();
    let controls: Vec<_> = (1..=6)
        .map(|index| session(1, &format!("C{index}")))
        .collect();
    let mut ring = Ring::new(capacity);
    for index in 0..2 {
        ring.submit(session(index, &format!("initial-session-{index}")), &[0, 1])
            .await;
    }
    ring.until("initial SESSION_READY", &[0, 1], |ring| {
        ring.received
            .iter()
            .filter(|event| event.envelope.payload_content_type == SESSION_READY_CONTENT_TYPE)
            .count()
            == 2
    })
    .await;
    ring.submit(inputs[0].clone(), &[0, 1]).await;
    ring.until("tail entered genuine R release", &[0, 1], |ring| {
        ring.gates[1].entered()
    })
    .await;
    ring.submit(inputs[1].clone(), &[0, 1]).await;
    ring.until("head entered genuine Q issue", &[0, 1], |ring| {
        ring.gates[0].entered()
    })
    .await;

    // Native latency, not invented mailbox events, lets normal ingress fill
    // each three-level path. Only selected EventNode futures are polled.
    for input in &inputs[2..5] {
        ring.submit(input.clone(), &[0]).await;
    }
    for control in &controls[..3] {
        ring.submit(control.clone(), &[1]).await;
    }
    if capacity == 1 {
        ring.until("H1 accepted, H2 held, H3 queued", &[0], |ring| {
            ring.accepted(0, &inputs[2])
                && ring.refused(0, &inputs[3])
                && ring.senders[0].capacity() == 0
        })
        .await;
        ring.until("C1 accepted, C2 held, C3 queued", &[1], |ring| {
            ring.accepted(1, &controls[0])
                && ring.refused(1, &controls[1])
                && ring.senders[1].capacity() == 0
        })
        .await;
    }

    ring.gates[0].open();
    ring.until(
        "Q physical really left the completion mailbox",
        &[0],
        |ring| ring.taken_type(0, PHYSICAL_BATCH_CONTENT_TYPE, "Q"),
    )
    .await;
    if capacity == 1 {
        ring.until("head held H1 and input/actor retain H2/H3", &[0], |ring| {
            ring.adapters[0].snapshot() == "completion_queue_full:waiting"
                && ring.accepted(0, &inputs[3])
                && ring.refused(0, &inputs[4])
                && ring.states[0]
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|view| view.point == "blocked_non_ack_held")
        })
        .await;
    }
    ring.submit(inputs[5].clone(), &[0]).await;
    if capacity == 1 {
        assert_eq!(ring.senders[0].capacity(), 0);
        // Until the witness below is captured, only tail is polled. Head's
        // real destination queue stays full, so an observed RELEASED(R)
        // cannot have been delivered or retired behind our observation.
        ring.head_frozen_full = true;
    }
    ring.gates[1].open();
    ring.until(
        "authentic RELEASED(R) really left tail mailbox",
        &[1],
        |ring| ring.taken_type(1, RELEASED_CONTENT_TYPE, "R"),
    )
    .await;
    let held_release = (capacity == 1).then(|| ring.authentic_held_release(&commands));
    for control in &controls[3..] {
        ring.submit(control.clone(), &[1]).await;
    }
    if capacity == 1 {
        let held_release = held_release.as_ref().unwrap();
        ring.until(
            "tail saturation or genuine C1 progress past held R",
            &[1],
            |ring| {
                let saturated = ring.adapters[1].snapshot() == "completion_queue_full:waiting"
                    && ring.accepted(1, &controls[3])
                    && ring.refused(1, &controls[4])
                    && ring.senders.iter().all(|sender| sender.capacity() == 0)
                    && ring.states[1]
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|view| view.point == "blocked_non_ack_held");
                saturated || ring.normal_c1_passed_held_release(&controls[0], held_release)
            },
        )
        .await;
        assert_eq!(ring.authentic_held_release(&commands), *held_release);
        let prevented = ring.normal_c1_passed_held_release(&controls[0], held_release);
        // Observe actual publication order; never infer this from stale prose.
        let head = ring.adapters[0].trace.lock().unwrap();
        let tail = ring.adapters[1].trace.lock().unwrap();
        assert_eq!(
            head.taken.last().unwrap().envelope.payload_content_type,
            PHYSICAL_BATCH_CONTENT_TYPE
        );
        if !prevented {
            assert_eq!(tail.taken.last(), Some(held_release));
        }
        println!("ACTOR_RING_PREVENTION capacity={capacity} normal_c1={prevented}");
        for (index, trace) in [&*head, &*tail].into_iter().enumerate() {
            println!(
                "ACTOR_RING_OWNERS capacity={capacity} node={index} broker_capacity={} accepted={:?} refused={:?} node_taken={:?}",
                ring.senders[index].capacity(),
                trace
                    .accepted
                    .iter()
                    .map(|event| &event.envelope.correlation_id)
                    .collect::<Vec<_>>(),
                trace.refusals,
                trace
                    .node_taken
                    .iter()
                    .map(|event| (
                        &event.envelope.correlation_id,
                        &event.envelope.payload_content_type
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }
    ring.head_frozen_full = false;
    ring.drain_outer();
    let before = ring.native_snapshot();
    let outer_before = ring.outer_polls;
    for _ in 0..100 {
        ring.step(&[0, 1]).await;
        if ring.complete(&commands) {
            break;
        }
    }
    let normal_progress = ring.complete(&commands);
    assert!(
        ring.outer_polls >= outer_before + 3,
        "OUTER kept draining while the ring ran"
    );
    if !normal_progress {
        assert_eq!(
            capacity, 1,
            "positive control must finish without external queue relief"
        );
        assert_eq!(
            ring.native_snapshot(),
            before,
            "no native work/release silently advanced in the stalled ring"
        );
        assert!(
            ring.states[0].lock().unwrap().iter().all(|view| {
                !(view.point == "after_release_committed"
                    && !view.pending.contains_key(&request_key("loop-session", "R")))
            }),
            "R's genuine ACK unexpectedly committed"
        );
        // Recovery is a separately labeled external intervention, NOT the
        // liveness result. Only the six known SESSION replies have independent
        // correlations and can safely be routed around the held R ACK. At most
        // six are externally taken; PHYSICAL/TAIL or other correlated telemetry
        // must NEVER be dequeued here to fake progress by reordering a stream.
        let mut relieved = std::collections::BTreeSet::new();
        for _ in 0..100 {
            ring.drain_outer();
            let may_relieve = {
                let tail = ring.adapters[1].trace.lock().unwrap();
                let held_ack = tail.node_taken.last().is_some_and(|event| {
                    event.envelope.payload_content_type == RELEASED_CONTENT_TYPE
                        && event.envelope.correlation_id == "R"
                });
                // C1..C6 enter the real worker FIFO before Q. Once all six
                // have left completion, the next item is no longer eligible.
                let sessions_left = controls.iter().any(|control| {
                    !tail.taken.iter().any(|event| {
                        event.envelope.payload_content_type == SESSION_READY_CONTENT_TYPE
                            && event.envelope.correlation_id == control.envelope.correlation_id
                    })
                });
                held_ack && sessions_left
            };
            if may_relieve && let Poll::Event(relief) = ring.adapters[1].try_take() {
                assert_eq!(
                    relief.envelope.payload_content_type,
                    SESSION_READY_CONTENT_TYPE
                );
                assert!(controls.iter().any(
                    |control| control.envelope.correlation_id == relief.envelope.correlation_id
                ));
                assert!(relieved.insert(relief.envelope.correlation_id.clone()));
                assert!(relieved.len() <= 6);
                assert!(matches!(
                    ring.broker.dispatch(event_wire(relief)),
                    Ok(DispatchOutcome::Enqueued(_))
                ));
            }
            ring.step(&[0, 1]).await;
            if ring.complete(&commands) {
                break;
            }
        }
        assert!(
            !relieved.is_empty(),
            "recovery must actually create previously unavailable capacity"
        );
        println!("ACTOR_RING_EXTERNAL_RECOVERY capacity={capacity} replies={relieved:?}");
    }
    ring.until(
        "every exact token and native release after normal/external progress",
        &[0, 1],
        |ring| ring.complete(&commands),
    )
    .await;
    ring.until(
        "all SESSION replies reach draining OUTER",
        &[0, 1],
        |ring| {
            ring.received
                .iter()
                .filter(|event| event.envelope.payload_content_type == SESSION_READY_CONTENT_TYPE)
                .count()
                == 8
        },
    )
    .await;
    ring.assert_finished(&commands);
    assert!(
        normal_progress,
        "normal cap1 EventBroker/EventNode/Worker ring stalled despite draining OUTER; bounded exact SESSION-reply dequeue recovered every token and release"
    );
}

#[test]
fn event_actor_ring_saturated_normal_ingress_must_progress_without_external_dequeue() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scenario(1));
}

#[test]
fn event_actor_ring_same_normal_ingress_completes_with_capacity_eight() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(scenario(8));
}
