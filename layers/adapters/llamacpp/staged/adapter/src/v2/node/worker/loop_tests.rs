//! B2: actual Worker::run threads, bounded input/completion mailboxes and event
//! codecs. Only native Frame computation is fake. LOAD/process/model/GPU and
//! EventNode/broker/network are NOT exercised by this independent routing pump.
//! The fake owns its own KV positions/incarnations; it does not call scheduler,
//! request, flight, ownership, frontier or settlement transition helpers.
use super::*;
use crate::process::{ReadyInfo, ServerControl};
use crate::v2::capsule::{
    GeneratedToken, Invocation, PhysicalCapsule, PhysicalOutcome, Tensor, TensorDescriptor,
};
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use p4_protocol::event::{Envelope, OuterEndpoint};
use std::collections::{BTreeMap, VecDeque};
use std::thread::JoinHandle;

mod actor_ring;
mod effect_backpressure;
mod issue_witness;
mod observation_contract;
#[path = "../../../../test-fixtures/head_approved_output.rs"]
mod output_contract;
mod release_notifications;
mod speculative;
mod submission_identity;
mod submission_limits;
mod unload;

const INPUT_CAPACITY: usize = 64;
const ROUTE_CAPACITY: usize = 512;
const SEQUENCE_CAPACITY: u32 = 8;
const BATCH_CAPACITY: usize = 4;
const PHYSICAL_CAPACITY: usize = 2;

type NativeKey = (u32, String, u64);

#[derive(Default, Debug)]
struct NativeTrace {
    logical_calls: usize,
    tokenize_calls: usize,
    first_logical_after_tokenize: Option<usize>,
    physical_calls: usize,
    live: BTreeMap<NativeKey, Vec<i32>>,
    written: BTreeMap<NativeKey, Vec<(u32, i32)>>,
    releases: BTreeMap<NativeKey, usize>,
    release_bodies: Vec<Vec<u8>>,
    sampler_calls: usize,
    shutdowns: usize,
    speculative: speculative::ScriptTrace,
    issued_native: Vec<issue_witness::NativeRecord>,
    generated_submissions: Vec<Event>,
}

struct NativeStage {
    role: NodeRole,
    next_execution: u64,
    trace: Arc<Mutex<NativeTrace>>,
    chain: Option<TokenizeChain>,
    speculative: Option<speculative::NativeScript>,
    issue_fault: Option<issue_witness::NativeFault>,
}

struct TokenizeChain {
    sender: mpsc::SyncSender<WorkerInput>,
    left: usize,
    next: usize,
}

fn native_wire(frame: Frame) -> Result<Frame, String> {
    let limits = crate::ProtocolLimits::default();
    let bytes = frame.encode(limits).map_err(|error| error.to_string())?;
    Frame::decode(&bytes, limits).map_err(|error| error.to_string())
}

fn native_key(owner: &RowOwner) -> NativeKey {
    (
        owner.sequence_id,
        owner.sequence_key.clone(),
        owner.incarnation,
    )
}

fn physical(id: u64, owners: Vec<RowOwner>) -> PhysicalCapsule {
    PhysicalCapsule {
        execution_id: id,
        terminal: false,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: 1,
            n_seqs: owners.len() as u32,
            n_seqs_unq: owners
                .iter()
                .map(|owner| owner.sequence_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len() as u32,
            n_pos: 1,
            positions: owners.iter().map(|owner| owner.position as i32).collect(),
            sequence_counts: vec![1; owners.len()],
            sequence_ids: owners
                .iter()
                .map(|owner| owner.sequence_id as i32)
                .collect(),
            output: owners.iter().map(|owner| owner.output).collect(),
        },
        owners,
        tensors: vec![Tensor {
            descriptor: TensorDescriptor {
                tensor_type: 0,
                dimensions: vec![1],
                strides: vec![4],
                nbytes: 4,
                view_offset: 0,
                alias_of: None,
                name: "loop-activation".into(),
            },
            data: vec![0; 4],
        }],
        outcomes: vec![],
    }
}

impl NativeStage {
    fn compute(&mut self, set: &mut CapsuleSet) -> Result<(), String> {
        let mut trace = self.trace.lock().unwrap();
        if let Some(script) = &mut self.speculative {
            return script.compute(self.role, &mut trace, set);
        }
        for capsule in &mut set.0 {
            for (index, owner) in capsule.owners.iter().enumerate() {
                if !matches!(owner.phase, Phase::Prefill | Phase::Decode) {
                    return Err("ordinary loop fixture received a speculative row".into());
                }
                let key = native_key(owner);
                if trace.live.keys().any(|(slot, _, _)| {
                    *slot == owner.sequence_id && !trace.live.contains_key(&key)
                }) {
                    return Err("fake native slot was reused before release".into());
                }
                let state = trace.live.entry(key.clone()).or_default();
                if owner.position as usize != state.len() {
                    return Err(format!(
                        "fake native position {} follows {} for {}",
                        owner.position,
                        state.len(),
                        owner.request_id
                    ));
                }
                state.push(owner.input_token);
                trace
                    .written
                    .entry(key)
                    .or_default()
                    .push((owner.position, owner.input_token));
                if self.role == NodeRole::Last && owner.output {
                    trace.sampler_calls += 1;
                    // Depends on request progress, not cross-request timing.
                    let token = 1000 + owner.generated_tokens as i32;
                    let finished = owner.generated_tokens + 1 == owner.max_tokens;
                    capsule.outcomes.push(PhysicalOutcome {
                        owner_index: index as u32,
                        generated: vec![GeneratedToken {
                            token,
                            text: format!("token-{token} "),
                            position: owner.position + 1,
                            stop: finished.then(|| "length".into()),
                        }],
                        proposal: if finished { vec![] } else { vec![token] },
                        retain_from: None,
                        replay_tokens: vec![],
                        replay_position: 0,
                    });
                }
            }
            if self.role == NodeRole::Last {
                capsule.terminal = true;
                capsule.tensors.clear();
            }
        }
        Ok(())
    }

