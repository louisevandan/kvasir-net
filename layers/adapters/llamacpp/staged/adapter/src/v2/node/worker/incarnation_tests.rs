//! Same-load reuse reaches real prefill/issue/tail/release consumers. The fake
//! models native KV occupancy only; it does not enforce adapter incarnation or
//! receipt rules. A delayed old command must be rejected before another native
//! effect, while a fresh command for the reused request must still work.
use super::*;
use crate::process::{ReadyInfo, ServerControl};
use crate::v2::capsule::{
    GeneratedToken, Invocation, PhysicalCapsule, PhysicalOutcome, Tensor, TensorDescriptor,
};
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use p4_protocol::event::OuterEndpoint;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NativeTrace {
    operations: Vec<(Operation, Vec<u8>)>,
    release_identities: Vec<ObservedControlIdentity>,
    // The value distinguishes two incarnations whose key and slot are equal.
    kv: BTreeMap<(u32, String), NativeKv>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeKv {
    execution: u64,
    incarnation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedControlIdentity {
    load_generation: u64,
    incarnation: u64,
    operation_id: u64,
    slot: u32,
    session: String,
    key: String,
}

// Independent byte observation, not the production encoder/decoder under test.
// The fake intentionally does not protect KV by incarnation: if an old control
// reaches native, its effect remains observable and must fail the worker test.
fn release_identity(body: &[u8]) -> Result<ObservedControlIdentity, String> {
    if body.len() < 44 || &body[..8] != b"P4ID\x01\x00\x00\x00" {
        return Err("release lacks P4ID revision 1 header".into());
    }
    let load_generation = u64::from_le_bytes(body[8..16].try_into().unwrap());
    let incarnation = u64::from_le_bytes(body[16..24].try_into().unwrap());
    let operation_id = u64::from_le_bytes(body[24..32].try_into().unwrap());
    let slot = u32::from_le_bytes(body[32..36].try_into().unwrap());
    let mut cursor = 36;
    let mut string = || -> Result<String, String> {
        let length_bytes = body
            .get(cursor..cursor + 4)
            .ok_or("short identity length")?;
        let length = u32::from_le_bytes(length_bytes.try_into().unwrap()) as usize;
        cursor += 4;
        let end = cursor
            .checked_add(length)
            .ok_or("identity length overflow")?;
        let value = std::str::from_utf8(body.get(cursor..end).ok_or("short identity string")?)
            .map_err(|error| error.to_string())?
            .to_owned();
        cursor = end;
        Ok(value)
    };
    let session = string()?;
    let key = string()?;
    if cursor != body.len() {
        return Err("trailing release identity bytes".into());
    }
    Ok(ObservedControlIdentity {
        load_generation,
        incarnation,
        operation_id,
        slot,
        session,
        key,
    })
}

struct NativeStage {
    role: NodeRole,
    next_execution: u64,
    trace: Arc<Mutex<NativeTrace>>,
}

impl ServerControl for NativeStage {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        Ok(Some(ReadyInfo {
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "incarnation-native-fixture".into(),
            transactions: false,
            physical_batch: true,
            physical_identity_revision: 1,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 128,
            n_batch: 4,
            n_ubatch: 4,
            n_seq_max: 1,
            upstream_commit: "fixture-only".into(),
            patch_set: "fixture-only".into(),
            backend_inventory: "no-real-engine".into(),
        }))
    }

    fn request(&mut self, request: Frame) -> Result<Frame, String> {
        self.trace
            .lock()
            .unwrap()
            .operations
            .push((request.header.operation, request.body.clone()));
        if request.header.operation == Operation::PhysicalRelease {
            let identity = release_identity(&request.body)?;
            let mut trace = self.trace.lock().unwrap();
            trace.kv.remove(&(identity.slot, identity.key.clone()));
            trace.release_identities.push(identity);
            return Frame::new(Operation::PhysicalRelease, request.body)
                .map_err(|error| error.to_string());
        }
        let mut set = match request.header.operation {
            Operation::LogicalBatch => {
                let logical =
                    LogicalBatch::decode(&request.body).map_err(|error| format!("{error:?}"))?;
                let owners: Vec<_> = logical.0.iter().map(|row| row.owner.clone()).collect();
                let rows = owners.len();
                let invocation = Invocation {
                    flags: 0,
                    n_seq_tokens: rows as u32,
                    n_seqs: 1,
                    n_seqs_unq: 1,
                    n_pos: 1,
                    positions: owners.iter().map(|owner| owner.position as i32).collect(),
                    sequence_counts: vec![1; rows],
                    sequence_ids: owners
                        .iter()
                        .map(|owner| owner.sequence_id as i32)
                        .collect(),
                    output: owners.iter().map(|owner| owner.output).collect(),
                };
                let execution_id = self.next_execution;
                self.next_execution += 1;
                CapsuleSet(vec![PhysicalCapsule {
                    execution_id,
                    terminal: false,
                    invocation,
                    owners,
                    tensors: vec![Tensor {
                        descriptor: TensorDescriptor {
                            tensor_type: 0,
                            dimensions: vec![1],
                            strides: vec![4],
                            nbytes: 4,
                            view_offset: 0,
                            alias_of: None,
                            name: "fixture-kv".into(),
                        },
                        data: vec![0; 4],
                    }],
                    outcomes: Vec::new(),
                }])
            }
            Operation::PhysicalBatch => {
                CapsuleSet::decode(&request.body).map_err(|error| format!("{error:?}"))?
            }
            other => return Err(format!("unexpected native operation: {other:?}")),
        };
        for capsule in &set.0 {
            for owner in &capsule.owners {
                self.trace.lock().unwrap().kv.insert(
                    (owner.sequence_id, owner.sequence_key.clone()),
                    NativeKv {
                        execution: capsule.execution_id,
                        incarnation: owner.incarnation,
                    },
                );
            }
        }
        if self.role == NodeRole::Last {
            make_terminal(&mut set);
        }
        Frame::new(
            Operation::PhysicalResult,
            set.encode().map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| error.to_string())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}

struct Fixture {
    worker: Worker,
    mailbox: Arc<CompletionMailbox>,
    trace: Arc<Mutex<NativeTrace>>,
}

impl Fixture {
    fn physical(&mut self, mut event: Event) -> Result<(), String> {
        event.envelope.target = self.worker.endpoint.clone();
        self.worker.physical(event)
    }

    fn release(&mut self, mut event: Event) -> Result<(), String> {
        event.envelope.target = self.worker.endpoint.clone();
        self.worker.release(event)
    }
}

fn input(id: &str) -> Event {
    let mut request = crate::v2::tests::request_state(vec![7]);
    request.command.request_id = "reused".into();
    request.command.max_tokens = 1;
    let mut event = request.template;
    event.envelope.event_id = id.into();
    event.envelope.correlation_id = id.into();
    event.envelope.payload_content_type = PREFILL_CONTENT_TYPE.into();
    event.envelope.return_route = Some(OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 42001),
        channel: "reply".into(),
        connection_generation: 1,
    });
    event.envelope.source = Endpoint::Outer(event.envelope.return_route.clone().unwrap());
    event.envelope.target = Endpoint::node(Address::tcp("127.0.0.1", 42001), "first", 1);
    event.payload = serde_json::to_vec(&request.command).unwrap();
    event
}

