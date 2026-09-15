//! T24 uses actual SESSION/Worker::handle/PHYSICAL/native Frame/result consumers.
//! Incoming capsules are explicit v4 wire fixtures, not a fake head flight
//! ledger. Every native call writes KV again and advances the tail sampler;
//! the fake deliberately supplies no replay protection. No run-loop/GPU claim.
use super::*;
use crate::process::{ReadyInfo, ServerControl};
use crate::v2::capsule::{
    GeneratedToken, Invocation, PhysicalCapsule, PhysicalOutcome, Tensor, TensorDescriptor,
};
use crate::v2::commands::ErrorPayload;
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use p4_protocol::event::OuterEndpoint;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NativeEffects {
    requests: Vec<CapsuleSet>,
    kv_writes: BTreeMap<(u32, String, u64), Vec<(u64, u32, i32)>>,
    live_kv: BTreeMap<(u32, String, u64), u32>,
    releases: Vec<Vec<u8>>,
    settlements: Vec<Vec<u8>>,
    sampler_calls: u32,
}

#[derive(Clone, Copy)]
enum NativeFault {
    None,
    LostAfterEffects,
    ChangedMembershipAfterEffects,
}

#[derive(Clone, Copy)]
enum VerifyMode {
    Disabled,
    Full,
    DirectPartial,
    Checkpoint,
}

struct NonIdempotentStage {
    role: NodeRole,
    effects: Arc<Mutex<NativeEffects>>,
    fault: NativeFault,
    verify_mode: VerifyMode,
}

fn observed_identity(body: &[u8]) -> (usize, u32, String, u64) {
    assert_eq!(&body[..8], b"P4ID\x01\x00\x00\x00");
    assert_eq!(u64::from_le_bytes(body[8..16].try_into().unwrap()), 1);
    let incarnation = u64::from_le_bytes(body[16..24].try_into().unwrap());
    assert_ne!(u64::from_le_bytes(body[24..32].try_into().unwrap()), 0);
    let slot = u32::from_le_bytes(body[32..36].try_into().unwrap());
    let mut offset = 36;
    let mut read_text = || {
        let len = u32::from_le_bytes(body[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let text = std::str::from_utf8(&body[offset..offset + len])
            .unwrap()
            .to_owned();
        offset += len;
        text
    };
    let session = read_text();
    let key = read_text();
    let (owner, request) = key.split_once('\0').unwrap();
    assert_eq!(owner, session);
    assert!(!request.is_empty() && !request.contains('\0'));
    (offset, slot, key, incarnation)
}

impl ServerControl for NonIdempotentStage {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        Ok(Some(ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "physical-replay-fixture".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 128,
            n_batch: 8,
            n_ubatch: 8,
            n_seq_max: 2,
            physical_result_payload_bytes: 0,
            physical_result_tensor_count: 0,
            max_physical_result_bytes: 33_554_432,
            upstream_commit: "fixture-only".into(),
            patch_set: "fixture-only".into(),
            backend_inventory: "no-real-engine".into(),
            stage_wire_abi: "unknown".into(),
        }))
    }
    fn request(&mut self, frame: Frame) -> Result<Frame, String> {
        if frame.header.operation == Operation::PhysicalRelease {
            // Independent observation of the native wire, not the production
            // codec/owner registry. The fake really removes prior native KV.
            let body = &frame.body;
            let (offset, slot, key, incarnation) = observed_identity(body);
            assert_eq!(offset, body.len());
            let mut native = self.effects.lock().unwrap();
            assert!(native.live_kv.remove(&(slot, key, incarnation)).is_some());
            native.releases.push(body.clone());
            return Frame::new(Operation::PhysicalRelease, body.clone()).map_err(|e| e.to_string());
        }
        if frame.header.operation == Operation::PhysicalSettle {
            let body = &frame.body;
            let (prefix, slot, key, incarnation) = observed_identity(body);
            let retain = u32::from_le_bytes(body[prefix..prefix + 4].try_into().unwrap());
            let replay = u32::from_le_bytes(body[prefix + 4..prefix + 8].try_into().unwrap());
            let count = u32::from_le_bytes(body[prefix + 8..prefix + 12].try_into().unwrap());
            assert_eq!(body.len(), prefix + 12 + count as usize * 4);
            assert!(if count == 0 {
                replay == 0
            } else {
                replay + count == retain
            });
            let mut native = self.effects.lock().unwrap();
            let occupied = native.live_kv.get_mut(&(slot, key, incarnation)).unwrap();
            // Like native server_physical.cpp: direct trim keeps retain_from;
            // checkpoint restore keeps replay_position. No replay guard lives
            // in this fake: even an unsolicited/repeated request mutates it.
            *occupied = (*occupied).min(if count == 0 { retain } else { replay });
            native.settlements.push(body.clone());
            let proposal = if self.role == NodeRole::Last && count == 0 {
                vec![201i32]
            } else {
                Vec::new()
            };
            let mut response = body[..prefix].to_vec();
            response.extend_from_slice(&(proposal.len() as u32).to_le_bytes());
            for token in proposal {
                response.extend_from_slice(&token.to_le_bytes());
            }
            return Frame::new(Operation::PhysicalSettle, response).map_err(|e| e.to_string());
        }
        assert_eq!(frame.header.operation, Operation::PhysicalBatch);
        let mut result = CapsuleSet::decode(&frame.body).map_err(|e| format!("{e:?}"))?;
        let mut native = self.effects.lock().unwrap();
        native.requests.push(result.clone());
        for capsule in &mut result.0 {
            for owner in &capsule.owners {
                *native
                    .live_kv
                    .entry((
                        owner.sequence_id,
                        owner.sequence_key.clone(),
                        owner.incarnation,
                    ))
                    .or_default() += 1;
                native
                    .kv_writes
                    .entry((
                        owner.sequence_id,
                        owner.sequence_key.clone(),
                        owner.incarnation,
                    ))
                    .or_default()
                    .push((capsule.execution_id, owner.position, owner.input_token));
            }
            if self.role == NodeRole::Last {
                capsule.terminal = true;
                capsule.tensors.clear();
                if matches!(capsule.owners[0].phase, Phase::Verify | Phase::Replay) {
                    let first = &capsule.owners[0];
                    let replay = first.phase == Phase::Replay;
                    let checkpoint = !replay && matches!(self.verify_mode, VerifyMode::Checkpoint);
                    let partial = !replay && matches!(self.verify_mode, VerifyMode::DirectPartial);
                    let accepted = if checkpoint {
                        Vec::new()
                    } else if partial {
                        vec![201]
                    } else if replay {
                        vec![201, 301]
                    } else {
                        vec![201, 202, 301]
                    };
                    assert_eq!(capsule.owners.len(), if replay { 2 } else { 3 });
                    native.sampler_calls += accepted.len() as u32;
                    let generated = accepted
                        .iter()
                        .enumerate()
                        .map(|(index, &token)| GeneratedToken {
                            token,
                            text: format!("accepted {token}"),
                            position: first.position + index as u32 + 1,
                            stop: None,
                        })
                        .collect();
                    capsule.outcomes.push(PhysicalOutcome {
                        owner_index: 0,
                        generated,
                        proposal: if checkpoint || partial {
                            Vec::new()
                        } else {
                            vec![*accepted.last().unwrap()]
                        },
                        retain_from: if checkpoint {
                            Some(first.position + 2)
                        } else if partial {
                            Some(first.position + 1)
                        } else {
                            None
                        },
                        replay_tokens: if checkpoint {
                            vec![first.input_token, 201]
                        } else {
                            Vec::new()
                        },
                        replay_position: if checkpoint { first.position } else { 0 },
                    });
                    continue;
                }
                for (index, owner) in capsule.owners.iter().enumerate().filter(|(_, o)| o.output) {
                    native.sampler_calls += 1;
                    let token = 100 + native.sampler_calls as i32;
                    capsule.outcomes.push(PhysicalOutcome {
                        owner_index: index as u32,
                        generated: vec![GeneratedToken {
                            token,
                            text: format!("native sample {}", native.sampler_calls),
                            position: owner.position + 1,
                            stop: None,
                        }],
                        proposal: if !matches!(self.verify_mode, VerifyMode::Disabled)
                            && owner.phase == Phase::Prefill
                        {
                            vec![token, 201, 202]
                        } else {
                            vec![token]
                        },
                        retain_from: None,
                        replay_tokens: Vec::new(),
                        replay_position: 0,
                    });
                }
            } else {
                // Recomputing has different transfer bytes as well as a second
                // write. Exact replay must use the initially retained result.
                // Preserve the real fixture tensor shape for atomic groups:
                // replacing a multi-row tensor with one word is malformed.
                let marker = (native.requests.len() as u32).to_le_bytes();
                assert_eq!(capsule.tensors[0].data.len() % marker.len(), 0);
                for word in capsule.tensors[0].data.chunks_exact_mut(marker.len()) {
                    word.copy_from_slice(&marker);
                }
            }
        }
        match self.fault {
            NativeFault::None => {}
            NativeFault::LostAfterEffects => {
                return Err("injected physical response loss after native effects".into());
            }
            NativeFault::ChangedMembershipAfterEffects => {
                result.0[0].execution_id += 1000;
            }
        }
        Frame::new(Operation::PhysicalResult, result.encode().unwrap()).map_err(|e| e.to_string())
    }
    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}

