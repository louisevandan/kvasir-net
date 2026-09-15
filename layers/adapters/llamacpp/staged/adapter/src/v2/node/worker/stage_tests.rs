//! Real worker/native-Frame boundary tests. The fake supplies native results,
//! never scheduler, flight ledger, admission or settlement transitions.
//! Lifecycle readiness is exercised, but direct test stage injection bypasses
//! LOAD event parsing and its capacity checks; this is not a native/GPU test.

use super::*;
use crate::process::{ReadyInfo, ServerControl};
use crate::v2::capsule::{
    GeneratedToken, Invocation, PhysicalCapsule, PhysicalOutcome, Tensor, TensorDescriptor,
};
use crate::v2::node::state::IssueProgress;
use p4_adapter::node_adapter::{
    CompletionMailbox, OwnedPoll, Poll, completion_mailbox, completion_mailbox_with_limits,
};
use p4_protocol::event::OuterEndpoint;

#[derive(Clone, Copy)]
enum ResponseMode {
    ExactSplit,
    LostAfterExecution,
    OmittedRow,
    SettlementShortBody,
    SettlementWrongLength,
    SettlementUnexpectedProposal,
    SettlementExact,
    ReleaseExactStatus,
    ReleaseWrongStatus,
    PhysicalProposalWidth(usize),
    SettlementProposalWidth(usize),
    MixedProposalWidth,
}

#[derive(Default)]
struct NativeTrace {
    starts: usize,
    ready: usize,
    shutdowns: usize,
    native_operations: Vec<Operation>,
    logical: Vec<LogicalBatch>,
    physical: Vec<CapsuleSet>,
    settlement_responses: Vec<Vec<u8>>,
    native_kv: std::collections::BTreeMap<(u32, String, u64), u32>,
}

struct ScriptedStage {
    mode: ResponseMode,
    role: NodeRole,
    trace: Arc<Mutex<NativeTrace>>,
    next_execution: u64,
}

// Decode the observed P4ID wire independently of the production codec. Fault
// responses retain a valid identity prefix so body-length/role tests really
// reach their original parser/semantic checks instead of failing the new gate.
fn observe_control_identity(body: &[u8]) -> Result<(usize, u32, u64, String), String> {
    if body.len() < 44 || &body[..8] != b"P4ID\x01\x00\x00\x00" {
        return Err("scripted control has no P4ID v1 identity".into());
    }
    let incarnation = u64::from_le_bytes(body[16..24].try_into().unwrap());
    let slot = u32::from_le_bytes(body[32..36].try_into().unwrap());
    let mut cursor = 36;
    let mut read_string = || -> Result<String, String> {
        let length = u32::from_le_bytes(
            body.get(cursor..cursor + 4)
                .ok_or("short P4ID length")?
                .try_into()
                .unwrap(),
        ) as usize;
        cursor += 4;
        let end = cursor.checked_add(length).ok_or("P4ID length overflow")?;
        let value = std::str::from_utf8(body.get(cursor..end).ok_or("short P4ID string")?)
            .map_err(|error| error.to_string())?
            .to_owned();
        cursor = end;
        Ok(value)
    };
    let session = read_string()?;
    let key = read_string()?;
    assert_eq!(session, "session");
    let (key_session, key_request) = key.split_once('\0').ok_or("noncanonical scripted key")?;
    assert_eq!(key_session, session);
    assert!(!key_request.is_empty() && !key_request.contains('\0'));
    assert_eq!(key, request_key(&session, key_request));
    assert_eq!(u64::from_le_bytes(body[8..16].try_into().unwrap()), 1);
    assert_ne!(incarnation, 0);
    assert_ne!(u64::from_le_bytes(body[24..32].try_into().unwrap()), 0);
    // The caller additionally requires this exact (slot, canonical key,
    // incarnation) in native_kv, populated by an actual PHYSICAL batch. This
    // generalizes the old single-request fixture without accepting aliases.
    Ok((cursor, slot, incarnation, key))
}

impl ServerControl for ScriptedStage {
    fn start(&mut self) -> Result<(), String> {
        self.trace.lock().unwrap().starts += 1;
        Ok(())
    }

    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        self.trace.lock().unwrap().ready += 1;
        Ok(Some(ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "scripted-stage".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 1024,
            n_batch: 4,
            n_ubatch: 2,
            n_seq_max: 8,
            physical_result_payload_bytes: 0,
            physical_result_tensor_count: 0,
            max_physical_result_bytes: 33_554_432,
            upstream_commit: "fixture-only".into(),
            patch_set: "fixture-only".into(),
            backend_inventory: "fixture-only-no-engine".into(),
            stage_wire_abi: "unknown".into(),
        }))
    }

    fn request(&mut self, request: Frame) -> Result<Frame, String> {
        self.trace
            .lock()
            .unwrap()
            .native_operations
            .push(request.header.operation);
        if request.header.operation == Operation::PhysicalSettle {
            // A successful opcode is deliberately not enough: the native
            // mutation already happened before these malformed bodies arrive.
            let (prefix_end, slot, incarnation, key) = observe_control_identity(&request.body)?;
            let prefix = request.body[..prefix_end].to_vec();
            let retain_from = u32::from_le_bytes(
                request
                    .body
                    .get(prefix_end..prefix_end + 4)
                    .ok_or("short scripted settle")?
                    .try_into()
                    .unwrap(),
            );
            let mut trace = self.trace.lock().unwrap();
            let occupied = trace
                .native_kv
                .get_mut(&(slot, key, incarnation))
                .ok_or("scripted settlement has no native KV")?;
            *occupied = (*occupied).min(retain_from);
            drop(trace);
            let body = match self.mode {
                ResponseMode::SettlementShortBody => {
                    let mut body = prefix;
                    body.extend_from_slice(&[0, 0]);
                    body
                }
                ResponseMode::SettlementWrongLength => {
                    let mut body = prefix;
                    body.extend_from_slice(&2_u32.to_le_bytes());
                    body.extend_from_slice(&23_i32.to_le_bytes());
                    body
                }
                ResponseMode::SettlementUnexpectedProposal => {
                    let mut body = prefix;
                    body.extend_from_slice(&1_u32.to_le_bytes());
                    body.extend_from_slice(&23_i32.to_le_bytes());
                    body
                }
                ResponseMode::SettlementExact => {
                    let mut body = prefix;
                    if self.role == NodeRole::Last {
                        body.extend_from_slice(&2_u32.to_le_bytes());
                        body.extend_from_slice(&23_i32.to_le_bytes());
                        body.extend_from_slice(&29_i32.to_le_bytes());
                    } else {
                        body.extend_from_slice(&0_u32.to_le_bytes());
                    }
                    body
                }
                ResponseMode::SettlementProposalWidth(width) => {
                    let mut body = prefix;
                    body.extend_from_slice(&(width as u32).to_le_bytes());
                    for token in &[23_i32, 29, 31][..width] {
                        body.extend_from_slice(&token.to_le_bytes());
                    }
                    body
                }
                _ => return Err("unexpected scripted settlement".into()),
            };
            self.trace
                .lock()
                .unwrap()
                .settlement_responses
                .push(body.clone());
            return Frame::new(Operation::PhysicalSettle, body).map_err(|error| error.to_string());
        }
        if request.header.operation == Operation::PhysicalRelease {
            let (prefix_end, slot, incarnation, key) = observe_control_identity(&request.body)?;
            assert_eq!(prefix_end, request.body.len(), "release has identity only");
            self.trace
                .lock()
                .unwrap()
                .native_kv
                .remove(&(slot, key, incarnation))
                .ok_or("scripted release has no native KV")?;
            let body = match self.mode {
                ResponseMode::ReleaseExactStatus => request.body.clone(),
                ResponseMode::ReleaseWrongStatus => b"NOT_RELEASED".to_vec(),
                _ => return Err("unexpected scripted release".into()),
            };
            return Frame::new(Operation::PhysicalRelease, body).map_err(|error| error.to_string());
        }
        if request.header.operation == Operation::Tokenize {
            return Frame::new(Operation::Tokenized, request.body)
                .map_err(|error| error.to_string());
        }
        if request.header.operation == Operation::PhysicalBatch {
            let mut set =
                CapsuleSet::decode(&request.body).map_err(|error| format!("{error:?}"))?;
            for capsule in &mut set.0 {
                for owner in &capsule.owners {
                    let mut trace = self.trace.lock().unwrap();
                    let occupied = trace
                        .native_kv
                        .entry((
                            owner.sequence_id,
                            owner.sequence_key.clone(),
                            owner.incarnation,
                        ))
                        .or_default();
                    *occupied = (*occupied).max(owner.position + 1);
                }
                if self.role == NodeRole::Last {
                    *capsule = completed_capsule(capsule.clone());
                    if matches!(
                        self.mode,
                        ResponseMode::SettlementShortBody
                            | ResponseMode::SettlementWrongLength
                            | ResponseMode::SettlementUnexpectedProposal
                            | ResponseMode::SettlementExact
                            | ResponseMode::SettlementProposalWidth(_)
                    ) {
                        for outcome in &mut capsule.outcomes {
                            if capsule.owners[outcome.owner_index as usize].phase == Phase::Prefill
                            {
                                outcome.proposal = vec![23, 29];
                            }
                        }
                    }
                    if let Some(width) = match self.mode {
                        ResponseMode::PhysicalProposalWidth(width) => Some(width),
                        ResponseMode::MixedProposalWidth => {
                            Some(if capsule.owners[0].sequence_id == 0 {
                                2
                            } else {
                                3
                            })
                        }
                        _ => None,
                    } {
                        for outcome in &mut capsule.outcomes {
                            if capsule.owners[outcome.owner_index as usize].phase != Phase::Verify {
                                outcome.proposal = [23, 29, 31][..width].to_vec();
                            }
                        }
                    }
                }
            }
            self.trace.lock().unwrap().physical.push(set.clone());
            return Frame::new(
                Operation::PhysicalResult,
                set.encode().map_err(|error| format!("{error:?}"))?,
            )
            .map_err(|error| error.to_string());
        }
        if request.header.operation != Operation::LogicalBatch {
            return Err(format!(
                "unexpected scripted operation {:?}",
                request.header.operation
            ));
        }
        let logical = LogicalBatch::decode(&request.body).map_err(|error| format!("{error:?}"))?;
        self.trace.lock().unwrap().logical.push(logical.clone());
        let mut native_rows = logical.0;
        if matches!(self.mode, ResponseMode::OmittedRow) {
            // The body remains codec-valid. Only issued-membership validation
            // can detect that the native result dropped a submitted row.
            native_rows.pop();
        }
        let mut capsules = Vec::new();
        for chunk in native_rows.chunks(2) {
            let owners: Vec<_> = chunk.iter().map(|row| row.owner.clone()).collect();
            let first_sequence = owners[0].sequence_id;
            assert!(
                owners
                    .iter()
                    .all(|owner| owner.sequence_id == first_sequence),
                "this narrow native fixture models a single-sequence physical batch"
            );
            let invocation = Invocation {
                flags: 0,
                n_seq_tokens: owners.len() as u32,
                n_seqs: 1,
                n_seqs_unq: 1,
                n_pos: 1,
                positions: owners.iter().map(|owner| owner.position as i32).collect(),
                sequence_counts: vec![1; owners.len()],
                sequence_ids: vec![first_sequence as i32; owners.len()],
                output: owners.iter().map(|owner| owner.output).collect(),
            };
            capsules.push(PhysicalCapsule {
                execution_id: self.next_execution,
                terminal: false,
                invocation,
                owners,
                // Opaque transfer bytes satisfy the real capsule codec, not
                // a claim about a native tensor calculation or layout.
                tensors: vec![Tensor {
                    descriptor: TensorDescriptor {
                        tensor_type: 0,
                        dimensions: vec![1],
                        strides: vec![4],
                        nbytes: 4,
                        view_offset: 0,
                        alias_of: None,
                        name: "fixture-output".into(),
                    },
                    data: vec![0; 4],
                }],
                outcomes: Vec::new(),
            });
            self.next_execution += 1;
        }
        let physical = CapsuleSet(capsules);
        self.trace.lock().unwrap().physical.push(physical.clone());
        if matches!(self.mode, ResponseMode::LostAfterExecution) {
            return Err("injected response loss after the recorded native attempt".into());
        }
        Frame::new(
            Operation::PhysicalResult,
            physical.encode().map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| error.to_string())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.trace.lock().unwrap().shutdowns += 1;
        Ok(())
    }
}