fn fixture(role: NodeRole) -> Fixture {
    let name = match role {
        NodeRole::First => "first",
        NodeRole::Middle => "middle",
        NodeRole::Last => "last",
    };
    let endpoint = Endpoint::node(Address::tcp("127.0.0.1", 42001), name, 1);
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
    .with_stage_for_test(Box::new(NativeStage {
        role,
        next_execution: 1,
        trace: trace.clone(),
    }))
    .unwrap();
    worker.state.load_generation = 1;
    // This fixture injects a ready stage rather than exercising LOAD.
    worker.state.physical_receives =
        crate::v2::node::physical_receive::PhysicalReceiveLedger::new(1).unwrap();
    worker.state.batch_capacity = 4;
    worker.state.physical_capacity = 4;
    worker.state.context_size = 128;
    worker.state.sequence_capacity = 1;
    worker.state.free_sequences = [0].into();
    worker.state.max_atomic_sequences = 1;
    worker.state.min_batch_rows = 0;
    worker.state.max_issue_rows = 0;
    worker.state.max_open_batches = 0;
    worker.state.prefill_fragments = 1;
    let mut event = input("session-setup");
    event.envelope.target = worker.endpoint.clone();
    event.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&SessionCommand {
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
    })
    .unwrap();
    worker.handle(event).unwrap();
    let replies = drain(&mailbox);
    assert!(
        worker.state.sessions.contains_key("session"),
        "SESSION rejected: {replies:?}"
    );
    Fixture {
        worker,
        mailbox,
        trace,
    }
}