struct Fixture {
    worker: Worker,
    mailbox: Arc<CompletionMailbox>,
    native: Arc<Mutex<NativeEffects>>,
    role: NodeRole,
}

impl Fixture {
    // The existing body/row/frontier tests address a selected receiver. Source
    // remains a fixture input, so a wrong-source event is not repaired here.
    fn handle(&mut self, mut event: Event) -> Result<(), ()> {
        event.envelope.target = self.worker.endpoint.clone();
        self.worker.handle(event)
    }
}

fn base_event(id: &str) -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template.clone();
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "first", 1);
    event.envelope.event_id = id.into();
    event.envelope.correlation_id = "physical-request".into();
    event.envelope.return_route = Some(OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 42001),
        channel: "reply".into(),
        connection_generation: 1,
    });
    event
}

fn fixture(role: NodeRole) -> Fixture {
    fixture_with_fault(role, NativeFault::None)
}

fn fixture_with_fault(role: NodeRole, fault: NativeFault) -> Fixture {
    fixture_with_behavior(role, fault, VerifyMode::Disabled)
}

fn fixture_with_behavior(role: NodeRole, fault: NativeFault, verify_mode: VerifyMode) -> Fixture {
    assert!(matches!(role, NodeRole::Middle | NodeRole::Last));
    let name = if role == NodeRole::Middle {
        "middle"
    } else {
        "last"
    };
    let endpoint = Endpoint::node(Address::tcp("127.0.0.1", 42001), name, 1);
    let (_sender, receiver) = mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(64);
    let native = Arc::new(Mutex::new(NativeEffects::default()));
    let mut worker = Worker::new(
        endpoint,
        receiver,
        publisher,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    )
    .with_stage_for_test(Box::new(NonIdempotentStage {
        role,
        effects: native.clone(),
        fault,
        verify_mode,
    }))
    .unwrap();
    // Negotiated post-LOAD fixture only. Actual LOAD parser/BindLoad is outside
    // this seam; SESSION, row ownership, codec and native result are not mocked.
    worker.state.load_generation = 1;
    worker.state.physical_receives =
        crate::v2::node::physical_receive::PhysicalReceiveLedger::new(1).unwrap();
    worker.state.batch_capacity = 8;
    worker.state.physical_capacity = 8;
    worker.state.context_size = 128;
    worker.state.sequence_capacity = 2;
    worker.state.free_sequences = [0, 1].into();
    let mut session = base_event("setup-session");
    session.envelope.target = worker.endpoint.clone();
    session.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    session.payload = serde_json::to_vec(&SessionCommand {
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
        stage_index: 1,
    })
    .unwrap();
    worker.handle(session).unwrap();
    let setup = drain(&mailbox);
    assert!(
        worker.state.sessions.contains_key("session"),
        "SESSION rejected: {setup:?}"
    );
    assert!(native.lock().unwrap().requests.is_empty());
    Fixture {
        worker,
        mailbox,
        native,
        role,
    }
}

fn capsule(execution_id: u64, position: u32) -> PhysicalCapsule {
    PhysicalCapsule {
        execution_id,
        terminal: false,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: 1,
            n_seqs: 1,
            n_seqs_unq: 1,
            n_pos: 1,
            positions: vec![position as i32],
            sequence_counts: vec![1],
            sequence_ids: vec![0],
            output: vec![true],
        },
        owners: vec![RowOwner {
            load_generation: 1,
            incarnation: 1,
            request_id: "request".into(),
            sequence_key: request_key("session", "request"),
            session_id: "session".into(),
            reply: serde_json::to_string(&ReplySpec {
                ingress_agent: Address::tcp("127.0.0.1", 42001).to_string(),
                channel: "reply".into(),
                connection_generation: 1,
                correlation_id: "physical-request".into(),
                deadline_unix_ms: None,
            })
            .unwrap(),
            sequence_id: 0,
            phase: if position == 0 {
                Phase::Prefill
            } else {
                Phase::Decode
            },
            position,
            max_tokens: 16,
            generated_tokens: position,
            output: true,
            input_token: if position == 0 {
                7
            } else {
                100 + position as i32
            },
            speculative_id: 0,
            speculative_index: 0,
            speculative_count: 0,
            options: String::new(),
        }],
        tensors: vec![Tensor {
            descriptor: TensorDescriptor {
                tensor_type: 0,
                dimensions: vec![1],
                strides: vec![4],
                nbytes: 4,
                view_offset: 0,
                alias_of: None,
                name: "cut-set".into(),
            },
            data: vec![7, 0, 0, 0],
        }],
        outcomes: Vec::new(),
    }
}