struct Fixture {
    worker: Worker,
    mailbox: Arc<CompletionMailbox>,
    trace: Arc<Mutex<NativeTrace>>,
}

#[test]
fn service_budget_actual_handler_checks_route_issue_identity_and_never_retires_flight() {
    use crate::v2::scheduler::service::ServiceBudget;
    use crate::v2::{SERVICE_SAMPLE_CONTENT_TYPE, ServiceSample};
    let mut head = fixture(ResponseMode::ExactSplit);
    head.worker.state.max_open_batches = 4;
    head.worker.state.pipeline_policy = Some(crate::v2::scheduler::pipeline::PipelinePolicy {
        mixed_batch_rows: None,
        mixed_prefill_rows: 2,
    });
    head.worker.service_budget = ServiceBudget::new(150);
    head.handle(submission("request", vec![7; 8])).unwrap();
    head.worker.drive_first_batches().unwrap();
    let physical = forwarded(&head.mailbox);
    let sample = ServiceSample {
        load_generation: 1,
        session_id: "session".into(),
        execution_ids: physical.0.iter().map(|c| c.execution_id).collect(),
        stage_index: 1,
        shape: super::service::physical_shape(&physical).unwrap(),
        rpc_us: 100,
    };
    let mut event = input("sample", vec![]);
    event.envelope.payload_content_type = SERVICE_SAMPLE_CONTENT_TYPE.into();
    event.envelope.target = head.worker.endpoint.clone();
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "imposter", 1);
    event.payload = serde_json::to_vec(&sample).unwrap();
    let original = head.worker.service_budget.clone();
    let flights = head.worker.state.flights.clone();
    let open = head.worker.state.open_batches.clone();
    assert!(head.worker.service_sample(&event).is_err());
    assert_eq!(head.worker.service_budget, original);
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "last", 1);
    let mut bad = sample.clone();
    bad.shape.prefill_rows += 1;
    event.payload = serde_json::to_vec(&bad).unwrap();
    assert!(head.worker.service_sample(&event).is_err());
    assert_eq!(head.worker.service_budget, original);
    event.payload = serde_json::to_vec(&sample).unwrap();
    head.worker.service_sample(&event).unwrap();
    let accepted = head.worker.service_budget.clone();
    assert_ne!(accepted, original);
    head.worker.service_sample(&event).unwrap();
    assert_eq!(
        head.worker.service_budget, accepted,
        "duplicate must not train twice"
    );
    let mut conflict = sample;
    conflict.rpc_us += 1;
    event.payload = serde_json::to_vec(&conflict).unwrap();
    assert!(head.worker.service_sample(&event).is_err());
    assert_eq!(head.worker.service_budget, accepted);
    assert_eq!(head.worker.state.flights, flights);
    assert_eq!(head.worker.state.open_batches, open);
    assert_eq!(request(&head).outstanding, 1);
    assert_eq!(native_attempts(&head), 1);
}

#[test]
fn service_budget_actual_unload_ignores_late_cost_without_reopening_state() {
    use crate::v2::scheduler::service::ServiceBudget;
    use crate::v2::{SERVICE_SAMPLE_CONTENT_TYPE, ServiceSample, ServiceShape};
    let mut head = fixture(ResponseMode::ExactSplit);
    head.worker.service_budget = ServiceBudget::new(150);
    head.worker.state.last_load_generation = 1;
    let sample = ServiceSample {
        load_generation: 1,
        session_id: "session".into(),
        execution_ids: vec![1],
        stage_index: 0,
        shape: ServiceShape {
            prefill_rows: 4,
            decode_rows: 0,
            members: 1,
            last_position: 3,
        },
        rpc_us: 10,
    };
    // Seed only optimization history; native, flight and KV authority stay idle.
    head.worker
        .service_budget
        .register(sample.clone(), 1, 2, &Default::default());
    let mut unload = input("unload", vec![]);
    unload.payload = serde_json::to_vec(&UnloadCommand { load_generation: 1 }).unwrap();
    unload.envelope.payload_content_type = UNLOAD_CONTENT_TYPE.into();
    head.handle(unload).unwrap();
    assert_eq!(head.worker.state.load_generation, 0);
    assert!(head.worker.state.sessions.is_empty());
    assert_eq!(head.worker.service_budget, ServiceBudget::new(150));
    assert_eq!(head.trace.lock().unwrap().shutdowns, 1);
    let mut sample = sample;
    sample.stage_index = 1;
    let mut event = input("late-sample", vec![]);
    event.payload = serde_json::to_vec(&sample).unwrap();
    event.envelope.payload_content_type = SERVICE_SAMPLE_CONTENT_TYPE.into();
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "last", 1);
    head.handle(event).unwrap();
    assert_eq!(head.worker.service_budget, ServiceBudget::new(150));
    assert_eq!(head.worker.state.load_generation, 0);
    assert!(head.worker.state.sessions.is_empty());
    assert_eq!(native_attempts(&head), 0);
    assert_eq!(head.trace.lock().unwrap().shutdowns, 1);
}

impl Fixture {
    // Only address the chosen receiver; do not repair source/body identity.
    // These tests retain their pre-existing native/body/frontier oracles.
    fn handle(&mut self, mut event: Event) -> Result<(), ()> {
        event.envelope.target = self.worker.endpoint.clone();
        self.worker.handle(event)
    }
}

fn fixture(mode: ResponseMode) -> Fixture {
    fixture_at(mode, NodeRole::First)
}

fn fixture_at(mode: ResponseMode, role: NodeRole) -> Fixture {
    let address = Address::tcp("127.0.0.1", 42001);
    let node = match role {
        NodeRole::First => "first",
        NodeRole::Middle => "middle",
        NodeRole::Last => "last",
    };
    let endpoint = Endpoint::node(address, node, 1);
    let (_sender, receiver) = mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(64);
    let trace = Arc::new(Mutex::new(NativeTrace::default()));
    let mut worker = Worker::new(
        endpoint,
        receiver,
        publisher,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    )
    .with_stage_for_test(Box::new(ScriptedStage {
        mode,
        role,
        trace: trace.clone(),
        next_execution: 1,
    }))
    .unwrap();
    // LOAD is deliberately not simulated. Pin only the negotiated fields
    // needed by these post-load worker paths and exercise SESSION normally.
    worker.state.load_generation = 1;
    worker.state.physical_receives =
        crate::v2::node::physical_receive::PhysicalReceiveLedger::new(1).unwrap();
    worker.state.batch_capacity = 4;
    worker.state.physical_capacity = 2;
    worker.state.context_size = 128;
    worker.state.sequence_capacity = 8;
    worker.state.free_sequences = (0..8).collect();
    worker.state.max_atomic_sequences = 1;
    worker.state.min_batch_rows = 0;
    worker.state.max_issue_rows = 0;
    worker.state.max_open_batches = 0;
    worker.state.prefill_fragments = 1;
    let session = SessionCommand {
        load_generation: 1,
        session_id: "session".into(),
        stages: if role == NodeRole::Middle {
            vec!["first", "middle", "last"]
        } else {
            vec!["first", "last"]
        }
        .into_iter()
        .map(|node| NodeAddress {
            agent: Address::tcp("127.0.0.1", 42001).to_string(),
            node: node.into(),
            generation: 1,
        })
        .collect(),
        stage_index: usize::from(role != NodeRole::First),
    };
    let mut event = input("session", vec![1]);
    event.envelope.target = worker.endpoint.clone();
    event.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&session).unwrap();
    worker.handle(event).unwrap();
    let session_events = drain(&mailbox);
    assert!(
        worker.state.sessions.contains_key("session"),
        "SESSION rejected: {session_events:?}"
    );
    assert_eq!(
        (
            trace.lock().unwrap().starts,
            worker.lifecycle.physical_batch_capable()
        ),
        (1, true)
    );
    Fixture {
        worker,
        mailbox,
        trace,
    }
}

fn input(name: &str, tokens: Vec<i32>) -> Event {
    let mut request = crate::v2::tests::request_state(tokens);
    request.input_mut_for_test().command.request_id = name.into();
    let mut event = request.template.clone();
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "first", 1);
    event.envelope.event_id = format!("input-{name}");
    event.envelope.correlation_id = name.into();
    event.envelope.payload_content_type = PREFILL_CONTENT_TYPE.into();
    event.envelope.return_route = Some(OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 42001),
        channel: "reply".into(),
        connection_generation: 1,
    });
    event.payload = serde_json::to_vec(&request.command).unwrap();
    event
}

/// Only real request submissions use OUTER authority. Stage/control builders
/// keep their independently declared first/last source, including bad-source
/// probes; the generic event receiver must never normalize incoming identity.
fn submission(name: &str, tokens: Vec<i32>) -> Event {
    let mut event = input(name, tokens);
    event.envelope.source = Endpoint::Outer(event.envelope.return_route.clone().unwrap());
    event.envelope.target = Endpoint::node(Address::tcp("127.0.0.1", 42001), "first", 1);
    event
}

fn drain(mailbox: &CompletionMailbox) -> Vec<Event> {
    let mut events = Vec::new();
    while let Poll::Event(event) = mailbox.try_take() {
        events.push(event);
    }
    events
}

fn drain_owned(mailbox: &CompletionMailbox) -> Vec<Event> {
    let mut events = Vec::new();
    while let OwnedPoll::Event(completion) = mailbox.try_take_owned() {
        events.push(completion.event().clone());
        completion.retire();
    }
    events
}

fn install_owned_completion_budget(fixture: &mut Fixture, retained_count: usize, bytes: usize) {
    let (publisher, mailbox) =
        completion_mailbox_with_limits(retained_count.max(1), retained_count, bytes).unwrap();
    fixture.worker.publisher = publisher;
    fixture.worker.owned_completions = true;
    fixture
        .worker
        .state
        .resource_profile
        .as_mut()
        .expect("stage fixture has a resource profile")
        .max_completion_retained_bytes = bytes as u64;
    fixture.mailbox = mailbox;
}