    fn release(&mut self, body: &[u8]) -> Result<(), String> {
        // Independent native-wire parse, not control_identity::parse.
        if body.len() < 44 || &body[..8] != b"P4ID\x01\x00\x00\x00" {
            return Err("fake release identity header".into());
        }
        if u64::from_le_bytes(body[8..16].try_into().unwrap()) != 1 {
            return Err("fake release load".into());
        }
        let incarnation = u64::from_le_bytes(body[16..24].try_into().unwrap());
        let operation = u64::from_le_bytes(body[24..32].try_into().unwrap());
        if incarnation == 0 || operation == 0 {
            return Err("fake release zero identity".into());
        }
        let slot = u32::from_le_bytes(body[32..36].try_into().unwrap());
        let mut at = 36;
        let mut text = || -> Result<String, String> {
            let length = u32::from_le_bytes(
                body.get(at..at + 4)
                    .ok_or("short release length")?
                    .try_into()
                    .unwrap(),
            ) as usize;
            at += 4;
            let end = at.checked_add(length).ok_or("release length overflow")?;
            let value = std::str::from_utf8(body.get(at..end).ok_or("short release text")?)
                .map_err(|e| e.to_string())?
                .to_owned();
            at = end;
            Ok(value)
        };
        let session = text()?;
        let key = text()?;
        if at != body.len() || session != "loop-session" || !key.starts_with("loop-session\0") {
            return Err("fake release identity body".into());
        }
        let key = (slot, key, incarnation);
        let mut trace = self.trace.lock().unwrap();
        if let Some(script) = &mut self.speculative {
            script.release(&mut trace, key, operation)?;
            trace.release_bodies.push(body.to_vec());
            return Ok(());
        }
        trace
            .live
            .remove(&key)
            .ok_or("fake release has no live KV")?;
        *trace.releases.entry(key).or_default() += 1;
        trace.release_bodies.push(body.to_vec());
        Ok(())
    }
}

impl ServerControl for NativeStage {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        Ok(Some(ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "loop-fake-no-model".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 2048,
            n_batch: BATCH_CAPACITY,
            n_ubatch: PHYSICAL_CAPACITY,
            n_seq_max: SEQUENCE_CAPACITY,
            upstream_commit: "fixture".into(),
            patch_set: "fixture".into(),
            backend_inventory: "no-engine".into(),
            stage_wire_abi: "unknown".into(),
        }))
    }
    fn request(&mut self, frame: Frame) -> Result<Frame, String> {
        let frame = native_wire(frame)?;
        let issued_input =
            (frame.header.operation == Operation::LogicalBatch).then(|| frame.body.clone());
        let mut result = match frame.header.operation {
            Operation::Tokenize => {
                self.trace.lock().unwrap().tokenize_calls += 1;
                if let Some(chain) = self.chain.as_mut() {
                    chain.left -= 1;
                    if chain.left > 0 {
                        let event = event_wire(tokenize_event(chain.next));
                        chain.next += 1;
                        chain
                            .sender
                            .try_send(WorkerInput::Event(event.clone()))
                            .map_err(|_| "tokenize chain unexpectedly exhausted its input slot")?;
                        self.trace.lock().unwrap().generated_submissions.push(event);
                    } else {
                        // A finite chain must not retain its own worker's
                        // input sender once every successor has been supplied.
                        self.chain = None;
                    }
                }
                let mut body = 1u32.to_le_bytes().to_vec();
                body.extend_from_slice(&10i32.to_le_bytes());
                return native_wire(
                    Frame::new(Operation::Tokenized, body).map_err(|e| e.to_string())?,
                );
            }
            Operation::LogicalBatch => {
                let mut trace = self.trace.lock().unwrap();
                let tokenizes = trace.tokenize_calls;
                trace.first_logical_after_tokenize.get_or_insert(tokenizes);
                trace.logical_calls += 1;
                drop(trace);
                let logical = LogicalBatch::decode(&frame.body).map_err(|e| format!("{e:?}"))?;
                let mut capsules = Vec::new();
                if self.speculative.is_some() {
                    capsules = speculative::split_logical(&logical, &mut self.next_execution);
                } else {
                    for chunk in logical.0.chunks(PHYSICAL_CAPACITY) {
                        assert!(chunk.iter().all(|row| row.token == row.owner.input_token));
                        capsules.push(physical(
                            self.next_execution,
                            chunk.iter().map(|row| row.owner.clone()).collect(),
                        ));
                        self.next_execution += 1;
                    }
                }
                CapsuleSet(capsules)
            }
            Operation::PhysicalBatch => {
                self.trace.lock().unwrap().physical_calls += 1;
                CapsuleSet::decode(&frame.body).map_err(|e| format!("{e:?}"))?
            }
            Operation::PhysicalRelease => {
                self.release(&frame.body)?;
                return native_wire(
                    Frame::new(Operation::PhysicalRelease, frame.body)
                        .map_err(|e| e.to_string())?,
                );
            }
            Operation::PhysicalSettle => {
                let script = self
                    .speculative
                    .as_mut()
                    .ok_or("ordinary fixture received SETTLE")?;
                let body =
                    script.settle(self.role, &mut self.trace.lock().unwrap(), &frame.body)?;
                return native_wire(
                    Frame::new(Operation::PhysicalSettle, body).map_err(|e| e.to_string())?,
                );
            }
            other => return Err(format!("ordinary loop unexpected native opcode {other:?}")),
        };
        self.compute(&mut result)?;
        if let Some(input) = issued_input {
            let call = self.trace.lock().unwrap().logical_calls;
            if call == 2 && self.issue_fault == Some(issue_witness::NativeFault::FailSecond) {
                self.trace
                    .lock()
                    .unwrap()
                    .issued_native
                    .push(issue_witness::NativeRecord {
                        input,
                        result: None,
                    });
                return Err("issue-witness fixture lost the second native reply".into());
            }
            if call == 2 && self.issue_fault == Some(issue_witness::NativeFault::ReuseExecution) {
                // Native KV really advanced above. Return a codec-valid but
                // previously used execution ID; acceptance must not count it.
                result.0[0].execution_id = 1;
            }
            self.trace
                .lock()
                .unwrap()
                .issued_native
                .push(issue_witness::NativeRecord {
                    input,
                    result: Some(result.encode().map_err(|error| format!("{error:?}"))?),
                });
        }
        native_wire(
            Frame::new(
                Operation::PhysicalResult,
                result.encode().map_err(|e| format!("{e:?}"))?,
            )
            .map_err(|e| e.to_string())?,
        )
    }
    fn shutdown(&mut self) -> Result<(), String> {
        self.trace.lock().unwrap().shutdowns += 1;
        Ok(())
    }
}