#[test]
fn physical_result_decoder_enforces_exact_ready_byte_bound() {
    let encoded = CapsuleSet(vec![capsule(7, 0)]).encode().unwrap();
    assert_eq!(
        CapsuleSet::decode_bounded(&encoded, encoded.len() as u64),
        Ok(CapsuleSet(vec![capsule(7, 0)]))
    );
    assert_eq!(
        CapsuleSet::decode_bounded(&encoded, encoded.len() as u64 - 1),
        Err(crate::v2::capsule::CapsuleError::LimitExceeded)
    );
}

fn other_slot(mut capsule: PhysicalCapsule) -> PhysicalCapsule {
    capsule.invocation.sequence_ids = vec![1];
    capsule.owners[0].sequence_id = 1;
    capsule.owners[0].request_id = "other".into();
    capsule.owners[0].sequence_key = request_key("session", "other");
    capsule.owners[0].incarnation = 2;
    capsule
}

fn physical_event(id: &str, capsules: Vec<PhysicalCapsule>) -> Event {
    let mut event = base_event(id);
    event.envelope.payload_content_type = PHYSICAL_BATCH_CONTENT_TYPE.into();
    event.payload = CapsuleSet(capsules).encode().unwrap();
    assert_eq!(&event.payload[..8], b"P4PB\x04\x00\x00\x00");
    event
}

fn drain(mailbox: &CompletionMailbox) -> Vec<Event> {
    let mut events = Vec::new();
    while let Poll::Event(event) = mailbox.try_take() {
        events.push(event);
    }
    events
}

fn forwarded(fixture: &Fixture) -> Vec<u8> {
    let events = drain(&fixture.mailbox);
    forwarded_events(fixture, events)
}

fn replay_forwarded(fixture: &Fixture) -> Vec<u8> {
    let events = drain(&fixture.mailbox);
    assert!(
        events
            .iter()
            .all(|e| e.envelope.payload_content_type != STAGE_SPAN_CONTENT_TYPE),
        "a receipt replay cannot claim a fresh native span: {events:?}"
    );
    forwarded_events(fixture, events)
}

fn forwarded_events(fixture: &Fixture, events: Vec<Event>) -> Vec<u8> {
    assert!(
        events.iter().all(|event| matches!(
            event.envelope.payload_content_type.as_str(),
            PHYSICAL_BATCH_CONTENT_TYPE | TAIL_BATCH_CONTENT_TYPE | STAGE_SPAN_CONTENT_TYPE
        )),
        "native evaluation cannot publish OUTPUT or errors: {events:?}"
    );
    let expected = if fixture.role == NodeRole::Last {
        TAIL_BATCH_CONTENT_TYPE
    } else {
        PHYSICAL_BATCH_CONTENT_TYPE
    };
    let forwards = events
        .iter()
        .filter(|event| event.envelope.payload_content_type == expected)
        .collect::<Vec<_>>();
    assert_eq!(forwards.len(), 1, "one result forward required: {events:?}");
    forwards[0].payload.clone()
}

fn accepted_first(fixture: &mut Fixture) -> (Event, Vec<u8>) {
    let event = physical_event("original-physical", vec![capsule(10, 0)]);
    fixture.handle(event.clone()).unwrap();
    assert!(!fixture.worker.effects_fenced);
    let reply = forwarded(fixture);
    let effects = fixture.native.lock().unwrap().clone();
    assert_eq!(effects.requests.len(), 1);
    assert_eq!(effects.kv_writes.values().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(
        effects.sampler_calls,
        u32::from(fixture.role == NodeRole::Last)
    );
    (event, reply)
}

fn exact_redelivery(role: NodeRole) {
    let mut fixture = fixture(role);
    let (mut repeated, original_reply) = accepted_first(&mut fixture);
    let native_before = fixture.native.lock().unwrap().clone();
    let owners_before = fixture.worker.state.stage_owners.clone();
    let frontiers_before = format!("{:?}", fixture.worker.state.stage_frontiers);
    let effects_before = format!("{:?}", fixture.worker.effects);
    repeated.envelope.event_id = "same-physical-new-event-id".into();
    fixture.handle(repeated).unwrap();
    let native_after = fixture.native.lock().unwrap().clone();
    assert_eq!(
        native_after, native_before,
        "new transport event identity cannot re-evaluate the same physical identity"
    );
    assert!(!fixture.worker.effects_fenced);
    assert_eq!(fixture.worker.state.stage_owners, owners_before);
    assert_eq!(
        format!("{:?}", fixture.worker.state.stage_frontiers),
        frontiers_before
    );
    assert_eq!(format!("{:?}", fixture.worker.effects), effects_before);
    assert_eq!(
        replay_forwarded(&fixture),
        original_reply,
        "replay must retain original cut-set/sample bytes"
    );
}

fn conflicting_redelivery(role: NodeRole) {
    let mut fixture = fixture(role);
    accepted_first(&mut fixture);
    let native_before = fixture.native.lock().unwrap().clone();
    let owners_before = fixture.worker.state.stage_owners.clone();
    let effects_before = format!("{:?}", fixture.worker.effects);
    let mut conflict = capsule(10, 0);
    conflict.tensors[0].data[0] ^= 1;
    // Codec and ownership remain valid; only execution/content binding can
    // reject these changed transfer bytes before native consumes them.
    fixture
        .handle(physical_event("conflicting-physical", vec![conflict]))
        .unwrap();
    let native_after = fixture.native.lock().unwrap().clone();
    assert_eq!(
        native_after, native_before,
        "same execution with different bytes reached native"
    );
    assert!(
        !fixture.worker.effects_fenced,
        "pre-native conflict must not poison valid work"
    );
    assert_eq!(fixture.worker.state.stage_owners, owners_before);
    assert_eq!(format!("{:?}", fixture.worker.effects), effects_before);
    let refused = drain(&fixture.mailbox);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].envelope.payload_content_type, ERROR_CONTENT_TYPE);
    let detail: ErrorPayload = serde_json::from_slice(&refused[0].payload).unwrap();
    assert!(
        detail.detail.contains("physical") && detail.detail.contains("conflict"),
        "wrong refusal: {detail:?}"
    );
    fixture
        .handle(physical_event("fresh-after-conflict", vec![capsule(11, 1)]))
        .unwrap();
    assert!(!fixture.worker.effects_fenced);
    let native = fixture.native.lock().unwrap().clone();
    assert_eq!(native.requests.len(), native_before.requests.len() + 1);
    assert_eq!(native.kv_writes.values().map(Vec::len).sum::<usize>(), 2);
    assert_eq!(
        CapsuleSet::decode(&forwarded(&fixture)).unwrap().0[0].execution_id,
        11
    );
}