#[test]
fn b2_first_native_reserves_forward_and_every_observation_before_issue() {
    let mut fixture = fixture(ResponseMode::ExactSplit);
    install_owned_completion_budget(&mut fixture, 8, 1 << 20);
    fixture
        .handle(submission("request", vec![3, 5, 7, 11]))
        .unwrap();
    fixture.worker.drive_first_batches().unwrap();
    assert_eq!(native_attempts(&fixture), 1);
    let events = drain_owned(&fixture.mailbox);
    assert_eq!(events.len(), 3);
    assert_eq!(
        events
            .iter()
            .map(|event| event.envelope.payload_content_type.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        [
            PHYSICAL_BATCH_CONTENT_TYPE,
            BATCH_OBSERVATION_CONTENT_TYPE,
            STAGE_SPAN_CONTENT_TYPE,
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(fixture.mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(fixture.mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn b2_first_reservation_refusal_precedes_native_and_every_issue_commit() {
    let mut fixture = fixture(ResponseMode::ExactSplit);
    install_owned_completion_budget(&mut fixture, 2, 1 << 20);
    fixture
        .handle(submission("request", vec![3, 5, 7, 11]))
        .unwrap();
    let event_id = fixture.worker.state.next_event;
    let pending = fixture.worker.state.pending.clone();
    let free = fixture.worker.state.free_sequences.clone();
    assert!(fixture.worker.drive_first_batches().is_err());
    assert_eq!(native_attempts(&fixture), 0);
    assert_eq!(fixture.worker.state.next_event, event_id);
    assert_eq!(fixture.worker.state.pending, pending);
    assert_eq!(fixture.worker.state.free_sequences, free);
    assert!(fixture.worker.state.prepared_issue.is_none());
    assert!(fixture.worker.state.open_batches.is_empty());
    assert_eq!(request(&fixture).outstanding, 0);
    assert!(fixture.worker.effects.is_empty());
    assert_eq!(fixture.mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(fixture.mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn b2_middle_native_moves_its_reserved_forward_and_span_to_owned_storage() {
    let mut fixture = fixture_at(ResponseMode::ExactSplit, NodeRole::Middle);
    install_owned_completion_budget(&mut fixture, 8, 1 << 20);
    fixture.handle(new_physical_input()).unwrap();
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        vec![Operation::PhysicalBatch]
    );
    let events = drain_owned(&fixture.mailbox);
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .any(|event| { event.envelope.payload_content_type == PHYSICAL_BATCH_CONTENT_TYPE })
    );
    assert!(
        events
            .iter()
            .any(|event| { event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE })
    );
    assert_eq!(fixture.mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(fixture.mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn b2_middle_reservation_refusal_preserves_receive_ledger_and_native_kv() {
    let mut fixture = fixture_at(ResponseMode::ExactSplit, NodeRole::Middle);
    install_owned_completion_budget(&mut fixture, 1, 1 << 20);
    let event_id = fixture.worker.state.next_event;
    let owners = fixture.worker.state.stage_owners.clone();
    let frontiers = format!("{:?}", fixture.worker.state.stage_frontiers);
    let physical_receives = format!("{:?}", fixture.worker.state.physical_receives);
    let receive_status = fixture.worker.state.physical_receives.shutdown_status();
    let mut event = new_physical_input();
    event.envelope.target = fixture.worker.endpoint.clone();
    assert!(fixture.worker.physical(&event).is_err());
    assert!(fixture.trace.lock().unwrap().native_operations.is_empty());
    assert!(fixture.trace.lock().unwrap().native_kv.is_empty());
    assert_eq!(fixture.worker.state.next_event, event_id);
    assert_eq!(fixture.worker.state.stage_owners, owners);
    assert_eq!(
        format!("{:?}", fixture.worker.state.stage_frontiers),
        frontiers
    );
    assert_eq!(
        format!("{:?}", fixture.worker.state.physical_receives),
        physical_receives
    );
    assert_eq!(
        fixture.worker.state.physical_receives.shutdown_status(),
        receive_status
    );
    assert!(fixture.worker.effects.is_empty());
    assert_eq!(fixture.mailbox.storage_snapshot().retained_count, 0);
    assert_eq!(fixture.mailbox.storage_snapshot().retained_bytes, 0);
}

#[test]
fn b3_adapter_snapshot_separates_pending_request_and_native_response_storage() {
    let mut fixture = fixture(ResponseMode::ExactSplit);
    let tracker = fixture.worker.retention_tracker();
    fixture
        .handle(submission("retention-request", vec![3, 5, 7, 11]))
        .unwrap();
    fixture.worker.sync_pending_retention();
    let pending = tracker.snapshot();
    assert_eq!(pending.pending_requests.count, 1);
    assert!(pending.pending_requests.bytes > 0);
    assert_eq!(pending.native_responses.count, 0);

    let response = fixture
        .worker
        .stage_request(
            Operation::Tokenize,
            Operation::Tokenized,
            b"storage".to_vec(),
        )
        .unwrap();
    let native = tracker.snapshot();
    assert_eq!(native.pending_requests, pending.pending_requests);
    assert_eq!(native.native_responses.count, 1);
    assert_eq!(native.native_responses.bytes, response.capacity());
    drop(response);
    assert_eq!(tracker.snapshot().native_responses.count, 0);
}

fn forwarded(mailbox: &CompletionMailbox) -> CapsuleSet {
    let mut forwards: Vec<_> = drain(mailbox)
        .into_iter()
        .filter(|event| event.envelope.payload_content_type == PHYSICAL_BATCH_CONTENT_TYPE)
        .collect();
    assert_eq!(
        forwards.len(),
        1,
        "one accepted logical issue has one forwarded CapsuleSet"
    );
    CapsuleSet::decode(&forwards.remove(0).payload).unwrap()
}

fn completed_capsule(mut capsule: PhysicalCapsule) -> PhysicalCapsule {
    capsule.terminal = true;
    capsule.tensors.clear();
    for (index, owner) in capsule.owners.iter().enumerate() {
        if owner.phase == Phase::Verify {
            if owner.speculative_index == 0 {
                // The fake's deterministic partial acceptance is one token
                // from a two-row Verify. It leaves a real pending trim at
                // this stage, rather than authorizing SETTLE after Prefill.
                assert_eq!(owner.speculative_count, 2);
                capsule.outcomes.push(PhysicalOutcome {
                    owner_index: index as u32,
                    generated: vec![GeneratedToken {
                        token: 23,
                        text: "fixture verified response".into(),
                        position: owner.position + 1,
                        stop: None,
                    }],
                    proposal: Vec::new(),
                    retain_from: Some(owner.position + 1),
                    replay_tokens: Vec::new(),
                    replay_position: 0,
                });
            }
            continue;
        }
        if owner.output {
            capsule.outcomes.push(PhysicalOutcome {
                owner_index: index as u32,
                generated: vec![GeneratedToken {
                    token: 23,
                    text: "fixture response".into(),
                    position: owner.position + 1,
                    stop: None,
                }],
                proposal: vec![23],
                retain_from: None,
                replay_tokens: Vec::new(),
                replay_position: 0,
            });
        }
    }
    capsule
}

fn terminal(capsule: PhysicalCapsule) -> Event {
    let capsule = completed_capsule(capsule);
    let mut event = input("tail", vec![1]);
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "last", 1);
    event.envelope.event_id = format!("terminal-{}", capsule.execution_id);
    event.envelope.payload_content_type = TAIL_BATCH_CONTENT_TYPE.into();
    event.payload = CapsuleSet(vec![capsule]).encode().unwrap();
    event
}

fn request(fixture: &Fixture) -> &RequestState {
    &fixture.worker.state.requests[&request_key("session", "request")]
}

fn native_attempts(fixture: &Fixture) -> usize {
    fixture.trace.lock().unwrap().logical.len()
}

#[test]
fn real_worker_conserves_native_split_rows_and_settles_only_the_complete_fragment() {
    let mut fixture = fixture(ResponseMode::ExactSplit);
    let tokens = vec![3, 5, 7, 11];
    fixture
        .handle(submission("request", tokens.clone()))
        .unwrap();
    fixture.worker.drive_first_batches().unwrap();
    let set = forwarded(&fixture.mailbox);
    assert_eq!(set.0.len(), 2);
    let owners: Vec<_> = set.0.iter().flat_map(|capsule| &capsule.owners).collect();
    assert_eq!(
        owners
            .iter()
            .map(|owner| owner.input_token)
            .collect::<Vec<_>>(),
        tokens
    );
    assert_eq!(
        owners
            .iter()
            .map(|owner| owner.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    assert_eq!(native_attempts(&fixture), 1);
    assert_eq!(
        (
            request(&fixture).prompt_issued,
            request(&fixture).prompt_cursor,
            request(&fixture).outstanding
        ),
        (4, 0, 1)
    );
    assert_eq!(
        fixture
            .worker
            .state
            .open_batches
            .values()
            .next()
            .unwrap()
            .len(),
        2
    );
    assert!(fixture.worker.state.prepared_issue.is_none());

    // The decision-bearing last piece arrives first. It is only a receipt:
    // request progress/output must wait for its other physical member.
    fixture.handle(terminal(set.0[1].clone())).unwrap();
    assert_eq!(
        (
            request(&fixture).prompt_cursor,
            request(&fixture).outstanding,
            request(&fixture).generated
        ),
        (0, 1, 0)
    );
    assert!(
        drain(&fixture.mailbox)
            .iter()
            .all(|event| event.envelope.payload_content_type != OUTPUT_CONTENT_TYPE)
    );
    fixture.handle(terminal(set.0[0].clone())).unwrap();
    assert_eq!(
        (
            request(&fixture).prompt_cursor,
            request(&fixture).outstanding,
            request(&fixture).generated
        ),
        (4, 0, 1)
    );
    assert!(fixture.worker.state.open_batches.is_empty());
    let outputs: Vec<_> = drain(&fixture.mailbox)
        .into_iter()
        .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
        .collect();
    assert_eq!(outputs.len(), 1);
    let output: serde_json::Value = serde_json::from_slice(&outputs[0].payload).unwrap();
    assert_eq!(
        (output["token"].as_i64(), output["position"].as_u64()),
        (Some(23), Some(4))
    );
    fixture.worker.lifecycle.unload().unwrap();
    assert_eq!(fixture.trace.lock().unwrap().shutdowns, 1);
}

#[test]
fn real_worker_keeps_native_response_loss_uncertain_and_never_reissues() {
    let mut fixture = fixture(ResponseMode::LostAfterExecution);
    fixture
        .handle(submission("request", vec![3, 5, 7, 11]))
        .unwrap();
    assert!(fixture.worker.drive_first_batches().is_err());
    let prepared = fixture.worker.state.prepared_issue.as_ref().unwrap();
    assert_eq!(prepared.progress, IssueProgress::Uncertain);
    assert_eq!(prepared.logical, fixture.trace.lock().unwrap().logical[0]);
    assert_eq!(
        (
            request(&fixture).prompt_issued,
            request(&fixture).outstanding
        ),
        (0, 0)
    );
    assert!(fixture.worker.state.open_batches.is_empty());
    assert!(fixture.worker.effects_fenced);
    for _ in 0..3 {
        assert!(fixture.worker.drive_first_batches().is_err());
    }
    assert_eq!(native_attempts(&fixture), 1);
    assert!(
        drain(&fixture.mailbox)
            .iter()
            .all(|event| event.envelope.payload_content_type != PHYSICAL_BATCH_CONTENT_TYPE)
    );
}

#[test]
fn real_worker_rejects_codec_valid_missing_rows_without_committing_issue_counters() {
    let mut fixture = fixture(ResponseMode::OmittedRow);
    fixture
        .handle(submission("request", vec![3, 5, 7, 11]))
        .unwrap();
    assert!(fixture.worker.drive_first_batches().is_err());
    let prepared = fixture.worker.state.prepared_issue.as_ref().unwrap();
    assert_eq!(prepared.progress, IssueProgress::Uncertain);
    assert_eq!(prepared.logical.0.len(), 4);
    assert_eq!(prepared.logical, fixture.trace.lock().unwrap().logical[0]);
    assert_eq!(
        (
            request(&fixture).prompt_issued,
            request(&fixture).prompt_cursor,
            request(&fixture).outstanding
        ),
        (0, 0, 0)
    );
    assert_eq!(fixture.worker.state.next_open_batch, 1);
    assert!(fixture.worker.state.open_batches.is_empty());
    assert_eq!(
        fixture.trace.lock().unwrap().physical[0]
            .0
            .iter()
            .map(|capsule| capsule.owners.len())
            .sum::<usize>(),
        3
    );
    assert!(fixture.worker.drive_first_batches().is_err());
    assert_eq!(native_attempts(&fixture), 1);
    assert!(
        drain(&fixture.mailbox)
            .iter()
            .all(|event| event.envelope.payload_content_type != PHYSICAL_BATCH_CONTENT_TYPE)
    );
}

#[test]
fn real_worker_open_batch_gate_waits_for_the_last_physical_member() {
    let mut fixture = fixture(ResponseMode::ExactSplit);
    fixture.worker.state.max_open_batches = 1;
    fixture.worker.state.prefill_fragments = 2;
    fixture.handle(submission("request", vec![7; 8])).unwrap();
    fixture.worker.drive_first_batches().unwrap();
    assert_eq!(
        native_attempts(&fixture),
        1,
        "remaining prompt is runnable but the batch gate must hold it"
    );
    assert_eq!(request(&fixture).prompt_issued, 4);
    let set = forwarded(&fixture.mailbox);
    assert_eq!(set.0.len(), 2);
    fixture.handle(terminal(set.0[0].clone())).unwrap();
    fixture.worker.drive_first_batches().unwrap();
    assert_eq!(
        native_attempts(&fixture),
        1,
        "one physical receipt is not a retired logical batch"
    );
    assert_eq!(
        (
            request(&fixture).prompt_cursor,
            request(&fixture).outstanding
        ),
        (0, 1)
    );
    fixture.handle(terminal(set.0[1].clone())).unwrap();
    assert_eq!(
        (
            request(&fixture).prompt_cursor,
            request(&fixture).outstanding
        ),
        (4, 0)
    );
    fixture.worker.drive_first_batches().unwrap();
    assert_eq!(native_attempts(&fixture), 2);
    assert_eq!(request(&fixture).prompt_issued, 8);
    let next = forwarded(&fixture.mailbox);
    assert_eq!(
        next.0
            .iter()
            .flat_map(|capsule| &capsule.owners)
            .map(|owner| owner.position)
            .collect::<Vec<_>>(),
        vec![4, 5, 6, 7]
    );
}

fn settlement_input() -> Event {
    let mut event = input("settlement", vec![1]);
    event.envelope.payload_content_type = SETTLE_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&SettlementCommand {
        load_generation: 1,
        session_id: "session".into(),
        sequences: vec![SettlementSequence {
            incarnation: 1,
            operation_id: 1,
            key: request_key("session", "request"),
            id: 0,
            retain_from: 5,
            replay_tokens: Vec::new(),
            replay_position: 0,
            proposal: Vec::new(),
        }],
    })
    .unwrap();
    event
}

fn release_input() -> Event {
    let mut event = input("release", vec![1]);
    event.envelope.payload_content_type = RELEASE_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&ReleaseCommand {
        load_generation: 1,
        session_id: "session".into(),
        sequences: vec![ReleaseSequence {
            incarnation: 1,
            operation_id: 1,
            key: request_key("session", "request"),
            id: 0,
        }],
    })
    .unwrap();
    event
}

fn new_physical_input() -> Event {
    // Reuse the real head path to construct a valid later PHYSICAL request,
    // so a missing fence cannot be hidden by a malformed next input.
    let mut head = fixture(ResponseMode::ExactSplit);
    head.handle(submission("request", vec![3, 5, 7, 11]))
        .unwrap();
    head.worker.drive_first_batches().unwrap();
    let set = forwarded(&head.mailbox);
    let mut event = input("new-physical", vec![1]);
    event.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
    event.payload = set.encode().unwrap();
    event
}

fn establish_native_owner(fixture: &mut Fixture) {
    assert!(fixture.trace.lock().unwrap().native_operations.is_empty());
    fixture.handle(new_physical_input()).unwrap();
    assert!(!fixture.worker.effects_fenced);
    let events = drain(&fixture.mailbox);
    assert!(
        events.iter().any(|event| matches!(
            event.envelope.payload_content_type.as_str(),
            PHYSICAL_BATCH_CONTENT_TYPE | TAIL_BATCH_CONTENT_TYPE
        )),
        "warmup must really execute and forward physical work: {events:?}"
    );
    let trace = fixture.trace.lock().unwrap();
    assert_eq!(trace.native_operations, vec![Operation::PhysicalBatch]);
    assert_eq!(
        trace
            .native_kv
            .get(&(0, request_key("session", "request"), 1)),
        Some(&4)
    );
}

fn append_native_verify(fixture: &mut Fixture, key: &str, generated_tokens: u32) {
    append_native_proposal(fixture, key, generated_tokens, &[23, 29]);
}

fn append_native_proposal(fixture: &mut Fixture, key: &str, generated_tokens: u32, tokens: &[i32]) {
    assert!(!tokens.is_empty() && tokens.len() <= fixture.worker.state.physical_capacity);
    let count = tokens.len() as u32;
    let phase = if count == 1 {
        Phase::Decode
    } else {
        Phase::Verify
    };
    // This is an explicit incoming wire fixture, not a fabricated ownership
    // or frontier map. The real receiving worker decodes, admits and executes
    // it after its earlier Prefill/SETTLE. Head Verify construction is covered
    // separately; this fixture is scoped to control's actual stage consumer.
    let (mut capsule, execution_id, start) = {
        let trace = fixture.trace.lock().unwrap();
        let previous = trace
            .physical
            .iter()
            .flat_map(|set| &set.0)
            .rev()
            .find(|capsule| capsule.owners.iter().any(|owner| owner.sequence_key == key))
            .expect("verification follows actual native work");
        let owner = previous
            .owners
            .iter()
            .find(|owner| owner.sequence_key == key)
            .unwrap();
        let start = trace.native_kv[&(owner.sequence_id, key.to_owned(), owner.incarnation)];
        let execution = trace
            .physical
            .iter()
            .flat_map(|set| &set.0)
            .map(|capsule| capsule.execution_id)
            .max()
            .unwrap()
            + 1;
        let mut capsule = previous.clone();
        capsule.owners = vec![owner.clone(); tokens.len()];
        (capsule, execution, start)
    };
    capsule.execution_id = execution_id;
    capsule.terminal = false;
    capsule.outcomes.clear();
    for (offset, owner) in capsule.owners.iter_mut().enumerate() {
        owner.phase = phase;
        owner.position = start + offset as u32;
        owner.generated_tokens = generated_tokens;
        owner.output = true;
        owner.input_token = tokens[offset];
        owner.speculative_id = if count > 1 { execution_id } else { 0 };
        owner.speculative_index = if count > 1 { offset as u32 } else { 0 };
        owner.speculative_count = if count > 1 { count } else { 0 };
    }
    capsule.invocation = Invocation {
        flags: 0,
        n_seq_tokens: count,
        n_seqs: 1,
        n_seqs_unq: 1,
        n_pos: 1,
        positions: (start..start + count)
            .map(|position| position as i32)
            .collect(),
        sequence_counts: vec![1; tokens.len()],
        sequence_ids: vec![capsule.owners[0].sequence_id as i32; tokens.len()],
        output: vec![true; tokens.len()],
    };
    capsule.tensors = vec![Tensor {
        descriptor: TensorDescriptor {
            tensor_type: 0,
            dimensions: vec![1],
            strides: vec![4],
            nbytes: 4,
            view_offset: 0,
            alias_of: None,
            name: "fixture-output".into(),
        },
        data: vec![0; 4],
    }];
    let identity = super::super::ownership::Identity::from_owner(&capsule.owners[0]);
    let mut event = input("verification-warmup", vec![1]);
    event.envelope.event_id = format!("verification-{}-{execution_id}", identity.sequence_id);
    event.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
    event.payload = CapsuleSet(vec![capsule]).encode().unwrap();
    let before = fixture.trace.lock().unwrap().native_operations.clone();
    fixture.handle(event).unwrap();
    let events = drain(&fixture.mailbox);
    assert!(
        !fixture.worker.effects_fenced,
        "Verify warmup failed: {events:?}"
    );
    assert!(
        events
            .iter()
            .all(|event| event.envelope.payload_content_type != ERROR_CONTENT_TYPE),
        "Verify must reach the native consumer: {events:?}"
    );
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        [before, vec![Operation::PhysicalBatch]].concat()
    );
    assert_eq!(
        fixture.trace.lock().unwrap().native_kv[&(
            identity.sequence_id,
            identity.sequence_key,
            identity.incarnation
        )],
        start + count
    );
}

fn establish_native_verify(fixture: &mut Fixture) {
    establish_native_owner(fixture);
    append_native_verify(fixture, &request_key("session", "request"), 1);
}

fn assert_post_native_failure_fenced(
    mut fixture: Fixture,
    event: Event,
    operation: Operation,
    expected_detail: &str,
) {
    if operation == Operation::PhysicalSettle {
        establish_native_verify(&mut fixture);
    } else {
        establish_native_owner(&mut fixture);
    }
    let warmup = fixture.trace.lock().unwrap().native_operations.clone();
    let frontiers_before = format!("{:?}", fixture.worker.state.stage_frontiers);
    assert!(fixture.handle(event.clone()).is_err());
    assert!(fixture.worker.effects_fenced);
    assert_eq!(
        format!("{:?}", fixture.worker.state.stage_frontiers),
        frontiers_before,
        "a malformed native response cannot certify a new KV frontier"
    );
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        [warmup.clone(), vec![operation]].concat()
    );
    let rejected = drain(&fixture.mailbox);
    assert!(
        !rejected.is_empty(),
        "the original protocol failure must be reported"
    );
    assert!(
        rejected
            .iter()
            .all(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE),
        "a malformed native result cannot emit success, forwarding or output: {rejected:?}"
    );
    assert!(
        rejected
            .iter()
            .any(|event| std::str::from_utf8(&event.payload)
                .unwrap()
                .contains(expected_detail)),
        "the intended post-native failure must remain reachable: expected {expected_detail}, got {rejected:?}"
    );

    assert!(
        fixture.handle(event).is_err(),
        "re-delivery must stop at the worker fence"
    );
    assert!(
        fixture.handle(new_physical_input()).is_err(),
        "new physical work must stop at the worker fence"
    );
    assert!(fixture.worker.drive_first_batches().is_err());
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        [warmup, vec![operation]].concat(),
        "native may have mutated memory already; there must be zero additional calls"
    );
    assert!(
        drain(&fixture.mailbox).is_empty(),
        "entry-fenced work emits no success or extra event"
    );
}

#[test]
fn native_settlement_short_body_fences_middle_and_tail_before_any_further_call() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        assert_post_native_failure_fenced(
            fixture_at(ResponseMode::SettlementShortBody, role),
            settlement_input(),
            Operation::PhysicalSettle,
            "physical settlement result is truncated",
        );
    }
}

#[test]
fn native_settlement_wrong_length_fences_middle_and_tail_before_any_further_call() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        assert_post_native_failure_fenced(
            fixture_at(ResponseMode::SettlementWrongLength, role),
            settlement_input(),
            Operation::PhysicalSettle,
            "physical settlement result length is invalid",
        );
    }
}

#[test]
fn native_middle_settlement_proposal_fences_before_any_further_call() {
    assert_post_native_failure_fenced(
        fixture_at(ResponseMode::SettlementUnexpectedProposal, NodeRole::Middle),
        settlement_input(),
        Operation::PhysicalSettle,
        "frontier nonterminal or checkpoint SETTLE cannot produce a proposal",
    );
}

#[test]
fn native_release_wrong_status_fences_before_any_further_call() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        assert_post_native_failure_fenced(
            fixture_at(ResponseMode::ReleaseWrongStatus, role),
            release_input(),
            Operation::PhysicalRelease,
            "physical release acknowledgement is invalid",
        );
    }
}