fn address() -> Address {
    Address::tcp("127.0.0.1", 42999)
}
fn endpoint(index: usize) -> Endpoint {
    Endpoint::node(address(), format!("loop-{index}"), 1)
}
fn node_address(index: usize) -> NodeAddress {
    NodeAddress {
        agent: address().to_string(),
        node: format!("loop-{index}"),
        generation: 1,
    }
}

fn event(index: usize, name: &str, content: &str, payload: Vec<u8>) -> Event {
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: format!("input-{name}"),
            correlation_id: name.into(),
            causation_id: None,
            source: Endpoint::outer(address(), "loop-output", 1),
            target: endpoint(index),
            return_route: Some(OuterEndpoint {
                ingress_agent: address(),
                channel: "loop-output".into(),
                connection_generation: 1,
            }),
            class: if content == PREFILL_CONTENT_TYPE {
                EventClass::Data
            } else {
                EventClass::Control
            },
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: content.into(),
        },
        payload,
    }
}

fn request(name: &str, prompt_rows: usize, max_tokens: u32) -> InferenceCommand {
    InferenceCommand {
        load_generation: 1,
        session_id: "loop-session".into(),
        request_id: name.into(),
        tokens: (0..prompt_rows).map(|index| 10 + index as i32).collect(),
        prompt: None,
        options: String::new(),
        session_key: None,
        max_tokens,
    }
}

fn request_event(command: &InferenceCommand) -> Event {
    event(
        0,
        &command.request_id,
        PREFILL_CONTENT_TYPE,
        serde_json::to_vec(command).unwrap(),
    )
}

// Independent declaration of the OUTER Sender's public event-ID format. The
// fixture's SESSION setup has a separate control identity; inference submissions
// start at one and advance in actual send order, just as an inference Sender does.
fn submission_event(command: &InferenceCommand, sequence: u64, route: OuterEndpoint) -> Event {
    let mut value = request_event(command);
    value.envelope.event_id = format!(
        "outer:{}:{}:{}:{sequence}",
        route.ingress_agent, route.channel, route.connection_generation
    );
    value.envelope.sequence = sequence;
    value.envelope.source = Endpoint::Outer(route.clone());
    value.envelope.return_route = Some(route);
    value
}

fn default_route() -> OuterEndpoint {
    OuterEndpoint {
        ingress_agent: address(),
        channel: "loop-output".into(),
        connection_generation: 1,
    }
}

fn tokenize_event(index: usize) -> Event {
    let mut command = request(&format!("tokenize-chain-{index}"), 1, 1);
    command.tokens.clear();
    command.prompt = Some("A normal prompt for the deterministic native boundary fixture".into());
    request_event(&command)
}

fn event_wire(event: Event) -> Event {
    p4_protocol::event::decode(&p4_protocol::event::encode(&event).unwrap()).unwrap()
}

struct Node {
    sender: Option<mpsc::SyncSender<WorkerInput>>,
    mailbox: Arc<CompletionMailbox>,
    snapshot: Arc<Mutex<String>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    native: Arc<Mutex<NativeTrace>>,
}

struct Harness {
    nodes: Vec<Node>,
    pending: VecDeque<Event>,
    received: Vec<Event>,
    outputs: Vec<OutcomePayload>,
    routed: usize,
    hold_tail: bool,
    held_tail: VecDeque<Event>,
    hold_control: Option<(String, usize)>,
    held_control: VecDeque<Event>,
    expected_unload_error: Option<(String, usize)>,
    submissions: Vec<Event>,
    stage_events: Vec<Event>,
    next_submission: u64,
    paused_completions: Vec<bool>,
    expected_issue_error: Option<(String, &'static str)>,
    // Only the exact deliberately invalid submission may return an error.
    // The child test checks the actual error code/detail and all side effects.
    expected_submission_error: Option<Event>,
    // One deliberately invalid ACK may yield exactly this error/provenance.
    // Consumed on delivery; a duplicate or any unrelated error still fails.
    expected_ack_error: Option<(Event, serde_json::Value)>,
    accepted: observation_contract::Accepted,
}

impl Harness {
    fn new(
        stages: usize,
        max_open: usize,
        completion_capacity: usize,
        initial: &[InferenceCommand],
    ) -> Self {
        Self::with_tokenize_chain(stages, max_open, completion_capacity, initial, 0)
    }