#[test]
fn t24_middle_exact_physical_redelivery_does_not_repeat_native_writes() {
    exact_redelivery(NodeRole::Middle);
}
#[test]
fn t24_tail_exact_physical_redelivery_does_not_advance_the_sampler() {
    exact_redelivery(NodeRole::Last);
}
#[test]
fn t24_middle_same_execution_changed_tensor_is_rejected_before_native() {
    conflicting_redelivery(NodeRole::Middle);
}
#[test]
fn t24_tail_same_execution_changed_tensor_is_rejected_before_native() {
    conflicting_redelivery(NodeRole::Last);
}

#[test]
fn t24_fresh_physical_identity_continues_the_same_request_on_both_roles() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        accepted_first(&mut fixture);
        fixture
            .handle(physical_event("fresh-physical", vec![capsule(11, 1)]))
            .unwrap();
        assert!(!fixture.worker.effects_fenced);
        let native = fixture.native.lock().unwrap().clone();
        assert_eq!(native.requests.len(), 2);
        assert_eq!(native.kv_writes.values().map(Vec::len).sum::<usize>(), 2);
        assert_eq!(
            native.sampler_calls,
            if role == NodeRole::Last { 2 } else { 0 }
        );
        let result = CapsuleSet::decode(&forwarded(&fixture)).unwrap();
        assert_eq!(result.0[0].execution_id, 11);
        assert_eq!(result.0[0].owners[0].position, 1);
        if role == NodeRole::Last {
            assert_eq!(result.0[0].outcomes[0].generated[0].token, 102);
        }
    }
}

#[test]
fn t24_mixed_cached_and_fresh_capsules_execute_only_the_fresh_member() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        let (_, old_result) = accepted_first(&mut fixture);
        let old_result = CapsuleSet::decode(&old_result).unwrap().0.remove(0);
        let fresh = other_slot(capsule(11, 0));
        fixture
            .handle(physical_event(
                "mixed-old-new",
                vec![capsule(10, 0), fresh.clone()],
            ))
            .unwrap();
        let native = fixture.native.lock().unwrap().clone();
        assert_eq!(native.requests.len(), 2);
        assert_eq!(
            native.requests[1],
            CapsuleSet(vec![fresh]),
            "cached member must be removed before native invocation"
        );
        assert_eq!(native.kv_writes.values().map(Vec::len).sum::<usize>(), 2);
        assert_eq!(
            native.sampler_calls,
            if role == NodeRole::Last { 2 } else { 0 }
        );
        let result = CapsuleSet::decode(&forwarded(&fixture)).unwrap();
        assert_eq!(result.0.len(), 2);
        assert_eq!(
            result.0[0], old_result,
            "reconstituted mixed result preserves the cached result exactly"
        );
        assert_eq!(result.0[1].execution_id, 11);
        assert_eq!(result.0[1].owners[0].sequence_id, 1);
    }
}

#[test]
fn t24_unseen_older_execution_inside_the_window_is_not_mistaken_for_a_replay() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        // Distinct sequences have no prefix dependency. Global transport order
        // 12 then 11 is legal; a max-seen ID alone cannot classify ID 11 stale.
        fixture
            .handle(physical_event("arrived-12-first", vec![capsule(12, 0)]))
            .unwrap();
        let first = forwarded(&fixture);
        let later = other_slot(capsule(11, 0));
        fixture
            .handle(physical_event("arrived-11-later", vec![later.clone()]))
            .unwrap();
        let second = forwarded(&fixture);
        let native_before = fixture.native.lock().unwrap().clone();
        assert_eq!(native_before.requests.len(), 2);
        assert_eq!(native_before.requests[1], CapsuleSet(vec![later.clone()]));
        assert_eq!(
            native_before
                .kv_writes
                .values()
                .map(Vec::len)
                .sum::<usize>(),
            2
        );
        for (id, capsule, expected) in [
            ("repeat-12", capsule(12, 0), first),
            ("repeat-11", later, second),
        ] {
            fixture.handle(physical_event(id, vec![capsule])).unwrap();
            assert_eq!(
                fixture.native.lock().unwrap().clone(),
                native_before,
                "both retained receipts must replay after out-of-order admission"
            );
            assert_eq!(replay_forwarded(&fixture), expected);
        }
    }
}

fn post_native_failure_never_reissues(fault: NativeFault, expected_detail: &str) {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_with_fault(role, fault);
        let event = physical_event("effect-then-failure", vec![capsule(10, 0)]);
        assert!(fixture.handle(event.clone()).is_err());
        assert!(fixture.worker.effects_fenced);
        let native_after = fixture.native.lock().unwrap().clone();
        assert_eq!(native_after.requests.len(), 1);
        assert_eq!(
            native_after.kv_writes.values().map(Vec::len).sum::<usize>(),
            1
        );
        assert_eq!(
            native_after.sampler_calls,
            u32::from(role == NodeRole::Last)
        );
        let errors = drain(&fixture.mailbox);
        assert!(!errors.is_empty());
        assert!(
            errors
                .iter()
                .all(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE),
            "no successful physical/terminal result may escape: {errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|event| std::str::from_utf8(&event.payload)
                    .unwrap()
                    .contains(expected_detail)),
            "the intended post-native failure must be reached: {errors:?}"
        );
        let owners_after = fixture.worker.state.stage_owners.clone();
        let effects_after = format!("{:?}", fixture.worker.effects);
        let mut repeat = event;
        repeat.envelope.event_id = "repeat-failed-physical-new-event".into();
        for incoming in [
            repeat,
            physical_event("fresh-while-fenced", vec![capsule(11, 1)]),
        ] {
            assert!(fixture.handle(incoming).is_err());
            assert_eq!(fixture.native.lock().unwrap().clone(), native_after);
            assert_eq!(fixture.worker.state.stage_owners, owners_after);
            assert_eq!(format!("{:?}", fixture.worker.effects), effects_after);
            assert!(drain(&fixture.mailbox).is_empty());
        }
    }
}

#[test]
fn t24_lost_physical_response_preserves_the_uncertain_effect_and_blocks_reexecution() {
    post_native_failure_never_reissues(
        NativeFault::LostAfterEffects,
        "injected physical response loss after native effects",
    );
}

#[test]
fn t24_success_opcode_with_changed_physical_membership_fences_before_reexecution() {
    post_native_failure_never_reissues(
        NativeFault::ChangedMembershipAfterEffects,
        "physical receive result changed its execution membership or role",
    );
}

fn add_session(fixture: &mut Fixture, id: &str, first: NodeAddress) {
    let mut command = fixture.worker.state.sessions["session"].command.clone();
    command.session_id = id.into();
    command.stages[0] = first;
    let mut event = base_event(&format!("setup-{id}"));
    event.envelope.payload_content_type = SESSION_CONTENT_TYPE.into();
    event.payload = serde_json::to_vec(&command).unwrap();
    fixture.handle(event).unwrap();
    let events = drain(&fixture.mailbox);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].envelope.payload_content_type,
        SESSION_READY_CONTENT_TYPE
    );
    assert_eq!(fixture.worker.state.sessions[id].command, command);
}