#[test]
fn native_release_exact_status_produces_the_terminal_acknowledgement() {
    let mut fixture = fixture_at(ResponseMode::ReleaseExactStatus, NodeRole::Last);
    establish_native_owner(&mut fixture);
    fixture.handle(release_input()).unwrap();
    assert!(!fixture.worker.effects_fenced);
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        vec![Operation::PhysicalBatch, Operation::PhysicalRelease]
    );
    let events = drain(&fixture.mailbox);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].envelope.payload_content_type,
        RELEASED_CONTENT_TYPE
    );
    let command: ReleaseCommand = serde_json::from_slice(&events[0].payload).unwrap();
    assert_eq!(
        command.sequences,
        vec![ReleaseSequence {
            incarnation: 1,
            operation_id: 1,
            key: request_key("session", "request"),
            id: 0
        }]
    );
}

fn execute_valid_control(fixture: &mut Fixture, event: Event, operation: Operation) -> Event {
    let before = fixture.trace.lock().unwrap().native_operations.clone();
    fixture.handle(event).unwrap();
    assert!(!fixture.worker.effects_fenced);
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        [before, vec![operation]].concat()
    );
    let mut events = drain(&fixture.mailbox);
    assert_eq!(events.len(), 1);
    assert_ne!(events[0].envelope.payload_content_type, ERROR_CONTENT_TYPE);
    events.pop().unwrap()
}