fn drain(mailbox: &CompletionMailbox) -> Vec<Event> {
    let mut events = Vec::new();
    while let Poll::Event(event) = mailbox.try_take() {
        events.push(event);
    }
    events
}

fn exactly_one(events: Vec<Event>, content_type: &str) -> Event {
    let mut selected: Vec<_> = events
        .into_iter()
        .filter(|event| event.envelope.payload_content_type == content_type)
        .collect();
    assert_eq!(selected.len(), 1, "expected one {content_type}");
    selected.pop().unwrap()
}

fn make_terminal(set: &mut CapsuleSet) {
    for capsule in &mut set.0 {
        capsule.terminal = true;
        capsule.tensors.clear();
        capsule.outcomes = capsule
            .owners
            .iter()
            .enumerate()
            .filter(|(_, owner)| owner.output)
            .map(|(index, owner)| PhysicalOutcome {
                owner_index: index as u32,
                generated: vec![GeneratedToken {
                    token: 23,
                    text: "fixture answer".into(),
                    position: owner.position + 1,
                    stop: Some("length".into()),
                }],
                proposal: Vec::new(),
                retain_from: None,
                replay_tokens: Vec::new(),
                replay_position: 0,
            })
            .collect();
    }
}

fn issue(fixture: &mut Fixture, event_id: &str) -> Event {
    fixture.worker.prefill(input(event_id)).unwrap();
    let request = &fixture.worker.state.requests[&request_key("session", "reused")];
    assert_eq!(request.sequence_id, Some(0));
    fixture.worker.drive_first_batches().unwrap();
    exactly_one(drain(&fixture.mailbox), PHYSICAL_BATCH_CONTENT_TYPE)
}

fn finish(fixture: &mut Fixture, physical: &Event) -> Event {
    let mut set = CapsuleSet::decode(&physical.payload).unwrap();
    make_terminal(&mut set);
    let mut event = physical.clone();
    event.envelope.source = fixture.worker.state.sessions["session"].last.clone();
    event.envelope.target = fixture.worker.endpoint.clone();
    event.envelope.event_id = format!("tail-{}", set.0[0].execution_id);
    event.envelope.payload_content_type = TAIL_BATCH_CONTENT_TYPE.into();
    event.payload = set.encode().unwrap();
    fixture.worker.tail(event).unwrap();
    assert!(fixture.worker.state.requests.is_empty());
    let release = exactly_one(drain(&fixture.mailbox), RELEASE_CONTENT_TYPE);
    assert_native_release(fixture, &release);
    release
}

fn acknowledgement(release: &Event, id: &str) -> Event {
    let mut event = release.clone();
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "last", 1);
    event.envelope.target = Endpoint::node(Address::tcp("127.0.0.1", 42001), "first", 1);
    event.envelope.event_id = id.into();
    event.envelope.payload_content_type = RELEASED_CONTENT_TYPE.into();
    event
}

fn assert_round_identity(physical: &Event, release: &Event, incarnation: u64) {
    let capsules = CapsuleSet::decode(&physical.payload).unwrap();
    for owner in capsules.0.iter().flat_map(|capsule| &capsule.owners) {
        assert_eq!(owner.incarnation, incarnation);
        assert_eq!(owner.sequence_id, 0);
        assert_eq!(owner.sequence_key, request_key("session", "reused"));
    }
    let command: ReleaseCommand = serde_json::from_slice(&release.payload).unwrap();
    assert_eq!(command.sequences.len(), 1);
    assert_eq!(command.sequences[0].incarnation, incarnation);
    assert_ne!(command.sequences[0].operation_id, 0);
}

fn assert_native_release(fixture: &Fixture, release: &Event) {
    let command: ReleaseCommand = serde_json::from_slice(&release.payload).unwrap();
    let sequence = &command.sequences[0];
    assert_eq!(
        fixture.trace.lock().unwrap().release_identities.last(),
        Some(&ObservedControlIdentity {
            load_generation: command.load_generation,
            incarnation: sequence.incarnation,
            operation_id: sequence.operation_id,
            slot: sequence.id,
            session: command.session_id,
            key: sequence.key.clone(),
        }),
        "native P4ID must bind the exact adapter control identity"
    );
}