fn in_other_session(mut physical: PhysicalCapsule, session: &str) -> PhysicalCapsule {
    for owner in &mut physical.owners {
        owner.session_id = session.into();
        owner.sequence_key = request_key(session, &owner.request_id);
    }
    physical
}

fn rejected_without_effects(fixture: &mut Fixture, event: Event, expected: &str) {
    let native = fixture.native.lock().unwrap().clone();
    let owners = fixture.worker.state.stage_owners.clone();
    let frontiers = format!("{:?}", fixture.worker.state.stage_frontiers);
    let receipt = format!("{:?}", fixture.worker.state.physical_receives);
    let effects = format!("{:?}", fixture.worker.effects);
    fixture.handle(event).unwrap();
    assert_eq!(fixture.native.lock().unwrap().clone(), native);
    assert_eq!(fixture.worker.state.stage_owners, owners);
    assert_eq!(
        format!("{:?}", fixture.worker.state.stage_frontiers),
        frontiers
    );
    assert_eq!(
        format!("{:?}", fixture.worker.state.physical_receives),
        receipt
    );
    assert_eq!(format!("{:?}", fixture.worker.effects), effects);
    assert!(!fixture.worker.effects_fenced);
    let events = drain(&fixture.mailbox);
    assert_eq!(events.len(), 1, "only refusal may escape: {events:?}");
    assert_eq!(events[0].envelope.payload_content_type, ERROR_CONTENT_TYPE);
    let error: ErrorPayload = serde_json::from_slice(&events[0].payload).unwrap();
    assert!(error.detail.contains(expected), "wrong refusal: {error:?}");
}

#[test]
fn t24_distinct_head_full_endpoints_own_independent_execution_namespaces() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        // Each endpoint component alone must distinguish two producers. The
        // payload claims no issuer; actual SESSION.first supplies authority.
        for component in 0..3 {
            let mut fixture = fixture(role);
            let first = fixture.worker.state.sessions["session"].command.stages[0].clone();
            let mut second = first.clone();
            match component {
                0 => second.agent = Address::tcp("127.0.0.2", 42001).to_string(),
                1 => second.node = "other-head".into(),
                _ => second.generation += 1,
            }
            add_session(&mut fixture, "session-b", second);
            let (_, result_a) = accepted_first(&mut fixture);
            let input_b = in_other_session(other_slot(capsule(10, 0)), "session-b");
            let mut first_b = physical_event("head-b-first", vec![input_b.clone()]);
            first_b.envelope.source = fixture.worker.state.sessions["session-b"].first.clone();
            fixture.handle(first_b).unwrap();
            let result_b = forwarded(&fixture);
            let native = fixture.native.lock().unwrap().clone();
            assert_eq!(
                native.requests.len(),
                2,
                "component {component} collapsed producer identity"
            );
            assert_eq!(native.requests[1], CapsuleSet(vec![input_b.clone()]));
            assert_eq!(native.live_kv.len(), 2);
            assert_eq!(
                native.sampler_calls,
                if role == NodeRole::Last { 2 } else { 0 }
            );
            let owners = fixture.worker.state.stage_owners.clone();
            for (id, input, expected) in [
                ("head-a-repeat", capsule(10, 0), result_a),
                ("head-b-repeat", input_b, result_b),
            ] {
                let session_id = input.owners[0].session_id.clone();
                let mut repeated = physical_event(id, vec![input]);
                repeated.envelope.source = fixture.worker.state.sessions[&session_id].first.clone();
                fixture.handle(repeated).unwrap();
                assert_eq!(fixture.native.lock().unwrap().clone(), native);
                assert_eq!(fixture.worker.state.stage_owners, owners);
                assert_eq!(replay_forwarded(&fixture), expected);
            }
        }
    }
}

#[test]
fn t24_same_head_cannot_reuse_an_execution_id_under_another_session() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        let first = fixture.worker.state.sessions["session"].command.stages[0].clone();
        add_session(&mut fixture, "session-b", first);
        accepted_first(&mut fixture);
        let input = in_other_session(other_slot(capsule(10, 0)), "session-b");
        rejected_without_effects(
            &mut fixture,
            physical_event("same-head-other-session", vec![input.clone()]),
            "conflicts with its canonical input",
        );
        let mut fresh = input;
        fresh.execution_id = 11;
        fixture
            .handle(physical_event(
                "same-head-next-execution",
                vec![fresh.clone()],
            ))
            .unwrap();
        assert_eq!(
            fixture.native.lock().unwrap().requests[1],
            CapsuleSet(vec![fresh])
        );
        forwarded(&fixture);
    }
}

#[test]
fn t24_conflict_anywhere_in_a_mixed_event_preserves_all_receipts_and_fresh_work() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        for conflict_first in [false, true] {
            let mut fixture = fixture(role);
            let (_, original_result) = accepted_first(&mut fixture);
            let mut conflict = capsule(10, 0);
            conflict.tensors[0].data[0] ^= 1;
            let fresh = other_slot(capsule(11, 0));
            let items = if conflict_first {
                vec![conflict, fresh.clone()]
            } else {
                vec![fresh.clone(), conflict]
            };
            rejected_without_effects(
                &mut fixture,
                physical_event("whole-event-conflict", items),
                "conflicts with its canonical input",
            );
            fixture
                .handle(physical_event("old-still-replayable", vec![capsule(10, 0)]))
                .unwrap();
            assert_eq!(replay_forwarded(&fixture), original_result);
            fixture
                .handle(physical_event("fresh-was-not-burned", vec![fresh.clone()]))
                .unwrap();
            assert_eq!(
                fixture.native.lock().unwrap().requests[1],
                CapsuleSet(vec![fresh])
            );
            forwarded(&fixture);
        }
    }
}