fn assert_control_rejected_without_native_effect(
    fixture: &mut Fixture,
    event: Event,
    detail: &str,
) {
    let operations = fixture.trace.lock().unwrap().native_operations.clone();
    let kv = fixture.trace.lock().unwrap().native_kv.clone();
    let owners = fixture.worker.state.stage_owners.clone();
    let frontiers = format!("{:?}", fixture.worker.state.stage_frontiers);
    let effects = format!("{:?}", fixture.worker.effects);
    // handle reports a protocol refusal and remains usable. A valid native
    // execution has not happened, so this is not an Uncertain/fenced outcome.
    fixture.handle(event).unwrap();
    assert!(!fixture.worker.effects_fenced);
    assert_eq!(fixture.trace.lock().unwrap().native_operations, operations);
    assert_eq!(fixture.trace.lock().unwrap().native_kv, kv);
    assert_eq!(fixture.worker.state.stage_owners, owners);
    assert_eq!(
        format!("{:?}", fixture.worker.state.stage_frontiers),
        frontiers
    );
    assert_eq!(format!("{:?}", fixture.worker.effects), effects);
    let events = drain(&fixture.mailbox);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].envelope.payload_content_type, ERROR_CONTENT_TYPE);
    assert!(
        std::str::from_utf8(&events[0].payload)
            .unwrap()
            .contains(detail),
        "wrong refusal: {events:?}"
    );
}

fn settlement_version(operation_id: u64, retain_from: u32) -> Event {
    let mut event = settlement_input();
    event.envelope.event_id = format!("settle-operation-{operation_id}-{retain_from}");
    let mut command: SettlementCommand = serde_json::from_slice(&event.payload).unwrap();
    command.sequences[0].operation_id = operation_id;
    command.sequences[0].retain_from = retain_from;
    event.payload = serde_json::to_vec(&command).unwrap();
    event
}

#[test]
fn exact_control_redelivery_reuses_the_receipt_without_another_native_effect() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        for (mode, event, operation) in [
            (
                ResponseMode::SettlementExact,
                settlement_input(),
                Operation::PhysicalSettle,
            ),
            (
                ResponseMode::ReleaseExactStatus,
                release_input(),
                Operation::PhysicalRelease,
            ),
        ] {
            let mut fixture = fixture_at(mode, role);
            if operation == Operation::PhysicalSettle {
                establish_native_verify(&mut fixture);
            } else {
                establish_native_owner(&mut fixture);
            }
            let first = execute_valid_control(&mut fixture, event.clone(), operation);
            let operations = fixture.trace.lock().unwrap().native_operations.clone();
            let kv = fixture.trace.lock().unwrap().native_kv.clone();
            let owners = fixture.worker.state.stage_owners.clone();
            let frontiers = format!("{:?}", fixture.worker.state.stage_frontiers);
            let mut duplicate = event;
            duplicate.envelope.event_id = "same-control-new-event-id".into();
            fixture.handle(duplicate).unwrap();
            assert_eq!(fixture.trace.lock().unwrap().native_operations, operations);
            assert_eq!(fixture.trace.lock().unwrap().native_kv, kv);
            assert_eq!(fixture.worker.state.stage_owners, owners);
            assert_eq!(
                format!("{:?}", fixture.worker.state.stage_frontiers),
                frontiers,
                "receipt replay must not rewind or advance the KV frontier twice"
            );
            let replay = drain(&fixture.mailbox);
            assert_eq!(replay.len(), 1);
            assert_eq!(
                replay[0].envelope.payload_content_type,
                first.envelope.payload_content_type
            );
            assert_eq!(replay[0].payload, first.payload);
        }
    }
}

#[test]
fn reused_control_operation_rejects_changed_body_or_kind_before_native() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut settle = fixture_at(ResponseMode::SettlementExact, role);
        establish_native_verify(&mut settle);
        execute_valid_control(
            &mut settle,
            settlement_version(1, 5),
            Operation::PhysicalSettle,
        );
        assert_control_rejected_without_native_effect(
            &mut settle,
            settlement_version(1, 4),
            "stage control operation identity conflicts with its request",
        );
        // Release has no mutable non-identity fields. A SETTLE with the same
        // operation number is a distinct canonical command, not a retry of it.
        let mut release = fixture_at(ResponseMode::ReleaseExactStatus, role);
        establish_native_owner(&mut release);
        execute_valid_control(&mut release, release_input(), Operation::PhysicalRelease);
        assert_control_rejected_without_native_effect(
            &mut release,
            settlement_version(1, 4),
            "stage control operation identity conflicts with its request",
        );
    }
}

#[test]
fn older_control_after_a_new_receipt_cannot_run_again_or_block_fresh_work() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_at(ResponseMode::SettlementExact, role);
        establish_native_verify(&mut fixture);
        execute_valid_control(
            &mut fixture,
            settlement_version(1, 5),
            Operation::PhysicalSettle,
        );
        append_native_verify(&mut fixture, &request_key("session", "request"), 2);
        execute_valid_control(
            &mut fixture,
            settlement_version(2, 6),
            Operation::PhysicalSettle,
        );
        assert_control_rejected_without_native_effect(
            &mut fixture,
            settlement_version(1, 5),
            "stage control operation is older than its latest receipt",
        );
        append_native_verify(&mut fixture, &request_key("session", "request"), 3);
        execute_valid_control(
            &mut fixture,
            settlement_version(3, 7),
            Operation::PhysicalSettle,
        );
        assert_eq!(
            fixture
                .trace
                .lock()
                .unwrap()
                .native_kv
                .get(&(0, request_key("session", "request"), 1)),
            Some(&7)
        );
    }
}

#[test]
fn refused_drive_issue_preserves_cohort_and_member_fairness_until_native_acceptance() {
    let mut fixture = fixture(ResponseMode::ExactSplit);
    // Establish both decode-ready requests through actual issue and terminal
    // consumers, rather than constructing the ready state the policy expects.
    for name in ["decode-a", "decode-b"] {
        fixture.handle(submission(name, vec![7, 11])).unwrap();
    }
    fixture.worker.drive_first_batches().unwrap();
    let seed = forwarded(&fixture.mailbox);
    assert_eq!(seed.0.len(), 2);
    for capsule in seed.0 {
        fixture.handle(terminal(capsule)).unwrap();
    }
    drain(&fixture.mailbox);
    assert!(fixture.worker.state.open_batches.is_empty());
    for name in ["decode-a", "decode-b"] {
        let request = &fixture.worker.state.requests[&request_key("session", name)];
        assert_eq!(request.ready.as_ref().unwrap().phase, Phase::Decode);
        assert_eq!((request.generated, request.outstanding), (1, 0));
    }
    fixture.handle(submission("prompt", vec![7; 16])).unwrap();
    fixture.worker.state.equal_sequence_ubatch = true;
    fixture.worker.state.batch_capacity = 1;
    fixture.worker.state.physical_capacity = 1;
    fixture.worker.state.max_open_batches = 1;

    // This immutable probe demand is declared independently of the drive's
    // demand builder. PreparedPlan Debug includes policy revision and all
    // candidate fairness fields; comparing it is a state-conservation check,
    // not using the implementation's allocation as a correctness oracle.
    let demands = [
        ("decode-a", 0, Phase::Decode, 1),
        ("decode-b", 1, Phase::Decode, 1),
        ("prompt", 2, Phase::Prefill, 16),
    ]
    .into_iter()
    .map(|(name, sequence_id, phase, available_rows)| Demand {
        request_id: request_key("session", name),
        sequence_id,
        compatibility: "session".into(),
        phase,
        available_rows,
        atomic: false,
    })
    .collect::<Vec<_>>();
    let inspect_policy = |worker: &Worker| {
        worker
            .scheduler
            .prepare_plan_with_physical_capacity(&demands, 1, 1, true, 1, false)
            .unwrap()
    };
    let before_plan = inspect_policy(&fixture.worker);
    assert_eq!(before_plan.allocations().len(), 1);
    assert_eq!(before_plan.allocations()[0].sequence_id, 0);
    assert_eq!(before_plan.allocations()[0].phase, Phase::Decode);
    let policy_before = format!("{before_plan:?}");
    let cursor_before = fixture.worker.scheduler.cursor();
    let requests_before = fixture
        .worker
        .state
        .requests
        .iter()
        .map(|(key, request)| {
            (
                key.clone(),
                request.prompt_cursor,
                request.prompt_issued,
                request.outstanding,
                request.generated,
                format!("{:?}", request.ready),
            )
        })
        .collect::<Vec<_>>();
    let owners_before = fixture.worker.state.stage_owners.clone();
    let flights_before = fixture.worker.state.flights.clone();
    let native_before = fixture.trace.lock().unwrap().native_operations.clone();
    let valid_issue_id = fixture.worker.state.next_open_batch;
    assert_ne!(valid_issue_id, 0);
    fixture.worker.state.next_open_batch = 0;

    for _ in 0..64 {
        assert!(fixture.worker.drive_first_batches().is_err());
        assert_eq!(
            &*fixture.worker.snapshot.lock().unwrap(),
            "issue_prepare_failed:logical issue identity is zero",
            "the refusal must occur after policy preparation, before native issue"
        );
        assert_eq!(
            fixture.trace.lock().unwrap().native_operations,
            native_before
        );
        assert_eq!(fixture.worker.scheduler.cursor(), cursor_before);
        assert_eq!(
            format!("{:?}", inspect_policy(&fixture.worker)),
            policy_before,
            "a refused drive issue spent cohort patience or a member resume position"
        );
        assert!(fixture.worker.state.prepared_issue.is_none());
        assert_eq!(fixture.worker.state.stage_owners, owners_before);
        assert_eq!(fixture.worker.state.flights, flights_before);
        assert_eq!(
            fixture
                .worker
                .state
                .requests
                .iter()
                .map(|(key, request)| (
                    key.clone(),
                    request.prompt_cursor,
                    request.prompt_issued,
                    request.outstanding,
                    request.generated,
                    format!("{:?}", request.ready)
                ))
                .collect::<Vec<_>>(),
            requests_before
        );
        assert!(drain(&fixture.mailbox).is_empty());
    }

    // Repair only the issue ordinal. Native must now receive the member that
    // should have been selected before any of the refused opportunities.
    fixture.worker.state.next_open_batch = valid_issue_id;
    fixture.worker.drive_first_batches().unwrap();
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        [native_before, vec![Operation::LogicalBatch]].concat()
    );
    let issued = forwarded(&fixture.mailbox);
    assert_eq!(issued.0.len(), 1);
    assert_eq!(issued.0[0].owners[0].sequence_id, 0);
    assert_eq!(issued.0[0].owners[0].phase, Phase::Decode);
    assert_eq!(fixture.worker.state.open_batches.len(), 1);
    let accepted_policy = inspect_policy(&fixture.worker);
    assert_ne!(format!("{accepted_policy:?}"), policy_before);
    assert_eq!(
        accepted_policy.allocations()[0].sequence_id,
        1,
        "only the accepted issue advances the decode member resume point"
    );
}