    fn with_tokenize_chain(
        stages: usize,
        max_open: usize,
        completion_capacity: usize,
        initial: &[InferenceCommand],
        chain_length: usize,
    ) -> Self {
        Self::configured(
            stages,
            max_open,
            completion_capacity,
            initial,
            chain_length,
            None,
        )
    }

    fn configured(
        stages: usize,
        max_open: usize,
        completion_capacity: usize,
        initial: &[InferenceCommand],
        chain_length: usize,
        script: Option<speculative::Scenario>,
    ) -> Self {
        let submissions: Vec<_> = initial
            .iter()
            .enumerate()
            .map(|(index, command)| submission_event(command, index as u64 + 1, default_route()))
            .collect();
        Self::configured_events(
            stages,
            max_open,
            completion_capacity,
            &submissions,
            chain_length,
            script,
        )
    }

    fn configured_events(
        stages: usize,
        max_open: usize,
        completion_capacity: usize,
        initial: &[Event],
        chain_length: usize,
        script: Option<speculative::Scenario>,
    ) -> Self {
        Self::observed_events(
            stages,
            max_open,
            completion_capacity,
            initial,
            chain_length,
            script,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn observed_events(
        stages: usize,
        max_open: usize,
        completion_capacity: usize,
        initial: &[Event],
        chain_length: usize,
        script: Option<speculative::Scenario>,
        observer: Option<IssueObserver>,
        issue_fault: Option<issue_witness::NativeFault>,
    ) -> Self {
        assert!((2..=8).contains(&stages));
        assert!(initial.len() < INPUT_CAPACITY);
        let mut nodes = Vec::new();
        let mut workers = Vec::new();
        let accepted = observation_contract::accepted();
        for index in 0..stages {
            let role = if index == 0 {
                NodeRole::First
            } else if index + 1 == stages {
                NodeRole::Last
            } else {
                NodeRole::Middle
            };
            let (sender, receiver) = mpsc::sync_channel(INPUT_CAPACITY);
            let (publisher, mailbox) = completion_mailbox(completion_capacity);
            let native = Arc::new(Mutex::new(NativeTrace::default()));
            let snapshot = Arc::new(Mutex::new(String::new()));
            let shutdown = Arc::new(AtomicBool::new(false));
            let mut worker = Worker::new(
                endpoint(index),
                receiver,
                publisher,
                Arc::clone(&snapshot),
                Arc::clone(&shutdown),
            )
            .with_stage_for_test(Box::new(NativeStage {
                role,
                next_execution: 1,
                trace: Arc::clone(&native),
                chain: (index == 0 && chain_length > 0).then(|| TokenizeChain {
                    sender: sender.clone(),
                    left: chain_length,
                    next: 1,
                }),
                speculative: script.map(speculative::NativeScript::new),
                issue_fault: if index == 0 { issue_fault } else { None },
            }))
            .unwrap();
            if index == 0 {
                worker.issue_observer = Some(observation_contract::observer(
                    Arc::clone(&accepted),
                    Arc::clone(&native),
                    observer.clone(),
                ));
            }
            // Explicit post-LOAD fixture. No command parser/process/native
            // placement/capacity negotiation proof is claimed by these fields.
            worker.state.load_generation = 1;
            worker.state.physical_receives =
                crate::v2::node::physical_receive::PhysicalReceiveLedger::new(1).unwrap();
            worker.state.batch_capacity = BATCH_CAPACITY;
            worker.state.physical_capacity = PHYSICAL_CAPACITY;
            worker.state.context_size = 256;
            worker.state.sequence_capacity = SEQUENCE_CAPACITY;
            worker.state.free_sequences = (0..SEQUENCE_CAPACITY).collect();
            if let Some(script) = script {
                // Full acceptance tests deliberate one-slot reuse; settlement
                // tests retain a second independently runnable fence probe.
                worker.state.sequence_capacity = script.sequence_capacity();
                worker.state.free_sequences = (0..script.sequence_capacity()).collect();
            }
            worker.state.max_atomic_sequences = 1;
            worker.state.equal_sequence_ubatch = false;
            worker.state.atomic_batch_exclusive = false;
            worker.state.min_batch_rows = 0;
            worker.state.max_open_batches = max_open;
            worker.state.max_issue_rows = 0;
            worker.state.prefill_fragments = 1;
            let session = SessionCommand {
                load_generation: 1,
                session_id: "loop-session".into(),
                stages: (0..stages).map(node_address).collect(),
                stage_index: index,
            };
            sender
                .try_send(WorkerInput::Event(event_wire(event(
                    index,
                    &format!("session-{index}"),
                    SESSION_CONTENT_TYPE,
                    serde_json::to_vec(&session).unwrap(),
                ))))
                .unwrap_or_else(|_| panic!("SESSION fixture exceeds bounded input"));
            if index == 0 {
                for submission in initial {
                    sender
                        .try_send(WorkerInput::Event(event_wire(submission.clone())))
                        .unwrap_or_else(|_| panic!("first wave exceeds bounded input"));
                }
                if chain_length > 0 {
                    let event = event_wire(tokenize_event(0));
                    sender
                        .try_send(WorkerInput::Event(event.clone()))
                        .unwrap_or_else(|_| panic!("initial tokenize event exceeds bounded input"));
                    native.lock().unwrap().generated_submissions.push(event);
                }
            }
            nodes.push(Node {
                sender: Some(sender),
                mailbox,
                snapshot,
                shutdown,
                thread: None,
                native,
            });
            workers.push(worker);
        }
        // Every SESSION and the complete first wave are queued before any run
        // loop starts. This removes thread-start races from initial membership.
        for (node, worker) in nodes.iter_mut().zip(workers) {
            node.thread = Some(std::thread::spawn(move || worker.run()));
        }
        Self {
            nodes,
            pending: VecDeque::new(),
            received: vec![],
            outputs: vec![],
            routed: 0,
            hold_tail: false,
            held_tail: VecDeque::new(),
            hold_control: None,
            held_control: VecDeque::new(),
            expected_unload_error: None,
            submissions: initial.iter().cloned().map(event_wire).collect(),
            stage_events: vec![],
            next_submission: initial.len() as u64 + 1,
            paused_completions: vec![false; stages],
            expected_issue_error: issue_fault.map(|fault| {
                assert_eq!(initial.len(), 1);
                (
                    initial[0].envelope.correlation_id.clone(),
                    fault.error_code(),
                )
            }),
            expected_submission_error: None,
            expected_ack_error: None,
            accepted,
        }
    }

    fn enqueue(&mut self, command: &InferenceCommand) {
        assert!(self.pending.len() < ROUTE_CAPACITY);
        let submitted = event_wire(submission_event(
            command,
            self.next_submission,
            default_route(),
        ));
        self.next_submission += 1;
        self.submissions.push(submitted.clone());
        self.pending.push_back(submitted);
    }

    fn tick(&mut self) {
        // Fair bounded drain per node. The independent pump must not make an
        // unbounded broker promise or drop an event on a full node input.
        for (index, node) in self.nodes.iter().enumerate() {
            if self.paused_completions[index] {
                continue;
            }
            for _ in 0..16 {
                let Poll::Event(event) = node.mailbox.try_take() else {
                    break;
                };
                let event = event_wire(event);
                let accepted_ack_error = event.envelope.payload_content_type == ERROR_CONTENT_TYPE
                    && self
                        .expected_ack_error
                        .as_ref()
                        .is_some_and(|(input, body)| {
                            event.envelope.class == EventClass::Output
                                && event.envelope.source == input.envelope.target
                                && event.envelope.target == reply_target(input)
                                && event.envelope.return_route == input.envelope.return_route
                                && event.envelope.correlation_id == input.envelope.correlation_id
                                && event.envelope.deadline_unix_ms
                                    == input.envelope.deadline_unix_ms
                                && event.envelope.causation_id.as_deref()
                                    == Some(input.envelope.event_id.as_str())
                                && event.envelope.event_id
                                    == format!(
                                        "{}:llamacpp:{}",
                                        input.envelope.event_id, event.envelope.sequence
                                    )
                                && serde_json::from_slice::<serde_json::Value>(&event.payload)
                                    .ok()
                                    .as_ref()
                                    == Some(body)
                        });
                if accepted_ack_error {
                    self.expected_ack_error.take();
                }
                if event.envelope.payload_content_type == ERROR_CONTENT_TYPE
                    && !accepted_ack_error
                    && !self
                        .expected_unload_error
                        .as_ref()
                        .is_some_and(|(correlation, index)| {
                            event.envelope.correlation_id == *correlation
                                && event.envelope.source == endpoint(*index)
                        })
                    && !self
                        .expected_issue_error
                        .as_ref()
                        .is_some_and(|(correlation, code)| {
                            event.envelope.source == endpoint(0)
                                && event.envelope.correlation_id == *correlation
                                && serde_json::from_slice::<serde_json::Value>(&event.payload)
                                    .ok()
                                    .is_some_and(|body| body["code"] == *code)
                        })
                    && !self
                        .expected_submission_error
                        .as_ref()
                        .is_some_and(|input| {
                            event.envelope.source == input.envelope.target
                                && event.envelope.target == input.envelope.source
                                && event.envelope.return_route == input.envelope.return_route
                                && event.envelope.correlation_id == input.envelope.correlation_id
                                && event.envelope.causation_id.as_deref()
                                    == Some(input.envelope.event_id.as_str())
                        })
                {
                    panic!(
                        "worker rejected event: {}",
                        String::from_utf8_lossy(&event.payload)
                    );
                }
                match &event.envelope.target {
                    Endpoint::Node { .. } => {
                        assert!(
                            self.stage_events.len() < 32_768,
                            "stage event capture bound"
                        );
                        self.stage_events.push(event.clone());
                        if self.hold_control.as_ref().is_some_and(|(content, target)| {
                            event.envelope.payload_content_type == *content
                                && event.envelope.target == endpoint(*target)
                        }) {
                            assert!(self.held_control.len() < ROUTE_CAPACITY);
                            self.held_control.push_back(event);
                            continue;
                        }
                        if self.hold_tail
                            && event.envelope.payload_content_type == TAIL_BATCH_CONTENT_TYPE
                        {
                            assert!(self.held_tail.len() < ROUTE_CAPACITY);
                            self.held_tail.push_back(event);
                            continue;
                        }
                        assert!(
                            self.pending.len() < ROUTE_CAPACITY,
                            "routing queue exceeded its declared bound"
                        );
                        self.pending.push_back(event);
                    }
                    Endpoint::Outer(_) => {
                        assert!(
                            self.received.len() < 32_768,
                            "fixture completion budget exhausted"
                        );
                        if event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE {
                            self.outputs
                                .push(serde_json::from_slice(&event.payload).unwrap());
                        }
                        self.received.push(event);
                    }
                    Endpoint::Agent(_) => panic!("unexpected agent-targeted fixture completion"),
                }
            }
        }
        let count = self.pending.len();
        for _ in 0..count {
            let event = self.pending.pop_front().unwrap();
            let index = (0..self.nodes.len())
                .find(|index| event.envelope.target == endpoint(*index))
                .expect("unknown route target");
            match self.nodes[index]
                .sender
                .as_ref()
                .unwrap()
                .try_send(WorkerInput::Event(event))
            {
                Ok(()) => self.routed += 1,
                Err(mpsc::TrySendError::Full(WorkerInput::Event(event))) => {
                    self.pending.push_back(event)
                }
                Err(mpsc::TrySendError::Disconnected(_)) => panic!(
                    "worker {index} stopped: {}",
                    self.nodes[index].snapshot.lock().unwrap()
                ),
            }
        }
    }

    fn until(&mut self, description: &str, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate(self) {
            assert!(
                Instant::now() < deadline,
                "{description} timed out: outputs={}, pending={}, snapshots={:?}",
                self.outputs.len(),
                self.pending.len(),
                self.nodes
                    .iter()
                    .map(|node| node.snapshot.lock().unwrap().clone())
                    .collect::<Vec<_>>()
            );
            self.tick();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn pump_for(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            self.tick();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn resume_tail(&mut self) {
        self.hold_tail = false;
        assert!(self.pending.len() + self.held_tail.len() <= ROUTE_CAPACITY);
        self.pending.extend(self.held_tail.drain(..));
    }

    fn finish(&mut self, commands: &[InferenceCommand]) {
        let expected = commands
            .iter()
            .map(|command| command.max_tokens as usize)
            .sum::<usize>();
        self.until("all output tokens and every native release", |h| {
            h.outputs.len() == expected
                && h.received
                    .iter()
                    .filter(|event| {
                        event.envelope.source == endpoint(0)
                            && event.envelope.payload_content_type
                                == crate::v2::RELEASE_RECEIPT_CONTENT_TYPE
                    })
                    .map(|event| {
                        serde_json::from_slice::<crate::v2::ReleaseReceipt>(&event.payload)
                            .unwrap()
                            .members
                            .len()
                    })
                    .sum::<usize>()
                    == commands.len()
                && h.nodes
                    .iter()
                    .all(|node| node.native.lock().unwrap().releases.len() == commands.len())
        });
        for command in commands {
            let outputs: Vec<_> = self
                .outputs
                .iter()
                .filter(|output| output.request_id == command.request_id)
                .collect();
            assert_eq!(outputs.len(), command.max_tokens as usize);
            for (index, output) in outputs.iter().enumerate() {
                assert_eq!(output.token, 1000 + index as i32);
                assert_eq!(output.position as usize, command.tokens.len() + index);
                assert_eq!(output.text, format!("token-{} ", output.token));
                assert_eq!(
                    output.stop.as_deref(),
                    (index + 1 == outputs.len()).then_some("length")
                );
            }
        }
        for node in &self.nodes {
            let native = node.native.lock().unwrap();
            assert!(
                native.live.is_empty(),
                "all KV must be released on every stage"
            );
            assert!(native.releases.values().all(|count| *count == 1));
            for command in commands {
                let records: Vec<_> = native
                    .written
                    .iter()
                    .filter(|((_, key, _), _)| {
                        key == &request_key("loop-session", &command.request_id)
                    })
                    .collect();
                assert_eq!(records.len(), 1);
                let writes = records[0].1;
                let expected_tokens: Vec<_> = command
                    .tokens
                    .iter()
                    .copied()
                    .chain((0..command.max_tokens - 1).map(|index| 1000 + index as i32))
                    .collect();
                assert_eq!(
                    writes,
                    &expected_tokens
                        .iter()
                        .enumerate()
                        .map(|(position, token)| (position as u32, *token))
                        .collect::<Vec<_>>()
                );
            }
        }
        self.wait_for_observations();
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Teardown is bounded, but not a graceful-drain proof: every test must
        // assert its own completion/release barrier before dropping the pump.
        for node in &mut self.nodes {
            node.shutdown.store(true, Ordering::SeqCst);
            node.sender.take();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.nodes.iter().any(|node| {
            node.thread
                .as_ref()
                .is_some_and(|thread| !thread.is_finished())
        }) && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        for node in &mut self.nodes {
            if node.thread.as_ref().is_some_and(JoinHandle::is_finished) {
                let joined = node.thread.take().unwrap().join();
                // A counterexample may already be unwinding. Preserve its
                // original failure; a second panic here aborts the test binary
                // and hides the actual violated invariant from libtest.
                if joined.is_err() && !std::thread::panicking() {
                    panic!("worker thread panicked");
                }
            } else if !std::thread::panicking() {
                panic!("worker loop did not stop after channel close");
            }
        }
    }
}

#[test]
fn b2_two_stage_run_loop_completes_one_request_through_real_event_and_native_codecs() {
    let commands = vec![request("one", 7, 5)];
    let mut harness = Harness::new(2, 1, 8, &commands);
    harness.finish(&commands);
    release_notifications::assert_complete(&harness);
    assert!(harness.routed > 0);
    assert!(harness.nodes[0].native.lock().unwrap().logical_calls > 1);
    assert_eq!(harness.nodes[1].native.lock().unwrap().sampler_calls, 5);
    let output_events: Vec<_> = harness
        .received
        .iter()
        .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
        .cloned()
        .collect();
    output_contract::assert_live_matches(
        "ordinary-2",
        &harness.submissions,
        &output_events,
        &harness.received,
    );
    output_contract::assert_live_prefill_counts("ordinary-2", &harness.received);
}

#[test]
fn b2_middle_cannot_impersonate_tail_release_or_reuse_a_slot_before_the_real_ack() {
    // Fill every real slot, then leave one independently observed request in
    // admission. The routing pump holds all genuine tail release acknowledgments.
    let mut commands: Vec<_> = (0..SEQUENCE_CAPACITY)
        .map(|index| request(&format!("source-owner-{index}"), 1, 1))
        .collect();
    commands.push(request("waiting-for-authentic-release", 1, 1));
    let mut h = Harness::new(3, 1, 32, &commands);
    h.hold_control = Some((RELEASED_CONTENT_TYPE.into(), 0));
    h.until(
        "all occupied slots await genuine release acknowledgments",
        |h| {
            h.held_control
                .iter()
                .map(|event| {
                    serde_json::from_slice::<ReleaseCommand>(&event.payload)
                        .unwrap()
                        .sequences
                        .len()
                })
                .sum::<usize>()
                == SEQUENCE_CAPACITY as usize
        },
    );
    let waiting_key = request_key("loop-session", "waiting-for-authentic-release");
    let started_waiting = |h: &Harness| {
        h.nodes[0]
            .native
            .lock()
            .unwrap()
            .written
            .keys()
            .any(|(_, key, _)| *key == waiting_key)
    };
    assert!(
        !started_waiting(&h),
        "the waiting request must be genuinely unadmitted"
    );
    let native_snapshot = |h: &Harness| {
        h.nodes
            .iter()
            .map(|node| {
                let native = node.native.lock().unwrap();
                (
                    native.logical_calls,
                    native.physical_calls,
                    native.sampler_calls,
                    native.live.clone(),
                    native.written.clone(),
                    native.releases.clone(),
                    native.release_bodies.clone(),
                    native.shutdowns,
                    native.speculative.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let before = native_snapshot(&h);
    let output_before = h.outputs.clone();
    let held_before: Vec<_> = h.held_control.iter().cloned().collect();
    assert!(
        h.held_control
            .iter()
            .all(|event| event.envelope.source == endpoint(2)
                && event.envelope.target == endpoint(0))
    );
    let mut forged = h.held_control[0].clone();
    let tag = "middle-pretending-to-be-tail-release";
    forged.envelope.source = endpoint(1);
    forged.envelope.event_id = "fresh-middle-release-forgery".into();
    forged.envelope.correlation_id = tag.into();
    // The legacy fixture field is a strict (correlation, responding endpoint)
    // rejection allowlist, not an UNLOAD-specific handler shortcut.
    assert!(h.expected_unload_error.is_none());
    h.expected_unload_error = Some((tag.into(), 0));
    h.pending.push_back(event_wire(forged));
    h.until(
        "forged ACK is rejected or incorrectly starts waiting work",
        |h| {
            started_waiting(h)
                || h.received.iter().any(|event| {
                    event.envelope.payload_content_type == ERROR_CONTENT_TYPE
                        && event.envelope.correlation_id == tag
                })
        },
    );
    println!(
        "RELEASE_SOURCE_GUARD waiting_started={} head_logical_before={} head_logical_after={}",
        started_waiting(&h),
        before[0].0,
        h.nodes[0].native.lock().unwrap().logical_calls
    );
    assert!(
        !started_waiting(&h),
        "a middle-stage forged ACK admitted new native work"
    );
    assert_eq!(
        native_snapshot(&h),
        before,
        "rejected source cannot alter any stage's calls, KV, writes, releases or sampler"
    );
    assert_eq!(h.outputs, output_before);
    assert_eq!(
        h.held_control.iter().cloned().collect::<Vec<_>>(),
        held_before
    );
    let errors: Vec<_> = h
        .received
        .iter()
        .filter(|event| {
            event.envelope.payload_content_type == ERROR_CONTENT_TYPE
                && event.envelope.correlation_id == tag
        })
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].envelope.source, endpoint(0));
    let error: serde_json::Value = serde_json::from_slice(&errors[0].payload).unwrap();
    assert_eq!(error["code"], "LLAMA_ADAPTER_EVENT_REJECTED");
    assert_eq!(
        error["detail"],
        "release completion route does not match the declared pipeline"
    );
    assert!(
        !h.received
            .iter()
            .any(|event| event.envelope.payload_content_type
                == crate::v2::RELEASE_RECEIPT_CONTENT_TYPE
                && event.envelope.correlation_id == tag)
    );
    // An authentic ACK must still make progress: the rejection is not a global
    // stop or a disguised always-reject policy. Keep the existing full oracle.
    h.expected_unload_error = None;
    h.hold_control = None;
    h.pending.extend(h.held_control.drain(..));
    h.finish(&commands);
    assert!(started_waiting(&h));
}

#[test]
fn b2_continuous_input_grants_runnable_work_a_plan_within_32_handled_events() {
    let mut observations = Vec::new();
    for chain_length in [0, 16, 256] {
        let first = request("already-runnable", 1, 1);
        let mut commands = vec![first.clone()];
        commands.extend(
            (0..chain_length).map(|index| request(&format!("tokenize-chain-{index}"), 1, 1)),
        );
        let mut harness = Harness::with_tokenize_chain(2, 1, 8, &[first], chain_length);
        harness.until("first logical native issue", |h| {
            h.nodes[0]
                .native
                .lock()
                .unwrap()
                .first_logical_after_tokenize
                .is_some()
        });
        let observed = harness.nodes[0]
            .native
            .lock()
            .unwrap()
            .first_logical_after_tokenize
            .unwrap();
        println!(
            "INPUT_DRAIN_BOUND chain_length={chain_length} tokenizes_before_first_logical={observed}"
        );
        // First prove that the causal producer chain and all its real worker
        // outputs/settlements/releases complete. The bound assertion below
        // must not leave a self-retained fixture input sender alive on failure.
        harness.finish(&commands);
        assert_eq!(
            harness.nodes[0].native.lock().unwrap().tokenize_calls,
            chain_length
        );
        observations.push((chain_length, observed));
    }
    // This literal is the independently specified actor-service contract. Do
    // NOT import its production constant: changing that to MAX must not make
    // the regression test silently approve the original starvation behavior.
    for (chain_length, observed) in observations {
        assert!(
            observed <= 32,
            "{observed} input events postponed an already-runnable issue; actor opportunity bound is 32 (chain length {chain_length})"
        );
        if chain_length > 32 {
            assert!(
                observed < chain_length,
                "the first native issue waited for the whole ingress chain"
            );
        }
    }
}

#[test]
fn b2_two_four_eight_stage_wave_join_preserves_each_request_and_releases_every_stage_once() {
    for stages in [2, 4, 8] {
        let mut commands: Vec<_> = (0..6)
            .map(|index| {
                request(
                    &format!("wave0-{index}"),
                    if index % 2 == 0 { 13 } else { 3 },
                    if index == 0 { 20 } else { 5 },
                )
            })
            .collect();
        let mut harness = Harness::new(stages, 2, 8, &commands);
        harness.until("first wave emits while still in flight", |h| {
            h.outputs
                .iter()
                .any(|output| output.request_id == "wave0-0")
        });
        assert!(
            !harness
                .outputs
                .iter()
                .any(|output| output.request_id == "wave0-0" && output.stop.is_some())
        );
        let later = request("wave1-short", 2, 2);
        harness.enqueue(&later);
        commands.push(later);
        let later = request("wave1-long", 17, 3);
        harness.enqueue(&later);
        commands.push(later);
        harness.finish(&commands);
        let new_first = harness
            .outputs
            .iter()
            .position(|output| output.request_id == "wave1-short")
            .unwrap();
        let old_last = harness
            .outputs
            .iter()
            .position(|output| output.request_id == "wave0-0" && output.stop.is_some())
            .unwrap();
        assert!(
            new_first < old_last,
            "a later wave must make progress before the old request completes, stages={stages}"
        );
    }
}

#[test]
fn b2_full_completion_mailbox_waits_then_delivers_without_discarding_native_results() {
    let commands = vec![request("full-mailbox", 7, 4)];
    let mut harness = Harness::new(2, 1, 1, &commands);
    // Do not drain. SESSION_READY occupies the single completion slot, so the
    // already computed head result must encounter Full in Worker::run.
    let until = Instant::now() + Duration::from_secs(2);
    while harness.nodes[0].snapshot.lock().unwrap().as_str() != "completion_queue_full:waiting" {
        assert!(Instant::now() < until, "completion Full was not exercised");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(harness.nodes[0].native.lock().unwrap().logical_calls, 1);
    assert!(!harness.nodes[0].thread.as_ref().unwrap().is_finished());
    harness.finish(&commands);
}

#[test]
fn b2_open_batch_one_waits_for_last_physical_return_and_two_is_a_positive_control() {
    let commands: Vec<_> = (0..6)
        .map(|index| request(&format!("gate-{index}"), 5, 2))
        .collect();
    for max_open in [1, 2] {
        let mut harness = Harness::new(2, max_open, 16, &commands);
        harness.hold_tail = true;
        harness.until("tail computes the first logical issue", |h| {
            !h.held_tail.is_empty()
        });
        if max_open == 2 {
            harness.until("depth two issues again before any terminal return", |h| {
                h.nodes[0].native.lock().unwrap().logical_calls >= 2
            });
            assert!(harness.outputs.is_empty());
            harness.until("both independent logical issues reached the tail", |h| {
                h.held_tail.len() >= 2
            });
            let first = harness.held_tail.pop_front().unwrap();
            let second = harness.held_tail.pop_front().unwrap();
            let first_slots: std::collections::BTreeSet<_> = CapsuleSet::decode(&first.payload)
                .unwrap()
                .0
                .iter()
                .flat_map(|capsule| &capsule.owners)
                .map(|row| row.sequence_id)
                .collect();
            let second_slots: std::collections::BTreeSet<_> = CapsuleSet::decode(&second.payload)
                .unwrap()
                .0
                .iter()
                .flat_map(|capsule| &capsule.owners)
                .map(|row| row.sequence_id)
                .collect();
            assert!(
                first_slots.is_disjoint(&second_slots),
                "the reordering fixture must not reverse one sequence's rows"
            );
            // Deliver a later independent issue before the earlier one. The
            // worker's real flight ledger, not this pump, must settle it.
            harness.held_tail.push_front(first);
            harness.held_tail.push_front(second);
        } else {
            harness.pump_for(Duration::from_millis(30));
            assert_eq!(harness.nodes[0].native.lock().unwrap().logical_calls, 1);
            let mut original = harness.held_tail.pop_front().unwrap();
            let mut set = CapsuleSet::decode(&original.payload).unwrap();
            assert_eq!(
                set.0.len(),
                2,
                "one logical issue must really split into two executions"
            );
            let last = set.0.pop().unwrap();
            let mut partial = original.clone();
            partial.envelope.event_id.push_str(":first-fragment");
            partial.payload = set.encode().unwrap();
            harness.pending.push_back(partial);
            original.envelope.event_id.push_str(":last-fragment");
            original.payload = CapsuleSet(vec![last]).encode().unwrap();
            harness.held_tail.push_front(original);
            harness.pump_for(Duration::from_millis(30));
            assert_eq!(
                harness.nodes[0].native.lock().unwrap().logical_calls,
                1,
                "partial terminal return must not free the logical batch slot"
            );
        }
        harness.resume_tail();
        harness.finish(&commands);
    }
}