#[test]
fn t24_cached_old_return_cannot_reacquire_a_released_and_reused_slot() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        let (original, old_result) = accepted_first(&mut fixture);
        let mut release = base_event("release-old-incarnation");
        release.envelope.payload_content_type = RELEASE_CONTENT_TYPE.into();
        release.payload = serde_json::to_vec(&ReleaseCommand {
            load_generation: 1,
            session_id: "session".into(),
            sequences: vec![ReleaseSequence {
                key: request_key("session", "request"),
                id: 0,
                incarnation: 1,
                operation_id: 1,
            }],
        })
        .unwrap();
        fixture.handle(release).unwrap();
        let events = drain(&fixture.mailbox);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].envelope.payload_content_type,
            if role == NodeRole::Last {
                RELEASED_CONTENT_TYPE
            } else {
                RELEASE_CONTENT_TYPE
            }
        );
        assert!(fixture.native.lock().unwrap().live_kv.is_empty());
        assert_eq!(fixture.native.lock().unwrap().releases.len(), 1);
        let mut replacement = capsule(11, 0);
        replacement.owners[0].incarnation = 2;
        fixture
            .handle(physical_event(
                "same-slot-new-incarnation",
                vec![replacement.clone()],
            ))
            .unwrap();
        forwarded(&fixture);
        let native = fixture.native.lock().unwrap().clone();
        let owners = fixture.worker.state.stage_owners.clone();
        assert_eq!(native.live_kv.len(), 1);
        assert_eq!(native.live_kv.keys().next().unwrap().2, 2);
        let mut replay = original;
        replay.envelope.event_id = "old-result-after-slot-reuse".into();
        fixture.handle(replay).unwrap();
        assert_eq!(fixture.native.lock().unwrap().clone(), native);
        assert_eq!(fixture.worker.state.stage_owners, owners);
        assert_eq!(replay_forwarded(&fixture), old_result);
        replacement.execution_id = 12;
        replacement.invocation.positions = vec![1];
        replacement.owners[0].position = 1;
        replacement.owners[0].phase = Phase::Decode;
        replacement.owners[0].generated_tokens = 1;
        replacement.owners[0].input_token = 102;
        fixture
            .handle(physical_event(
                "replacement-still-progresses",
                vec![replacement],
            ))
            .unwrap();
        forwarded(&fixture);
        assert_eq!(fixture.native.lock().unwrap().requests.len(), 3);
    }
}

#[test]
fn t24_evicted_or_oversized_receipts_never_authorize_a_second_native_execution() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        for oversized in [false, true] {
            let mut fixture = fixture(role);
            fixture.worker.state.physical_receives =
                crate::v2::node::physical_receive::PhysicalReceiveLedger::with_limits(
                    1,
                    if oversized { 1 } else { 1_000_000 },
                    1,
                    64,
                );
            let (original, _) = accepted_first(&mut fixture);
            if !oversized {
                fixture
                    .handle(physical_event(
                        "evict-old-receipt",
                        vec![other_slot(capsule(11, 0))],
                    ))
                    .unwrap();
                forwarded(&fixture);
            }
            let mut repeated = original;
            repeated.envelope.event_id = "expired-is-not-new-work".into();
            rejected_without_effects(&mut fixture, repeated, "execution receipt has expired");
            // A cache bound must not prohibit unrelated valid work permanently.
            fixture
                .handle(physical_event("fresh-after-expiry", vec![capsule(12, 1)]))
                .unwrap();
            forwarded(&fixture);
            assert_eq!(
                fixture.native.lock().unwrap().requests.len(),
                if oversized { 2 } else { 3 }
            );
        }
    }
}

// Stage-frontier tests intentionally use fresh execution IDs for invalid row
// ranges. A completed-execution receipt is not a per-sequence KV frontier.
// Verify/Replay and control rollback are not modeled in these ordinary cases.
fn frontier_prefill(id: u64, slot: u32, position: u32, last: bool) -> PhysicalCapsule {
    let mut input = capsule(id, position);
    if slot == 1 {
        input = other_slot(input);
    }
    input.owners[0].phase = Phase::Prefill;
    input.owners[0].generated_tokens = 0;
    input.owners[0].input_token = 7 + position as i32;
    input.owners[0].output = last;
    input.invocation.output = vec![last];
    input
}

fn frontier_decode(id: u64, slot: u32, position: u32) -> PhysicalCapsule {
    let mut input = capsule(id, position);
    if slot == 1 {
        input = other_slot(input);
    }
    input.owners[0].phase = Phase::Decode;
    input.owners[0].generated_tokens = 1;
    // In the two-slot warmup the final prompt rows are sampled A then B.
    input.owners[0].input_token = 101 + slot as i32;
    input
}

fn frontier_warmup(fixture: &mut Fixture, slots: u32) {
    for position in 0..2 {
        let inputs = (0..slots)
            .map(|slot| {
                frontier_prefill(
                    10 + u64::from(position * 2 + slot),
                    slot,
                    position,
                    position == 1,
                )
            })
            .collect::<Vec<_>>();
        fixture
            .handle(physical_event(
                &format!("frontier-prefill-{position}"),
                inputs,
            ))
            .unwrap();
        let result = CapsuleSet::decode(&forwarded(fixture)).unwrap();
        assert_eq!(result.0.len(), slots as usize);
        for returned in result.0 {
            assert_eq!(returned.owners[0].phase, Phase::Prefill);
            assert_eq!(returned.owners[0].generated_tokens, 0);
            assert_eq!(returned.owners[0].position, position);
            assert_eq!(returned.owners[0].output, position == 1);
            assert_eq!(
                returned.outcomes.len(),
                usize::from(fixture.role == NodeRole::Last && position == 1)
            );
        }
    }
    let native = fixture.native.lock().unwrap();
    assert_eq!(native.requests.len(), 2);
    assert_eq!(
        native.kv_writes.values().map(Vec::len).sum::<usize>(),
        slots as usize * 2
    );
    assert_eq!(
        native.sampler_calls,
        if fixture.role == NodeRole::Last {
            slots
        } else {
            0
        }
    );
}

#[test]
fn t24_frontier_contiguous_partial_prefill_then_decode_and_exact_old_replay_are_valid() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        frontier_warmup(&mut fixture, 1);
        let decode = frontier_decode(20, 0, 2);
        fixture
            .handle(physical_event(
                "ordinary-decode-after-final-prefill",
                vec![decode.clone()],
            ))
            .unwrap();
        let result = CapsuleSet::decode(&forwarded(&fixture)).unwrap();
        assert_eq!(result.0[0].owners, decode.owners);
        if role == NodeRole::Last {
            assert_eq!(result.0[0].outcomes[0].generated[0].position, 3);
            assert_eq!(result.0[0].outcomes[0].generated[0].token, 102);
        }
        let native = fixture.native.lock().unwrap().clone();
        assert_eq!(native.requests.len(), 3);
        assert_eq!(native.live_kv.values().copied().sum::<u32>(), 3);
        let owners = fixture.worker.state.stage_owners.clone();
        fixture
            .handle(physical_event(
                "exact-old-partial-prefill-replay",
                vec![frontier_prefill(10, 0, 0, false)],
            ))
            .unwrap();
        let old_result = CapsuleSet::decode(&replay_forwarded(&fixture)).unwrap();
        assert_eq!(old_result.0[0].execution_id, 10);
        assert_eq!(old_result.0[0].owners[0].position, 0);
        assert!(old_result.0[0].outcomes.is_empty());
        assert_eq!(fixture.native.lock().unwrap().clone(), native);
        assert_eq!(fixture.worker.state.stage_owners, owners);
    }
}