fn establish_two_native_owners(fixture: &mut Fixture) -> Vec<super::super::ownership::Identity> {
    let mut head = self::fixture(ResponseMode::ExactSplit);
    for name in ["budget-a", "budget-b"] {
        head.handle(submission(name, vec![7, 11])).unwrap();
    }
    head.worker.drive_first_batches().unwrap();
    let issued = forwarded(&head.mailbox);
    assert_eq!(issued.0.len(), 2);
    let identities: Vec<_> = issued
        .0
        .iter()
        .map(|capsule| super::super::ownership::Identity::from_owner(&capsule.owners[0]))
        .collect();
    assert_eq!(
        identities
            .iter()
            .map(|identity| (identity.sequence_id, identity.sequence_key.clone()))
            .collect::<Vec<_>>(),
        vec![
            (0, request_key("session", "budget-a")),
            (1, request_key("session", "budget-b")),
        ]
    );
    let mut physical = input("two-slot-warmup", vec![1]);
    physical.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
    physical.payload = issued.encode().unwrap();
    fixture.handle(physical).unwrap();
    assert!(!fixture.worker.effects_fenced);
    let events = drain(&fixture.mailbox);
    assert_eq!(events.len(), 2, "one physical forward and one stage span");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event.envelope.payload_content_type.as_str(),
                PHYSICAL_BATCH_CONTENT_TYPE | TAIL_BATCH_CONTENT_TYPE
            ))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE)
            .count(),
        1
    );
    let trace = fixture.trace.lock().unwrap();
    assert_eq!(trace.native_operations, vec![Operation::PhysicalBatch]);
    assert_eq!(trace.native_kv.len(), 2);
    for identity in &identities {
        assert_eq!(
            trace.native_kv.get(&(
                identity.sequence_id,
                identity.sequence_key.clone(),
                identity.incarnation,
            )),
            Some(&2)
        );
    }
    identities
}

fn budget_control_input(
    identities: &[super::super::ownership::Identity],
    release: bool,
) -> (
    Event,
    Vec<(super::super::ownership::Identity, u64, Vec<u8>)>,
) {
    let mut event = input("receipt-budget", vec![1]);
    let mut canonical = Vec::new();
    if release {
        let sequences = identities
            .iter()
            .map(|identity| ReleaseSequence {
                incarnation: identity.incarnation,
                operation_id: 1,
                key: identity.sequence_key.clone(),
                id: identity.sequence_id,
            })
            .collect::<Vec<_>>();
        for (identity, sequence) in identities.iter().zip(&sequences) {
            canonical.push((
                identity.clone(),
                sequence.operation_id,
                crate::v2::control_identity::release(1, "session", sequence).unwrap(),
            ));
        }
        event.envelope.payload_content_type = RELEASE_CONTENT_TYPE.into();
        event.payload = serde_json::to_vec(&ReleaseCommand {
            load_generation: 1,
            session_id: "session".into(),
            sequences,
        })
        .unwrap();
    } else {
        let sequences = identities
            .iter()
            .map(|identity| SettlementSequence {
                incarnation: identity.incarnation,
                operation_id: 1,
                key: identity.sequence_key.clone(),
                id: identity.sequence_id,
                retain_from: 3,
                replay_tokens: Vec::new(),
                replay_position: 0,
                proposal: Vec::new(),
            })
            .collect::<Vec<_>>();
        for (identity, sequence) in identities.iter().zip(&sequences) {
            canonical.push((
                identity.clone(),
                sequence.operation_id,
                crate::v2::control_identity::settlement(1, "session", sequence)
                    .unwrap()
                    .1,
            ));
        }
        event.envelope.payload_content_type = SETTLE_CONTENT_TYPE.into();
        event.payload = serde_json::to_vec(&SettlementCommand {
            load_generation: 1,
            session_id: "session".into(),
            sequences,
        })
        .unwrap();
    }
    (event, canonical)
}

fn assert_control_batch_reserves_all_receipts_before_native(release: bool) {
    const CONTROL_LIMIT: usize = 128;
    const TOTAL_LIMIT: usize = 220;
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_at(
            if release {
                ResponseMode::ReleaseExactStatus
            } else {
                ResponseMode::SettlementExact
            },
            role,
        );
        // Declare a small capacity before real ownership admission. This is
        // not a fabricated owner map or a post-effect budget alteration.
        fixture.worker.state.stage_owners =
            super::super::ownership::StageOwners::with_limits(CONTROL_LIMIT, TOTAL_LIMIT, 8);
        let identities = establish_two_native_owners(&mut fixture);
        if !release {
            for identity in &identities {
                append_native_verify(&mut fixture, &identity.sequence_key, 1);
            }
        }
        let (event, controls) = budget_control_input(&identities, release);
        // Each command's response ceiling read off its own wire contract, not
        // off the budget code: a physical release must echo its request byte
        // for byte, and a settlement echoes its identity prefix, a token
        // count, and at most one four-byte token per physical row. The
        // settlement bodies here carry no replay tokens, so the prefix is the
        // body minus retain_from, replay_position and the count.
        let capacity = fixture.worker.state.physical_capacity;
        let bound = |body: &[u8]| {
            if release {
                body.len()
            } else {
                capacity * 4 + (body.len() - 12) + 4
            }
        };
        for (identity, operation, body) in &controls {
            assert!(body.len() <= CONTROL_LIMIT && bound(body) <= CONTROL_LIMIT);
            assert!(body.len() + bound(body) <= TOTAL_LIMIT);
            assert_eq!(
                fixture
                    .worker
                    .state
                    .stage_owners
                    .check_control(identity, *operation, body, bound(body))
                    .unwrap(),
                super::super::ownership::ControlCheck::New,
                "each command is independently valid and affordable"
            );
        }
        assert!(
            controls
                .iter()
                .map(|(_, _, body)| body.len() + bound(body))
                .sum::<usize>()
                > TOTAL_LIMIT,
            "the refusal must come from the sum of the real bounds, not from \
             a per-control worst case nobody can return"
        );
        let owners_before = fixture.worker.state.stage_owners.clone();
        let frontiers_before = format!("{:?}", fixture.worker.state.stage_frontiers);
        let flights_before = fixture.worker.state.flights.clone();
        let effects_before = format!("{:?}", fixture.worker.effects);
        let operations_before = fixture.trace.lock().unwrap().native_operations.clone();
        let kv_before = fixture.trace.lock().unwrap().native_kv.clone();

        let result = fixture.handle(event);
        let operations_after = fixture.trace.lock().unwrap().native_operations.clone();
        let kv_after = fixture.trace.lock().unwrap().native_kv.clone();
        assert_eq!(
            operations_after, operations_before,
            "aggregate receipt refusal must precede every native effect; handle={result:?}, kv={:?}",
            kv_after
        );
        result.expect("pre-native capacity refusal is reported, not a native uncertainty fence");
        assert!(!fixture.worker.effects_fenced);
        assert_eq!(fixture.trace.lock().unwrap().native_kv, kv_before);
        assert_eq!(fixture.worker.state.stage_owners, owners_before);
        assert_eq!(
            format!("{:?}", fixture.worker.state.stage_frontiers),
            frontiers_before
        );
        assert_eq!(fixture.worker.state.flights, flights_before);
        assert_eq!(format!("{:?}", fixture.worker.effects), effects_before);
        let refused = drain(&fixture.mailbox);
        assert_eq!(refused.len(), 1);
        assert_eq!(refused[0].envelope.payload_content_type, ERROR_CONTENT_TYPE);
        assert!(
            std::str::from_utf8(&refused[0].payload)
                .unwrap()
                .contains("stage control batch total receipt budget is exhausted"),
            "the intended aggregate preflight must reject: {refused:?}"
        );

        // Nothing was spent: the very same first operation now fits alone,
        // without retry identity changes, budget reset or owner reconstruction.
        let (mut followup, _) = budget_control_input(&identities[..1], release);
        followup.envelope.event_id = "single-control-after-aggregate-refusal".into();
        let forwarded = execute_valid_control(
            &mut fixture,
            followup,
            if release {
                Operation::PhysicalRelease
            } else {
                Operation::PhysicalSettle
            },
        );
        assert_eq!(
            forwarded.envelope.payload_content_type,
            match (release, role) {
                (true, NodeRole::Last) => RELEASED_CONTENT_TYPE,
                (true, _) => RELEASE_CONTENT_TYPE,
                (false, NodeRole::Last) => SETTLED_CONTENT_TYPE,
                (false, _) => SETTLE_CONTENT_TYPE,
            }
        );
        let mut expected_kv = kv_before;
        let first = &identities[0];
        let first_key = (
            first.sequence_id,
            first.sequence_key.clone(),
            first.incarnation,
        );
        if release {
            expected_kv.remove(&first_key);
        } else {
            expected_kv.insert(first_key, 3);
        }
        assert_eq!(fixture.trace.lock().unwrap().native_kv, expected_kv);
        let (identity, operation, body) = &controls[0];
        assert!(matches!(
            fixture
                .worker
                .state
                .stage_owners
                .check_at_ceiling(identity, *operation, body)
                .unwrap(),
            super::super::ownership::ControlCheck::Replay(_)
        ));
    }
}

#[test]
fn aggregate_settlement_receipt_budget_refuses_before_the_first_native_effect() {
    assert_control_batch_reserves_all_receipts_before_native(false);
}

#[test]
fn aggregate_release_receipt_budget_refuses_before_the_first_native_effect() {
    assert_control_batch_reserves_all_receipts_before_native(true);
}