#[test]
fn t18_old_released_cannot_consume_new_release_after_identical_key_and_slot_reuse() {
    let mut head = fixture(NodeRole::First);
    let first = issue(&mut head, "incarnation-a");
    let old_release = finish(&mut head, &first);
    assert_round_identity(&first, &old_release, 1);
    head.worker
        .released(acknowledgement(&old_release, "ack-a"))
        .unwrap();
    drain(&head.mailbox);
    assert_eq!(
        head.worker
            .state
            .free_sequences
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![0]
    );

    let second = issue(&mut head, "incarnation-b");
    let new_release = finish(&mut head, &second);
    assert_round_identity(&second, &new_release, 2);
    assert_ne!(
        CapsuleSet::decode(&first.payload).unwrap().0[0].execution_id,
        CapsuleSet::decode(&second.payload).unwrap().0[0].execution_id
    );
    let before = head.worker.state.pending_releases.clone();
    let free_before = head.worker.state.free_sequences.clone();
    let native_before = head.trace.lock().unwrap().clone();
    let _stale_result = head.worker.released(acknowledgement(
        &old_release,
        "ack-a-redelivered-as-new-event",
    ));
    assert_eq!(
        head.worker.state.pending_releases, before,
        "old incarnation ACK consumed the new pending release"
    );
    assert_eq!(
        head.worker.state.free_sequences, free_before,
        "old ACK reopened the reused slot"
    );
    assert_eq!(*head.trace.lock().unwrap(), native_before);
    head.worker
        .released(acknowledgement(&new_release, "ack-b"))
        .unwrap();
    assert!(head.worker.state.pending_releases.is_empty());
    assert_eq!(
        head.worker
            .state
            .free_sequences
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![0]
    );
}

fn stale_release_does_not_reach_native(role: NodeRole) {
    let mut head = fixture(NodeRole::First);
    let mut downstream = fixture(role);
    let first = issue(&mut head, "incarnation-a");
    downstream.physical(first.clone()).unwrap();
    drain(&downstream.mailbox);
    let old_release = finish(&mut head, &first);
    assert_round_identity(&first, &old_release, 1);
    downstream.release(old_release.clone()).unwrap();
    assert_native_release(&downstream, &old_release);
    drain(&downstream.mailbox);
    assert!(
        downstream.trace.lock().unwrap().kv.is_empty(),
        "first release must actually consume native KV"
    );
    head.worker
        .released(acknowledgement(&old_release, "ack-a"))
        .unwrap();
    drain(&head.mailbox);

    let second = issue(&mut head, "incarnation-b");
    downstream.physical(second.clone()).unwrap();
    drain(&downstream.mailbox);
    let new_release = finish(&mut head, &second);
    assert_round_identity(&second, &new_release, 2);
    let before = downstream.trace.lock().unwrap().clone();
    let identity = (0, request_key("session", "reused"));
    assert_eq!(
        before.kv.get(&identity),
        Some(&NativeKv {
            execution: CapsuleSet::decode(&second.payload).unwrap().0[0].execution_id,
            incarnation: 2
        })
    );
    let mut stale = old_release;
    stale.envelope.event_id = "old-release-redelivered-as-new-event".into();
    let _stale_result = downstream.release(stale);
    let after = downstream.trace.lock().unwrap().clone();
    // Compare every operation body as well as the KV map without dumping raw
    // tensor/route bytes into an otherwise small counterexample diagnostic.
    assert!(
        after == before,
        "delayed old RELEASE reached native: before operations={}, KV={:?}; after operations={}, KV={:?}",
        before.operations.len(),
        before.kv,
        after.operations.len(),
        after.kv
    );
    downstream.release(new_release.clone()).unwrap();
    assert_native_release(&downstream, &new_release);
    assert!(
        downstream.trace.lock().unwrap().kv.is_empty(),
        "fresh release must remain executable"
    );
}

#[test]
fn t18_old_release_cannot_delete_reused_middle_stage_native_kv() {
    stale_release_does_not_reach_native(NodeRole::Middle);
}

#[test]
fn t18_old_release_cannot_delete_reused_tail_stage_native_kv() {
    stale_release_does_not_reach_native(NodeRole::Last);
}