fn frontier_invalid_new_execution(role: NodeRole, case: &str) {
    let mut fixture = fixture(role);
    frontier_warmup(&mut fixture, 1);
    let input = match case {
        "old-position" => frontier_prefill(20, 0, 0, false),
        "gap" => frontier_decode(20, 0, 3),
        "phase-regression" => frontier_prefill(20, 0, 2, false),
        _ => unreachable!(),
    };
    // The codec accepts every input. Only stage-local prefix authority can
    // reject it; these are not malformed v4 bytes or receipt-ID collisions.
    input.validate().unwrap();
    rejected_without_effects(&mut fixture, physical_event(case, vec![input]), "frontier");
    // Rejection may not burn execution 20 or poison the next legitimate row.
    fixture
        .handle(physical_event(
            "repair-same-new-id",
            vec![frontier_decode(20, 0, 2)],
        ))
        .unwrap();
    forwarded(&fixture);
    assert_eq!(fixture.native.lock().unwrap().requests.len(), 3);
}

#[test]
fn t24_frontier_middle_rejects_new_execution_at_old_position() {
    frontier_invalid_new_execution(NodeRole::Middle, "old-position");
}
#[test]
fn t24_frontier_tail_rejects_new_execution_at_old_position() {
    frontier_invalid_new_execution(NodeRole::Last, "old-position");
}
#[test]
fn t24_frontier_middle_rejects_new_execution_beyond_prefix_gap() {
    frontier_invalid_new_execution(NodeRole::Middle, "gap");
}
#[test]
fn t24_frontier_tail_rejects_new_execution_beyond_prefix_gap() {
    frontier_invalid_new_execution(NodeRole::Last, "gap");
}
#[test]
fn t24_frontier_middle_rejects_prefill_after_prompt_completion() {
    frontier_invalid_new_execution(NodeRole::Middle, "phase-regression");
}
#[test]
fn t24_frontier_tail_rejects_prefill_after_prompt_completion() {
    frontier_invalid_new_execution(NodeRole::Last, "phase-regression");
}

fn frontier_mixed_atomic(role: NodeRole) {
    for invalid_first in [false, true] {
        let mut fixture = fixture(role);
        frontier_warmup(&mut fixture, 2);
        let valid_a = frontier_decode(20, 0, 2);
        let invalid_b = frontier_decode(21, 1, 3);
        let input = if invalid_first {
            vec![invalid_b, valid_a.clone()]
        } else {
            vec![valid_a.clone(), invalid_b]
        };
        rejected_without_effects(
            &mut fixture,
            physical_event("mixed-valid-and-frontier-gap", input),
            "frontier",
        );
        // Both candidate frontiers and both receive IDs remain available.
        fixture
            .handle(physical_event(
                "repair-both-frontiers",
                vec![valid_a, frontier_decode(21, 1, 2)],
            ))
            .unwrap();
        forwarded(&fixture);
        let native = fixture.native.lock().unwrap();
        assert_eq!(native.requests.len(), 3);
        assert_eq!(
            native.live_kv.values().copied().collect::<Vec<_>>(),
            vec![3, 3]
        );
        assert_eq!(
            native.sampler_calls,
            if role == NodeRole::Last { 4 } else { 0 }
        );
    }
}

#[test]
fn t24_frontier_middle_rejects_whole_mixed_event_before_any_native_effect() {
    frontier_mixed_atomic(NodeRole::Middle);
}
#[test]
fn t24_frontier_tail_rejects_whole_mixed_event_before_any_native_effect() {
    frontier_mixed_atomic(NodeRole::Last);
}

fn frontier_atomic(id: u64, phase: Phase, tokens: &[i32]) -> PhysicalCapsule {
    let mut input = frontier_decode(id, 0, 2);
    let first = input.owners[0].clone();
    input.owners = tokens
        .iter()
        .enumerate()
        .map(|(index, &token)| {
            let mut owner = first.clone();
            owner.phase = phase;
            owner.position += index as u32;
            owner.output = phase == Phase::Verify;
            owner.input_token = token;
            // outcome::apply_fragment carries the Verify round through the
            // checkpoint SETTLE and Replay; only physical execution ID is new.
            owner.speculative_id = 50;
            owner.speculative_index = index as u32;
            owner.speculative_count = tokens.len() as u32;
            owner
        })
        .collect();
    input.invocation.n_seq_tokens = tokens.len() as u32;
    input.invocation.positions = input.owners.iter().map(|o| o.position as i32).collect();
    input.invocation.sequence_counts = vec![1; tokens.len()];
    input.invocation.sequence_ids = vec![0; tokens.len()];
    input.invocation.output = input.owners.iter().map(|o| o.output).collect();
    input.tensors[0].descriptor.dimensions = vec![tokens.len() as i64];
    input.tensors[0].descriptor.nbytes = tokens.len() as u64 * 4;
    input.tensors[0].data = tokens.iter().flat_map(|t| t.to_le_bytes()).collect();
    input.validate().unwrap();
    crate::v2::node::flight::validate_membership(&input).unwrap();
    input
}

fn frontier_settle(id: &str, retain: u32, replay: &[i32]) -> Event {
    let mut event = base_event(id);
    event.envelope.payload_content_type = SETTLE_CONTENT_TYPE.into();
    let command = SettlementCommand {
        load_generation: 1,
        session_id: "session".into(),
        sequences: vec![SettlementSequence {
            key: request_key("session", "request"),
            id: 0,
            incarnation: 1,
            operation_id: 1,
            retain_from: retain,
            replay_tokens: replay.to_vec(),
            replay_position: if replay.is_empty() { 0 } else { 2 },
            proposal: Vec::new(),
        }],
    };
    command.validate().unwrap();
    event.payload = serde_json::to_vec(&command).unwrap();
    event
}

fn frontier_settled(fixture: &Fixture, checkpoint: bool) {
    let events = drain(&fixture.mailbox);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].envelope.payload_content_type,
        if fixture.role == NodeRole::Last {
            SETTLED_CONTENT_TYPE
        } else {
            SETTLE_CONTENT_TYPE
        }
    );
    let command: SettlementCommand = serde_json::from_slice(&events[0].payload).unwrap();
    assert_eq!(command.sequences.len(), 1);
    assert_eq!(
        command.sequences[0].proposal,
        if fixture.role == NodeRole::Last && !checkpoint {
            vec![201]
        } else {
            Vec::new()
        }
    );
}

fn frontier_verify(fixture: &mut Fixture) -> CapsuleSet {
    fixture
        .handle(physical_event(
            "atomic-verify",
            vec![frontier_atomic(20, Phase::Verify, &[101, 201, 202])],
        ))
        .unwrap();
    let result = CapsuleSet::decode(&forwarded(fixture)).unwrap();
    assert_eq!(
        fixture
            .native
            .lock()
            .unwrap()
            .live_kv
            .values()
            .copied()
            .sum::<u32>(),
        5
    );
    assert_eq!(result.0[0].owners.len(), 3);
    if fixture.role == NodeRole::Last {
        assert_eq!(result.0[0].outcomes.len(), 1);
    }
    result
}