/// The other side of the same preflight, at the real consumption path. This
/// budget affords every member's own proven response and does not afford the
/// per-control ceiling for the same members, so a stage that charges the
/// ceiling refuses a batch it can pay for. That refusal is what stopped the
/// 2026-09-09 pressure run from releasing any of its owners.
fn assert_control_batch_admits_the_width_its_bounds_afford(release: bool) {
    const CONTROL_LIMIT: usize = 128;
    const TOTAL_LIMIT: usize = 340;
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_at(
            if release {
                ResponseMode::ReleaseExactStatus
            } else {
                ResponseMode::SettlementExact
            },
            role,
        );
        fixture.worker.state.stage_owners =
            super::super::ownership::StageOwners::with_limits(CONTROL_LIMIT, TOTAL_LIMIT, 8);
        let identities = establish_two_native_owners(&mut fixture);
        if !release {
            for identity in &identities {
                append_native_verify(&mut fixture, &identity.sequence_key, 1);
            }
        }
        let (event, controls) = budget_control_input(&identities, release);
        let capacity = fixture.worker.state.physical_capacity;
        let bound = |body: &[u8]| {
            if release {
                body.len()
            } else {
                capacity * 4 + (body.len() - 12) + 4
            }
        };
        let afforded: usize = controls
            .iter()
            .map(|(_, _, body)| body.len() + bound(body))
            .sum();
        let at_ceiling: usize = controls
            .iter()
            .map(|(_, _, body)| body.len() + CONTROL_LIMIT)
            .sum();
        assert!(
            afforded <= TOTAL_LIMIT && at_ceiling > TOTAL_LIMIT,
            "this budget must separate the two accountings: \
             afforded={afforded}, ceiling={at_ceiling}, limit={TOTAL_LIMIT}"
        );

        let before = fixture.trace.lock().unwrap().native_operations.clone();
        let result = fixture.handle(event);
        result.expect("an affordable control batch is not a refusal");
        assert!(!fixture.worker.effects_fenced);
        let expected = if release {
            Operation::PhysicalRelease
        } else {
            Operation::PhysicalSettle
        };
        assert_eq!(
            fixture.trace.lock().unwrap().native_operations,
            [before, vec![expected; controls.len()]].concat(),
            "every member of an affordable batch executes"
        );
        let events = drain(&fixture.mailbox);
        assert!(!events.is_empty());
        assert!(
            events
                .iter()
                .all(|event| event.envelope.payload_content_type != ERROR_CONTENT_TYPE),
            "no member may be refused for a receipt the stage can pay for: {events:?}"
        );
        for (identity, operation, body) in &controls {
            assert!(
                matches!(
                    fixture
                        .worker
                        .state
                        .stage_owners
                        .check_at_ceiling(identity, *operation, body)
                        .unwrap(),
                    super::super::ownership::ControlCheck::Replay(_)
                ),
                "every member retains its own receipt, so a resend replays"
            );
        }
        // The retained receipts are the bytes that actually arrived, and they
        // stay inside the budget that admitted the batch.
        let mut resent = budget_control_input(&identities, release).0;
        resent.envelope.event_id = "resent-affordable-control-batch".into();
        let operations = fixture.trace.lock().unwrap().native_operations.clone();
        let kv = fixture.trace.lock().unwrap().native_kv.clone();
        fixture.handle(resent).expect("an exact resend is a replay");
        assert!(!fixture.worker.effects_fenced);
        assert_eq!(fixture.trace.lock().unwrap().native_operations, operations);
        assert_eq!(fixture.trace.lock().unwrap().native_kv, kv);
    }
}

#[test]
fn a_settlement_batch_its_bounds_afford_is_admitted_at_the_consumption_path() {
    assert_control_batch_admits_the_width_its_bounds_afford(false);
}

#[test]
fn a_release_batch_its_bounds_afford_is_admitted_at_the_consumption_path() {
    assert_control_batch_admits_the_width_its_bounds_afford(true);
}

fn assert_release_uses_the_last_available_event_id(role: NodeRole, event: &Event) {
    let mut fixture = fixture_at(ResponseMode::ReleaseExactStatus, role);
    let mut identities = establish_two_native_owners(&mut fixture);
    identities.reverse();
    let (mut same_input, controls) = budget_control_input(&identities, true);
    same_input.envelope.target = fixture.worker.endpoint.clone();
    assert_eq!(
        &same_input, event,
        "the control arm must consume the same Event"
    );
    let command: ReleaseCommand = serde_json::from_slice(&event.payload).unwrap();
    command.validate().unwrap();
    assert_eq!(
        command
            .sequences
            .iter()
            .map(|sequence| sequence.id)
            .collect::<Vec<_>>(),
        [1, 0],
        "forwarding must retain command order, not sort by slot"
    );
    let session = fixture.worker.state.sessions["session"].clone();
    assert_eq!(Some(&event.envelope.source), session.previous.as_ref());
    fixture.worker.state.next_event = u64::MAX - 1;
    fixture.handle(event.clone()).unwrap();
    assert!(!fixture.worker.effects_fenced);
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        [
            Operation::PhysicalBatch,
            Operation::PhysicalRelease,
            Operation::PhysicalRelease
        ]
    );
    // The fake removes the independently decoded (slot, key, incarnation).
    // Releasing either owner twice cannot satisfy both this map and the calls.
    assert!(fixture.trace.lock().unwrap().native_kv.is_empty());
    for (identity, operation, body) in controls {
        assert_eq!(
            fixture
                .worker
                .state
                .stage_owners
                .check_at_ceiling(&identity, operation, &body)
                .unwrap(),
            super::super::ownership::ControlCheck::Replay(body)
        );
    }
    let mut expected = event.clone();
    expected.envelope.event_id = format!("{}:llamacpp:{}", event.envelope.event_id, u64::MAX - 1);
    expected.envelope.causation_id = Some(event.envelope.event_id.clone());
    expected.envelope.source = fixture.worker.endpoint.clone();
    expected.envelope.sequence = u64::MAX - 1;
    if let Some(next) = session.next {
        expected.envelope.target = next;
        expected.envelope.class = EventClass::Control;
        expected.envelope.payload_content_type = RELEASE_CONTENT_TYPE.into();
    } else {
        expected.envelope.target = session.first;
        expected.envelope.class = EventClass::Telemetry;
        expected.envelope.payload_content_type = RELEASED_CONTENT_TYPE.into();
    }
    let emitted = drain(&fixture.mailbox);
    assert_eq!(
        emitted,
        [expected.clone()],
        "the entire exact successor is the oracle"
    );
    assert_eq!(
        p4_protocol::event::encode(&emitted[0]).unwrap(),
        p4_protocol::event::encode(&expected).unwrap()
    );
    assert_eq!(fixture.worker.state.next_event, u64::MAX);
    assert!(fixture.worker.effects.is_empty());
}

fn assert_release_id_refusal_precedes_native(role: NodeRole, with_prefix: bool) {
    let mut fixture = fixture_at(ResponseMode::ReleaseExactStatus, role);
    let mut identities = establish_two_native_owners(&mut fixture);
    identities.reverse();
    let (mut event, _) = budget_control_input(&identities, true);
    event.envelope.target = fixture.worker.endpoint.clone();
    let command: ReleaseCommand = serde_json::from_slice(&event.payload).unwrap();
    command.validate().unwrap();
    if with_prefix {
        // Explicit method-level retained-prefix case, not a claim that the
        // current synchronous run loop accepts RELEASE during another flush.
        let session = &fixture.worker.state.sessions["session"];
        let (target, class, content_type) = if let Some(next) = &session.next {
            (next.clone(), EventClass::Control, RELEASE_CONTENT_TYPE)
        } else {
            (
                session.first.clone(),
                EventClass::Telemetry,
                RELEASED_CONTENT_TYPE,
            )
        };
        fixture
            .worker
            .effects
            .push_back(super::effects::CommittedEffect::Forward {
                base: input("older-release", vec![1]).envelope,
                target,
                class,
                content_type,
                body: event.payload.clone(),
            });
    }
    fixture.worker.state.next_event = if with_prefix { u64::MAX - 1 } else { u64::MAX };
    let next_before = fixture.worker.state.next_event;
    let owners_before = fixture.worker.state.stage_owners.clone();
    let frontiers_before = format!("{:?}", fixture.worker.state.stage_frontiers);
    let effects_before = format!("{:?}", fixture.worker.effects);
    let flights_before = fixture.worker.state.flights.clone();
    let native_before = fixture.trace.lock().unwrap().native_operations.clone();
    let kv_before = fixture.trace.lock().unwrap().native_kv.clone();
    assert!(fixture.handle(event.clone()).is_err());
    assert_eq!(
        fixture.worker.snapshot.lock().unwrap().as_str(),
        "failed:completion event ID is exhausted by committed obligations",
        "a malformed command or missing stage authority is not this counterexample"
    );
    assert!(
        !fixture.worker.effects_fenced,
        "no native mutation became uncertain"
    );
    assert_eq!(fixture.worker.state.next_event, next_before);
    assert_eq!(fixture.worker.state.stage_owners, owners_before);
    assert_eq!(
        format!("{:?}", fixture.worker.state.stage_frontiers),
        frontiers_before
    );
    assert_eq!(format!("{:?}", fixture.worker.effects), effects_before);
    assert_eq!(fixture.worker.state.flights, flights_before);
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        native_before
    );
    assert_eq!(fixture.trace.lock().unwrap().native_kv, kv_before);
    // The error itself has no unreserved ID either. It must not steal the
    // retained prefix's last ID, nor flush that prefix after this refusal.
    assert!(drain(&fixture.mailbox).is_empty());
    assert_release_uses_the_last_available_event_id(role, &event);
}

#[test]
fn release_event_id_exhaustion_refuses_the_whole_valid_group_before_native() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        assert_release_id_refusal_precedes_native(role, false);
    }
}

#[test]
fn release_preserves_the_last_id_already_owed_to_a_retained_prefix() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        assert_release_id_refusal_precedes_native(role, true);
    }
}

#[test]
fn release_one_id_covers_every_native_member_and_the_exact_ordered_successor() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_at(ResponseMode::ReleaseExactStatus, role);
        let mut identities = establish_two_native_owners(&mut fixture);
        identities.reverse();
        let (mut event, _) = budget_control_input(&identities, true);
        event.envelope.target = fixture.worker.endpoint.clone();
        assert_release_uses_the_last_available_event_id(role, &event);
    }
}

#[test]
fn release_native_failure_retains_uncertainty_without_emitting_its_successor() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_at(ResponseMode::ReleaseWrongStatus, role);
        let mut identities = establish_two_native_owners(&mut fixture);
        identities.reverse();
        let (mut event, _) = budget_control_input(&identities, true);
        event.envelope.target = fixture.worker.endpoint.clone();
        assert_release_uses_the_last_available_event_id(role, &event);
        fixture.worker.state.next_event = u64::MAX - 1;
        let owners_before = fixture.worker.state.stage_owners.clone();
        let frontiers_before = format!("{:?}", fixture.worker.state.stage_frontiers);
        let effects_before = format!("{:?}", fixture.worker.effects);
        let mut expected_kv = fixture.trace.lock().unwrap().native_kv.clone();
        let first = &identities[0];
        assert!(
            expected_kv
                .remove(&(
                    first.sequence_id,
                    first.sequence_key.clone(),
                    first.incarnation
                ))
                .is_some()
        );
        assert!(fixture.handle(event.clone()).is_err());
        assert!(fixture.worker.effects_fenced);
        assert_eq!(fixture.trace.lock().unwrap().native_kv, expected_kv);
        assert_eq!(
            fixture.trace.lock().unwrap().native_operations,
            [Operation::PhysicalBatch, Operation::PhysicalRelease],
            "the first native failure must stop the remaining member"
        );
        assert_eq!(fixture.worker.state.stage_owners, owners_before);
        assert_eq!(
            format!("{:?}", fixture.worker.state.stage_frontiers),
            frontiers_before
        );
        assert_eq!(format!("{:?}", fixture.worker.effects), effects_before);
        let emitted = drain(&fixture.mailbox);
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].envelope.payload_content_type, ERROR_CONTENT_TYPE);
        assert!(
            std::str::from_utf8(&emitted[0].payload)
                .unwrap()
                .contains("physical release acknowledgement is invalid")
        );
        assert_eq!(emitted[0].envelope.sequence, u64::MAX - 1);
        assert_eq!(fixture.worker.state.next_event, u64::MAX);
        assert!(fixture.handle(event).is_err());
        assert!(fixture.worker.flush_effects().is_err());
        assert!(fixture.worker.drive_first_batches().is_err());
        assert!(fixture.worker.effects_fenced);
        assert_eq!(fixture.trace.lock().unwrap().native_kv, expected_kv);
        assert_eq!(
            fixture.trace.lock().unwrap().native_operations,
            [Operation::PhysicalBatch, Operation::PhysicalRelease]
        );
        assert!(drain(&fixture.mailbox).is_empty());
    }
}