#[test]
fn t24_frontier_verify_full_acceptance_allows_only_the_next_append() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_with_behavior(role, NativeFault::None, VerifyMode::Full);
        frontier_warmup(&mut fixture, 1);
        let result = frontier_verify(&mut fixture);
        if role == NodeRole::Last {
            let outcome = &result.0[0].outcomes[0];
            assert_eq!(
                outcome
                    .generated
                    .iter()
                    .map(|t| t.token)
                    .collect::<Vec<_>>(),
                vec![201, 202, 301]
            );
            assert_eq!(outcome.retain_from, None);
            assert_eq!(outcome.proposal, vec![301]);
        }
        let mut next = frontier_decode(21, 0, 5);
        next.owners[0].generated_tokens = 4;
        next.owners[0].input_token = 301;
        let mut old_position = next.clone();
        old_position.owners[0].position = 4;
        old_position.invocation.positions = vec![4];
        rejected_without_effects(
            &mut fixture,
            physical_event("old-position-after-full-verify", vec![old_position]),
            "frontier",
        );
        fixture
            .handle(physical_event("append-after-full-verify", vec![next]))
            .unwrap();
        forwarded(&fixture);
        assert_eq!(fixture.native.lock().unwrap().requests.len(), 4);
        assert_eq!(
            fixture
                .native
                .lock()
                .unwrap()
                .live_kv
                .values()
                .copied()
                .sum::<u32>(),
            6
        );
    }
}

#[test]
fn t24_frontier_direct_partial_verify_requires_settle_before_next_decode() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_with_behavior(role, NativeFault::None, VerifyMode::DirectPartial);
        frontier_warmup(&mut fixture, 1);
        let result = frontier_verify(&mut fixture);
        if role == NodeRole::Last {
            let outcome = &result.0[0].outcomes[0];
            assert_eq!(outcome.generated[0].token, 201);
            assert_eq!(outcome.generated.len(), 1);
            assert_eq!(outcome.retain_from, Some(3));
            assert!(outcome.proposal.is_empty());
        }
        let mut premature = frontier_decode(21, 0, 3);
        premature.owners[0].generated_tokens = 2;
        premature.owners[0].input_token = 201;
        rejected_without_effects(
            &mut fixture,
            physical_event("decode-before-required-direct-settle", vec![premature]),
            "frontier",
        );
        rejected_without_effects(
            &mut fixture,
            frontier_settle("invalid-retain-beyond-verify", 6, &[]),
            "frontier",
        );
        let control = frontier_settle("direct-partial-settle", 3, &[]);
        fixture.handle(control.clone()).unwrap();
        frontier_settled(&fixture, false);
        assert_eq!(
            fixture
                .native
                .lock()
                .unwrap()
                .live_kv
                .values()
                .copied()
                .sum::<u32>(),
            3
        );
        assert_eq!(fixture.native.lock().unwrap().settlements.len(), 1);
        let native = fixture.native.lock().unwrap().clone();
        let mut repeated = control;
        repeated.envelope.event_id = "repeat-direct-settle-new-event".into();
        fixture.handle(repeated).unwrap();
        frontier_settled(&fixture, false);
        assert_eq!(fixture.native.lock().unwrap().clone(), native);
        let mut next = frontier_decode(21, 0, 3);
        next.owners[0].generated_tokens = 2;
        next.owners[0].input_token = 201;
        fixture
            .handle(physical_event("append-after-direct-settle", vec![next]))
            .unwrap();
        forwarded(&fixture);
        assert_eq!(
            fixture
                .native
                .lock()
                .unwrap()
                .live_kv
                .values()
                .copied()
                .sum::<u32>(),
            4
        );
    }
}

#[test]
fn t24_frontier_checkpoint_settle_allows_exact_replay_then_decode() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture_with_behavior(role, NativeFault::None, VerifyMode::Checkpoint);
        frontier_warmup(&mut fixture, 1);
        let result = frontier_verify(&mut fixture);
        if role == NodeRole::Last {
            let outcome = &result.0[0].outcomes[0];
            assert!(outcome.generated.is_empty());
            assert_eq!(outcome.replay_tokens, vec![101, 201]);
            assert_eq!(outcome.replay_position, 2);
            assert_eq!(outcome.retain_from, Some(4));
        }
        fixture
            .handle(frontier_settle("checkpoint-settle", 4, &[101, 201]))
            .unwrap();
        frontier_settled(&fixture, true);
        assert_eq!(
            fixture
                .native
                .lock()
                .unwrap()
                .live_kv
                .values()
                .copied()
                .sum::<u32>(),
            2
        );
        let mut wrong = frontier_atomic(21, Phase::Replay, &[101, 999]);
        wrong.validate().unwrap();
        rejected_without_effects(
            &mut fixture,
            physical_event("changed-checkpoint-replay", vec![wrong.clone()]),
            "frontier",
        );
        wrong.owners[1].input_token = 201;
        wrong.tensors[0].data = [101i32, 201].iter().flat_map(|t| t.to_le_bytes()).collect();
        fixture
            .handle(physical_event("exact-checkpoint-replay", vec![wrong]))
            .unwrap();
        let replayed = CapsuleSet::decode(&forwarded(&fixture)).unwrap();
        if role == NodeRole::Last {
            assert_eq!(
                replayed.0[0].outcomes[0]
                    .generated
                    .iter()
                    .map(|t| t.token)
                    .collect::<Vec<_>>(),
                vec![201, 301]
            );
        }
        assert_eq!(
            fixture
                .native
                .lock()
                .unwrap()
                .live_kv
                .values()
                .copied()
                .sum::<u32>(),
            4
        );
        let mut next = frontier_decode(22, 0, 4);
        next.owners[0].generated_tokens = 3;
        next.owners[0].input_token = 301;
        fixture
            .handle(physical_event("append-after-checkpoint-replay", vec![next]))
            .unwrap();
        forwarded(&fixture);
        assert_eq!(
            fixture
                .native
                .lock()
                .unwrap()
                .live_kv
                .values()
                .copied()
                .sum::<u32>(),
            5
        );
    }
}

#[test]
fn t24_frontier_unsolicited_settle_cannot_cut_an_ordinary_prefix() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        frontier_warmup(&mut fixture, 1);
        rejected_without_effects(
            &mut fixture,
            frontier_settle("unsolicited-settle", 1, &[]),
            "frontier",
        );
    }
}

#[test]
fn t24_frontier_unsolicited_replay_cannot_append_without_checkpoint_permission() {
    for role in [NodeRole::Middle, NodeRole::Last] {
        let mut fixture = fixture(role);
        frontier_warmup(&mut fixture, 1);
        rejected_without_effects(
            &mut fixture,
            physical_event(
                "unsolicited-replay",
                vec![frontier_atomic(20, Phase::Replay, &[101, 201])],
            ),
            "frontier",
        );
    }
}