fn physical_event_from_head(head: &mut Fixture, requests: &[(&str, Vec<i32>)]) -> Event {
    let mut capsules = Vec::new();
    for (name, tokens) in requests {
        head.handle(submission(name, tokens.clone())).unwrap();
        head.worker.drive_first_batches().unwrap();
        capsules.extend(forwarded(&head.mailbox).0);
    }
    let mut event = input("physical-proposal-probe", vec![1]);
    event.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
    event.payload = CapsuleSet(capsules).encode().unwrap();
    event
}

fn unrelated_fresh_physical_input() -> Event {
    // Slot two has never been touched by the bad one/two-slot native call.
    // Obtain its complete rows from real head admission/issue, not a handmade
    // owner registry. The fresh execution number avoids every warmup receipt.
    let mut head = fixture(ResponseMode::ExactSplit);
    for name in ["unused-slot-zero", "unused-slot-one", "fresh-after-fence"] {
        head.handle(submission(name, vec![7, 11])).unwrap();
        head.worker.drive_first_batches().unwrap();
    }
    let mut capsules = Vec::new();
    for event in drain(&head.mailbox) {
        if event.envelope.payload_content_type == PHYSICAL_BATCH_CONTENT_TYPE {
            capsules.extend(
                CapsuleSet::decode(&event.payload)
                    .unwrap()
                    .0
                    .into_iter()
                    .filter(|capsule| capsule.owners[0].sequence_id == 2),
            );
        }
    }
    assert_eq!(capsules.len(), 1);
    capsules[0].execution_id = 2_000;
    let mut event = input("fresh-after-fence", vec![1]);
    event.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
    event.payload = CapsuleSet(capsules).encode().unwrap();
    event
}

fn assert_overwide_native_result_fenced(fixture: &mut Fixture, event: Event, operation: Operation) {
    assert_eq!(fixture.worker.state.physical_capacity, 2);
    let fresh = unrelated_fresh_physical_input();
    let fresh_set = CapsuleSet::decode(&fresh.payload).unwrap();
    let fresh_rows: Vec<_> = fresh_set
        .0
        .iter()
        .flat_map(|capsule| &capsule.owners)
        .collect();
    fixture
        .worker
        .state
        .stage_owners
        .prepare_rows(1, 8, &fresh_rows)
        .unwrap();
    fixture
        .worker
        .state
        .stage_frontiers
        .prepare_rows(1, 8, &fresh_rows)
        .unwrap();
    fixture
        .worker
        .state
        .physical_receives
        .prepare(&fixture.worker.state.sessions["session"].first, &fresh_set)
        .unwrap();
    let owners_before = fixture.worker.state.stage_owners.clone();
    let frontiers_before = format!("{:?}", fixture.worker.state.stage_frontiers);
    let effects_before = format!("{:?}", fixture.worker.effects);
    let flights_before = fixture.worker.state.flights.clone();
    let operations_before = fixture.trace.lock().unwrap().native_operations.clone();
    let kv_before = fixture.trace.lock().unwrap().native_kv.clone();
    let control = if operation == Operation::PhysicalSettle {
        let command: SettlementCommand = serde_json::from_slice(&event.payload).unwrap();
        let sequence = &command.sequences[0];
        let identity = fixture
            .worker
            .operation_identity(&sequence.key, sequence.id, sequence.incarnation)
            .unwrap();
        let (_, body) = crate::v2::control_identity::settlement(1, "session", sequence).unwrap();
        Some((identity, sequence.operation_id, body))
    } else {
        None
    };

    let result = fixture.handle(event.clone());
    let events = drain(&fixture.mailbox);
    let native = fixture.trace.lock().unwrap();
    assert_eq!(
        native.native_operations,
        [operations_before, vec![operation]].concat(),
        "the codec-valid response must reach the actual native consumer"
    );
    assert_ne!(
        native.native_kv, kv_before,
        "native already mutated KV: rejection is not rollback"
    );
    if operation == Operation::PhysicalBatch {
        let reply = native.physical.last().unwrap();
        assert!(
            reply.encode().is_ok(),
            "the success response is codec-valid"
        );
        assert_eq!(
            reply.0.last().unwrap().outcomes[0].proposal,
            vec![23, 29, 31]
        );
        if reply.0.len() == 2
            && reply.0[0].owners[0].sequence_id != reply.0[1].owners[0].sequence_id
        {
            assert_eq!(
                reply.0[0].outcomes[0].proposal,
                vec![23, 29],
                "a valid first decision must not hide the overwide second one"
            );
            assert_eq!(
                native.native_kv.len(),
                2,
                "both slots actually executed before the bad response"
            );
        }
    } else {
        let reply = native.settlement_responses.last().unwrap();
        let (prefix, _, _, _) = observe_control_identity(reply).unwrap();
        assert_eq!(
            u32::from_le_bytes(reply[prefix..prefix + 4].try_into().unwrap()),
            3
        );
        assert_eq!(
            reply.len(),
            prefix + 4 + 3 * 4,
            "SETTLE success body has its exact declared length"
        );
    }
    let operations_after = native.native_operations.clone();
    drop(native);
    assert!(
        result.is_err() && fixture.worker.effects_fenced,
        "an overwide native proposal must fence: capacity=2, proposal=3, result={result:?}, fenced={}, emitted={:?}",
        fixture.worker.effects_fenced,
        events
            .iter()
            .map(|event| &event.envelope.payload_content_type)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fixture.worker.state.stage_owners, owners_before,
        "no successful owner/control receipt may commit"
    );
    assert_eq!(
        format!("{:?}", fixture.worker.state.stage_frontiers),
        frontiers_before
    );
    assert_eq!(fixture.worker.state.flights, flights_before);
    assert_eq!(format!("{:?}", fixture.worker.effects), effects_before);
    assert!(!events.is_empty());
    assert!(
        events
            .iter()
            .all(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE),
        "no output, successful forwarding, acknowledgement or stage span may escape: {events:?}"
    );
    assert!(
        events.iter().any(|event| {
            let error: serde_json::Value = serde_json::from_slice(&event.payload).unwrap();
            let detail = error["detail"].as_str().unwrap();
            detail.contains("proposal") && detail.contains("capacity")
        }),
        "the physical proposal bound must be the rejecting check: {events:?}"
    );
    if let Some((identity, operation, body)) = control {
        assert_eq!(
            fixture
                .worker
                .state
                .stage_owners
                .check_at_ceiling(&identity, operation, &body)
                .unwrap(),
            super::super::ownership::ControlCheck::New,
            "the invalid response must not become an exact control receipt"
        );
    } else {
        let receipts = format!("{:?}", fixture.worker.state.physical_receives);
        let attempted = CapsuleSet::decode(&event.payload).unwrap().0.len();
        assert_eq!(
            receipts.matches("Uncertain").count(),
            attempted,
            "all fresh native executions are uncertain, including an earlier good capsule: {receipts}"
        );
        assert!(
            !receipts.contains("Completed") && !receipts.contains("Running"),
            "neither a successful nor permanently running receipt may remain: {receipts}"
        );
        assert!(
            receipts.contains("cache_order: []") && receipts.contains("cache_bytes: 0"),
            "no successful response bytes may be retained: {receipts}"
        );
    }
    assert!(fixture.handle(event).is_err());
    assert!(
        fixture.handle(fresh).is_err(),
        "new unrelated work must respect the same fence"
    );
    assert!(fixture.worker.drive_first_batches().is_err());
    assert_eq!(
        fixture.trace.lock().unwrap().native_operations,
        operations_after
    );
    assert!(drain(&fixture.mailbox).is_empty());
}

#[test]
fn physical_proposal_beyond_negotiated_capacity_fences_before_approval() {
    let mut fixture = fixture_at(ResponseMode::PhysicalProposalWidth(3), NodeRole::Last);
    assert_overwide_native_result_fenced(
        &mut fixture,
        new_physical_input(),
        Operation::PhysicalBatch,
    );
}

#[test]
fn settlement_proposal_beyond_negotiated_capacity_fences_before_approval() {
    let mut fixture = fixture_at(ResponseMode::SettlementProposalWidth(3), NodeRole::Last);
    establish_native_verify(&mut fixture);
    assert_overwide_native_result_fenced(
        &mut fixture,
        settlement_input(),
        Operation::PhysicalSettle,
    );
}

#[test]
fn mixed_native_result_does_not_approve_good_prefix_before_overwide_proposal() {
    let mut head = fixture(ResponseMode::ExactSplit);
    let event = physical_event_from_head(
        &mut head,
        &[
            ("good-first", vec![7, 11]),
            ("overwide-second", vec![13, 17]),
        ],
    );
    let mut fixture = fixture_at(ResponseMode::MixedProposalWidth, NodeRole::Last);
    assert_overwide_native_result_fenced(&mut fixture, event, Operation::PhysicalBatch);
}

#[test]
fn native_proposal_width_one_and_capacity_boundary_both_continue_through_the_head() {
    for width in [1, 2] {
        let mut head = fixture(ResponseMode::ExactSplit);
        let event = physical_event_from_head(&mut head, &[("request", vec![7, 11])]);
        let mut tail = fixture_at(ResponseMode::PhysicalProposalWidth(width), NodeRole::Last);
        tail.handle(event).unwrap();
        let mut replies: Vec<_> = drain(&tail.mailbox)
            .into_iter()
            .filter(|event| event.envelope.payload_content_type == TAIL_BATCH_CONTENT_TYPE)
            .collect();
        assert_eq!(replies.len(), 1);
        let reply = replies.pop().unwrap();
        assert_eq!(
            CapsuleSet::decode(&reply.payload).unwrap().0[0].outcomes[0].proposal,
            [23, 29][..width]
        );
        head.handle(reply).unwrap();
        drain(&head.mailbox);
        head.worker.drive_first_batches().unwrap();
        let followup = forwarded(&head.mailbox);
        assert_eq!(followup.0.len(), 1);
        assert_eq!(followup.0[0].owners.len(), width);
        assert_eq!(
            followup.0[0].owners[0].phase,
            if width == 1 {
                Phase::Decode
            } else {
                Phase::Verify
            }
        );
        let mut event = input("followup-within-capacity", vec![1]);
        event.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
        event.payload = followup.encode().unwrap();
        tail.handle(event).unwrap();
        assert!(!tail.worker.effects_fenced);
        assert!(
            drain(&tail.mailbox)
                .iter()
                .any(|event| event.envelope.payload_content_type == TAIL_BATCH_CONTENT_TYPE)
        );
        assert_eq!(
            tail.trace.lock().unwrap().native_operations,
            vec![Operation::PhysicalBatch, Operation::PhysicalBatch]
        );
    }
}

#[test]
fn settlement_proposal_width_one_and_capacity_boundary_both_authorize_further_work() {
    for width in [1, 2] {
        let mut fixture = fixture_at(ResponseMode::SettlementProposalWidth(width), NodeRole::Last);
        establish_native_verify(&mut fixture);
        let acknowledged =
            execute_valid_control(&mut fixture, settlement_input(), Operation::PhysicalSettle);
        let command: SettlementCommand = serde_json::from_slice(&acknowledged.payload).unwrap();
        assert_eq!(command.sequences[0].proposal, [23, 29][..width]);
        append_native_proposal(
            &mut fixture,
            &request_key("session", "request"),
            2,
            &[23, 29][..width],
        );
        assert!(!fixture.worker.effects_fenced);
    }
}
