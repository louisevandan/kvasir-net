//! Actual flush_effects -> native Frame -> completion mailbox boundaries.
//! The ready head/session/KV starting point is explicitly injected: this is
//! not Worker::run, a stage-to-stage ACK test, a pump, or a real llama test.
//! Native parses the observed P4ID bytes independently and mutates its own KV
//! before returning exact, malformed, or lost responses. It knows no dispatch
//! phases, so phase assertions cannot be satisfied by the native fixture.
use super::effects::CommittedEffect;
use super::*;
use crate::process::{ReadyInfo, ServerControl};
use crate::v2::node::ownership::{ControlCheck, Identity};
use crate::v2::node::state::{
    ControlDispatch, ControlDispatchPhase, PendingRelease, PendingSettlement,
};
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
enum Kind {
    Release,
    Settle,
}

#[derive(Clone, Copy, Debug)]
enum ReplyMode {
    Exact,
    MalformedAfterEffect,
    LostAfterEffect,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct NativeTrace {
    calls: Vec<(Operation, Vec<u8>)>,
    kv: BTreeMap<(u32, String, u64), u32>,
}

struct Native {
    mode: ReplyMode,
    trace: Arc<Mutex<NativeTrace>>,
}

// Do not call the production control codec to manufacture its own echo oracle.
fn observe_identity(body: &[u8]) -> Result<(usize, u32, String, u64), String> {
    if body.len() < 44 || &body[..8] != b"P4ID\x01\0\0\0" {
        return Err("fixture expected P4ID v1".into());
    }
    assert_eq!(u64::from_le_bytes(body[8..16].try_into().unwrap()), 1);
    let incarnation = u64::from_le_bytes(body[16..24].try_into().unwrap());
    let operation = u64::from_le_bytes(body[24..32].try_into().unwrap());
    let slot = u32::from_le_bytes(body[32..36].try_into().unwrap());
    assert_eq!(incarnation, 1);
    assert_eq!(operation, 7 + u64::from(slot));
    let mut cursor = 36;
    let mut string = || -> Result<String, String> {
        let len = u32::from_le_bytes(
            body.get(cursor..cursor + 4)
                .ok_or("short fixture identity")?
                .try_into()
                .unwrap(),
        ) as usize;
        cursor += 4;
        let end = cursor.checked_add(len).ok_or("fixture identity overflow")?;
        let value = std::str::from_utf8(body.get(cursor..end).ok_or("short fixture string")?)
            .map_err(|e| e.to_string())?
            .to_owned();
        cursor = end;
        Ok(value)
    };
    assert_eq!(string()?, "session");
    let key = string()?;
    assert_eq!(key, format!("session\0request-{slot}"));
    Ok((cursor, slot, key, incarnation))
}

impl ServerControl for Native {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        Ok(Some(ReadyInfo {
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "control-dispatch-native-fixture".into(),
            transactions: false,
            physical_batch: true,
            physical_identity_revision: 1,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 2,
            atomic_batch_exclusive: false,
            n_ctx: 128,
            n_batch: 4,
            n_ubatch: 4,
            n_seq_max: 2,
            upstream_commit: "fixture-only".into(),
            patch_set: "fixture-only".into(),
            backend_inventory: "no-engine".into(),
        }))
    }

    fn request(&mut self, frame: Frame) -> Result<Frame, String> {
        let (prefix, slot, key, incarnation) = observe_identity(&frame.body)?;
        let operation = frame.header.operation;
        let mut trace = self.trace.lock().unwrap();
        trace.calls.push((operation, frame.body.clone()));
        let response = match operation {
            Operation::PhysicalRelease => {
                assert_eq!(prefix, frame.body.len());
                assert!(trace.kv.remove(&(slot, key, incarnation)).is_some());
                frame.body
            }
            Operation::PhysicalSettle => {
                assert_eq!(frame.body.len(), prefix + 12, "direct SETTLE header only");
                let retain = u32::from_le_bytes(frame.body[prefix..prefix + 4].try_into().unwrap());
                assert_eq!(retain, 2);
                assert_eq!(&frame.body[prefix + 4..], &[0; 8]);
                *trace
                    .kv
                    .get_mut(&(slot, key, incarnation))
                    .expect("fixture KV exists") = retain;
                let mut response = frame.body[..prefix].to_vec();
                response.extend_from_slice(&0_u32.to_le_bytes());
                response
            }
            other => return Err(format!("unexpected fixture operation {other:?}")),
        };
        drop(trace);
        let response = match self.mode {
            ReplyMode::Exact => response,
            ReplyMode::MalformedAfterEffect => {
                let mut response = response;
                response.push(0xA5);
                response
            }
            ReplyMode::LostAfterEffect => {
                return Err("fixture response lost after native effect".into());
            }
        };
        Frame::new(operation, response).map_err(|e| e.to_string())
    }

    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}

struct Fixture {
    worker: Worker,
    mailbox: Option<Arc<CompletionMailbox>>,
    trace: Arc<Mutex<NativeTrace>>,
    base: Event,
    kind: Kind,
    count: u32,
}

fn key(slot: u32) -> String {
    format!("session\0request-{slot}")
}

fn release_sequence(slot: u32) -> ReleaseSequence {
    ReleaseSequence {
        key: key(slot),
        id: slot,
        incarnation: 1,
        operation_id: 7 + u64::from(slot),
    }
}

fn settle_sequence(slot: u32) -> SettlementSequence {
    SettlementSequence {
        key: key(slot),
        id: slot,
        incarnation: 1,
        operation_id: 7 + u64::from(slot),
        retain_from: 2,
        replay_tokens: vec![],
        replay_position: 0,
        proposal: vec![],
    }
}

fn rows(slot: u32, phase: Phase, start: u32, generated: u32, tokens: &[i32]) -> Vec<RowOwner> {
    let reply = crate::v2::tests::request_state(vec![11]).reply.clone();
    tokens
        .iter()
        .enumerate()
        .map(|(index, token)| RowOwner {
            load_generation: 1,
            incarnation: 1,
            request_id: format!("request-{slot}"),
            sequence_key: key(slot),
            session_id: "session".into(),
            reply: reply.clone(),
            sequence_id: slot,
            phase,
            position: start + index as u32,
            max_tokens: 16,
            generated_tokens: generated,
            output: true,
            input_token: *token,
            speculative_id: if phase == Phase::Verify { 17 } else { 0 },
            speculative_index: if phase == Phase::Verify {
                index as u32
            } else {
                0
            },
            speculative_count: if phase == Phase::Verify {
                tokens.len() as u32
            } else {
                0
            },
            options: "{}".into(),
        })
        .collect()
}

fn seed_rows(worker: &mut Worker, owners: Vec<RowOwner>) {
    let refs: Vec<_> = owners.iter().collect();
    let owner_candidate = worker.state.stage_owners.prepare_rows(1, 2, &refs).unwrap();
    let frontier = worker
        .state
        .stage_frontiers
        .prepare_rows(1, 2, &refs)
        .unwrap();
    let result = CapsuleSet(vec![PhysicalCapsule {
        execution_id: 1,
        terminal: false,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: owners.len() as u32,
            n_seqs: 1,
            n_seqs_unq: 1,
            n_pos: 1,
            positions: owners.iter().map(|row| row.position as i32).collect(),
            sequence_counts: vec![1; owners.len()],
            sequence_ids: owners.iter().map(|row| row.sequence_id as i32).collect(),
            output: owners.iter().map(|row| row.output).collect(),
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
                name: "injected-starting-KV".into(),
            },
            data: vec![0; 4],
        }],
        outcomes: vec![],
    }]);
    let frontier = worker
        .state
        .stage_frontiers
        .complete_rows(frontier, &result, false)
        .unwrap();
    worker.state.stage_frontiers.commit(frontier).unwrap();
    worker.state.stage_owners = owner_candidate;
}

fn fixture(kind: Kind, mode: ReplyMode, count: u32) -> Fixture {
    let endpoint = Endpoint::node(Address::tcp("127.0.0.1", 42771), "first", 1);
    let next = Endpoint::node(Address::tcp("127.0.0.1", 42772), "last", 1);
    let (_sender, receiver) = mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(8);
    let trace = Arc::new(Mutex::new(NativeTrace::default()));
    let mut worker = Worker::new(
        endpoint.clone(),
        receiver,
        publisher,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    )
    .with_stage_for_test(Box::new(Native {
        mode,
        trace: trace.clone(),
    }))
    .unwrap();
    worker.state.load_generation = 1;
    worker.state.physical_capacity = 4;
    worker.state.sequence_capacity = 2;
    let command = SessionCommand {
        load_generation: 1,
        session_id: "session".into(),
        stage_index: 0,
        stages: vec![
            NodeAddress {
                agent: "tcp://127.0.0.1:42771".into(),
                node: "first".into(),
                generation: 1,
            },
            NodeAddress {
                agent: "tcp://127.0.0.1:42772".into(),
                node: "last".into(),
                generation: 1,
            },
        ],
    };
    command.validate().unwrap();
    worker.state.sessions.insert(
        "session".into(),
        PipelineSession {
            command,
            next: Some(next.clone()),
            first: endpoint.clone(),
            previous: None,
            last: next,
        },
    );
    let mut base = crate::v2::tests::request_state(vec![11]).template.clone();
    base.envelope.event_id = "dispatch-origin".into();
    base.envelope.target = endpoint;
    let reply: ReplySpec =
        serde_json::from_str(&crate::v2::tests::request_state(vec![11]).reply).unwrap();
    for slot in 0..count {
        seed_rows(&mut worker, rows(slot, Phase::Prefill, 0, 0, &[11]));
        if matches!(kind, Kind::Settle) {
            seed_rows(&mut worker, rows(slot, Phase::Verify, 1, 1, &[23, 29]));
        }
        trace.lock().unwrap().kv.insert(
            (slot, key(slot), 1),
            if matches!(kind, Kind::Settle) { 3 } else { 1 },
        );
        let dispatch = ControlDispatch::queued(1, "session".into());
        match kind {
            Kind::Release => {
                worker.state.pending_releases.insert(
                    key(slot),
                    PendingRelease {
                        sequence: release_sequence(slot),
                        original: base.envelope.clone(),
                        reply: reply.clone(),
                        dispatch,
                    },
                );
            }
            Kind::Settle => {
                worker.state.pending_settlements.insert(
                    key(slot),
                    PendingSettlement {
                        sequence: settle_sequence(slot),
                        dispatch,
                    },
                );
            }
        }
    }
    Fixture {
        worker,
        mailbox: Some(mailbox),
        trace,
        base,
        kind,
        count,
    }
}

impl Fixture {
    fn phase(&self, slot: u32) -> ControlDispatchPhase {
        match self.kind {
            Kind::Release => {
                self.worker.state.pending_releases[&key(slot)]
                    .dispatch
                    .phase
            }
            Kind::Settle => {
                self.worker.state.pending_settlements[&key(slot)]
                    .dispatch
                    .phase
            }
        }
    }

    fn local_effect(&self, slot: u32) -> CommittedEffect {
        match self.kind {
            Kind::Release => CommittedEffect::Release {
                load_generation: 1,
                session_id: "session".into(),
                sequence: release_sequence(slot),
            },
            Kind::Settle => CommittedEffect::Settle {
                load_generation: 1,
                session_id: "session".into(),
                sequence: settle_sequence(slot),
            },
        }
    }

    fn forward_effect(&self) -> CommittedEffect {
        let (content_type, body) = match self.kind {
            Kind::Release => (
                RELEASE_CONTENT_TYPE,
                serde_json::to_vec(&ReleaseCommand {
                    load_generation: 1,
                    session_id: "session".into(),
                    sequences: (0..self.count).map(release_sequence).collect(),
                })
                .unwrap(),
            ),
            Kind::Settle => (
                SETTLE_CONTENT_TYPE,
                serde_json::to_vec(&SettlementCommand {
                    load_generation: 1,
                    session_id: "session".into(),
                    sequences: (0..self.count).map(settle_sequence).collect(),
                })
                .unwrap(),
            ),
        };
        CommittedEffect::ForwardHeadControl {
            base: self.base.envelope.clone(),
            target: self.worker.state.sessions["session"].next.clone().unwrap(),
            class: EventClass::Control,
            content_type,
            body,
        }
    }

    fn local(&mut self) {
        for slot in 0..self.count {
            self.worker.effects.push_back(self.local_effect(slot));
        }
        self.worker.flush_effects().unwrap();
        for slot in 0..self.count {
            assert_eq!(self.phase(slot), ControlDispatchPhase::LocalApplied);
        }
        assert!(self.worker.effects.is_empty());
        assert!(
            self.drain().is_empty(),
            "local native success is not wire acceptance"
        );
    }

    fn drain(&self) -> Vec<Event> {
        let mut events = vec![];
        if let Some(mailbox) = &self.mailbox {
            while let Poll::Event(event) = mailbox.try_take() {
                events.push(event);
            }
        }
        events
    }

    fn state_image(&self) -> String {
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{}",
            self.worker.state.pending_releases,
            self.worker.state.pending_settlements,
            self.worker.state.stage_owners,
            self.worker.state.stage_frontiers,
            self.worker.state.free_sequences,
            self.worker.state.next_event
        )
    }

    fn rejected_without_effect(&mut self, effect: CommittedEffect, cause: &str) {
        let before = self.state_image();
        let native = self.trace.lock().unwrap().clone();
        self.worker.effects.push_back(effect);
        let queued = format!("{:?}", self.worker.effects);
        let error = self.worker.flush_effects().unwrap_err();
        assert!(error.contains(cause), "expected {cause}: {error}");
        assert_eq!(self.state_image(), before);
        assert_eq!(*self.trace.lock().unwrap(), native);
        assert_eq!(format!("{:?}", self.worker.effects), queued);
        assert!(self.worker.effects_fenced);
        assert!(self.drain().is_empty());
    }
}

#[test]
fn local_native_success_commits_receipt_and_frontier_without_forward_authority() {
    for kind in [Kind::Release, Kind::Settle] {
        let mut f = fixture(kind, ReplyMode::Exact, 2);
        f.local();
        assert_eq!(f.trace.lock().unwrap().calls.len(), 2);
        for slot in 0..2 {
            let identity = Identity::from_owner(&rows(slot, Phase::Prefill, 0, 0, &[11])[0]);
            let (op, body) = f.trace.lock().unwrap().calls[slot as usize].clone();
            let receipt = f
                .worker
                .state
                .stage_owners
                .check_at_ceiling(&identity, 7 + u64::from(slot), &body)
                .unwrap();
            let ControlCheck::Replay(response) = receipt else {
                panic!("native success must commit its receipt")
            };
            match kind {
                Kind::Release => {
                    assert_eq!(op, Operation::PhysicalRelease);
                    assert_eq!(response, body);
                    assert!(
                        f.worker
                            .state
                            .stage_frontiers
                            .prepare_release(&identity)
                            .is_err()
                    );
                }
                Kind::Settle => {
                    assert_eq!(op, Operation::PhysicalSettle);
                    let (prefix, _, _, _) = observe_identity(&body).unwrap();
                    assert_eq!(&response[..prefix], &body[..prefix]);
                    assert_eq!(&response[prefix..], &0_u32.to_le_bytes());
                    let next = rows(slot, Phase::Decode, 2, 2, &[31]);
                    assert!(
                        f.worker
                            .state
                            .stage_frontiers
                            .prepare_rows(1, 2, &next.iter().collect::<Vec<_>>())
                            .is_ok()
                    );
                    let old_end = rows(slot, Phase::Decode, 3, 2, &[31]);
                    assert!(
                        f.worker
                            .state
                            .stage_frontiers
                            .prepare_rows(1, 2, &old_end.iter().collect::<Vec<_>>())
                            .is_err()
                    );
                    assert_eq!(f.trace.lock().unwrap().kv[&(slot, key(slot), 1)], 2);
                }
            }
        }
        if matches!(kind, Kind::Release) {
            assert_eq!(f.worker.state.stage_owners.active_slots(), 0);
            assert_eq!(f.worker.state.stage_frontiers.active_slots(), 0);
            assert!(f.trace.lock().unwrap().kv.is_empty());
        }
    }
}

#[test]
fn malformed_or_lost_native_response_keeps_queued_authority_and_fences_reexecution() {
    for kind in [Kind::Release, Kind::Settle] {
        for mode in [ReplyMode::MalformedAfterEffect, ReplyMode::LostAfterEffect] {
            let mut f = fixture(kind, mode, 1);
            let before = f.state_image();
            f.worker.effects.push_back(f.local_effect(0));
            let queued = format!("{:?}", f.worker.effects);
            let error = f.worker.flush_effects().unwrap_err();
            assert!(
                error.contains("invalid") || error.contains("lost"),
                "{kind:?}/{mode:?}: {error}"
            );
            assert_eq!(f.phase(0), ControlDispatchPhase::Queued);
            assert_eq!(f.state_image(), before);
            assert_eq!(format!("{:?}", f.worker.effects), queued);
            assert!(f.worker.effects_fenced);
            assert_eq!(f.trace.lock().unwrap().calls.len(), 1);
            assert!(f.drain().is_empty());
            assert!(f.worker.flush_effects().unwrap_err().contains("fenced"));
            assert_eq!(
                f.trace.lock().unwrap().calls.len(),
                1,
                "uncertainty must not repeat native"
            );
        }
    }
}

#[test]
fn cached_native_replay_does_not_promote_or_downgrade_dispatch() {
    for kind in [Kind::Release, Kind::Settle] {
        let mut f = fixture(kind, ReplyMode::Exact, 1);
        f.local();
        let native = f.trace.lock().unwrap().clone();
        f.worker.effects.push_back(f.local_effect(0));
        f.worker.flush_effects().unwrap();
        assert_eq!(f.phase(0), ControlDispatchPhase::LocalApplied);
        assert!(f.drain().is_empty());
        assert_eq!(*f.trace.lock().unwrap(), native);
        f.worker.effects.push_back(f.forward_effect());
        f.worker.flush_effects().unwrap();
        assert_eq!(f.drain().len(), 1);
        assert_eq!(f.phase(0), ControlDispatchPhase::ForwardAccepted);
        f.worker.effects.push_back(f.local_effect(0));
        f.worker.flush_effects().unwrap();
        assert_eq!(f.phase(0), ControlDispatchPhase::ForwardAccepted);
        assert_eq!(*f.trace.lock().unwrap(), native);
        assert!(f.drain().is_empty());
    }
}

#[test]
fn accepted_whole_control_forward_promotes_every_member_after_exact_event_delivery() {
    for kind in [Kind::Release, Kind::Settle] {
        let mut f = fixture(kind, ReplyMode::Exact, 2);
        f.local();
        let native = f.trace.lock().unwrap().clone();
        let next_event = f.worker.state.next_event;
        let effect = f.forward_effect();
        let CommittedEffect::ForwardHeadControl {
            target,
            content_type,
            body,
            ..
        } = &effect
        else {
            unreachable!()
        };
        let expected = (target.clone(), *content_type, body.clone());
        f.worker.effects.push_back(effect);
        f.worker.flush_effects().unwrap();
        let sent = f.drain();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].envelope.source, f.worker.endpoint);
        assert_eq!(sent[0].envelope.target, expected.0);
        assert_eq!(sent[0].envelope.class, EventClass::Control);
        assert_eq!(sent[0].envelope.payload_content_type, expected.1);
        assert_eq!(sent[0].payload, expected.2);
        assert_eq!(
            sent[0].envelope.event_id,
            format!("dispatch-origin:llamacpp:{next_event}")
        );
        assert_eq!(f.worker.state.next_event, next_event + 1);
        for slot in 0..2 {
            assert_eq!(f.phase(slot), ControlDispatchPhase::ForwardAccepted);
        }
        assert_eq!(*f.trace.lock().unwrap(), native);
        assert!(f.worker.effects.is_empty());
    }
}

#[test]
fn full_control_replay_revalidates_its_ticket_after_ack_retirement() {
    for kind in [Kind::Release, Kind::Settle] {
        let mut f = fixture(kind, ReplyMode::Exact, 1);
        // The ready head/KV and, for SETTLE, its post-Verify continuation are
        // explicit fixture injection. Local native apply and forward acceptance
        // below are real consumers, not injected dispatch-phase transitions.
        if matches!(kind, Kind::Settle) {
            let mut request = crate::v2::tests::request_state(vec![11]);
            request.input_mut_for_test().command.request_id = "request-0".into();
            request.sequence_id = Some(0);
            request.prompt_cursor = 1;
            request.prompt_issued = 1;
            request.generated = 2;
            request.after_settlement =
                Some(crate::v2::node::state::SettlementContinuation::Proposal {
                    position: 2,
                    token: 31,
                });
            f.worker.state.requests.insert(key(0), request);
            f.worker.state.begin_verify_fence(&[key(0)]).unwrap();
        }
        f.local();
        let native = f.trace.lock().unwrap().clone();
        assert_eq!(native.calls.len(), 1);
        let (publisher, mailbox) = completion_mailbox(1);
        f.worker.publisher = publisher;
        f.mailbox = Some(mailbox);
        f.worker.effects.push_back(f.forward_effect());
        f.worker.flush_effects().unwrap();
        assert_eq!(f.phase(0), ControlDispatchPhase::ForwardAccepted);
        assert!(f.worker.effects.is_empty());

        // Reuse the exact Event accepted by the real forward path as the one
        // occupying capacity. This is not an arbitrary synthetic Full filler.
        let mailbox = f.mailbox.take().unwrap();
        let Poll::Event(historical) = mailbox.try_take() else {
            panic!("historical forward was not actually published")
        };
        let historical_wire = p4_protocol::event::encode(&historical).unwrap();
        assert_eq!(
            p4_protocol::event::decode(&historical_wire).unwrap(),
            historical
        );
        f.worker.publisher.try_publish(historical.clone()).unwrap();

        // This single-worker fixture constructs a valid tail ACK from the
        // actual accepted command. Its production codec/source/ACK consumer
        // are exercised; no remote or multi-stage ACK generation is claimed.
        let mut acknowledgement = historical.clone();
        acknowledgement.envelope.event_id = "tail-ack-for-historical-forward".into();
        acknowledgement.envelope.sequence += 100;
        acknowledgement.envelope.source = f.worker.state.sessions["session"].last.clone();
        acknowledgement.envelope.target = f.worker.endpoint.clone();
        acknowledgement.envelope.causation_id = Some(historical.envelope.event_id.clone());
        match kind {
            Kind::Release => {
                acknowledgement.envelope.payload_content_type = RELEASED_CONTENT_TYPE.into();
                let command: ReleaseCommand =
                    serde_json::from_slice(&acknowledgement.payload).unwrap();
                assert_eq!(command.sequences, vec![release_sequence(0)]);
            }
            Kind::Settle => {
                acknowledgement.envelope.payload_content_type = SETTLED_CONTENT_TYPE.into();
                let mut command: SettlementCommand =
                    serde_json::from_slice(&acknowledgement.payload).unwrap();
                assert_eq!(command.sequences, vec![settle_sequence(0)]);
                command.sequences[0].proposal = vec![31];
                command.validate().unwrap();
                acknowledgement.payload = serde_json::to_vec(&command).unwrap();
            }
        }
        let wire = p4_protocol::event::encode(&acknowledgement).unwrap();
        let acknowledgement = p4_protocol::event::decode(&wire).unwrap();
        let (sender, receiver) = mpsc::channel();
        f.worker.receiver = receiver;
        sender.send(WorkerInput::Event(acknowledgement)).unwrap();

        let replay = f.forward_effect();
        let CommittedEffect::ForwardHeadControl {
            base,
            target,
            class,
            content_type,
            body,
        } = &replay
        else {
            unreachable!()
        };
        let original_allocation = body.as_ptr() as usize;
        let replay_id = f.worker.state.next_event;
        let mut expected_envelope = base.clone();
        expected_envelope.event_id = format!("{}:llamacpp:{replay_id}", base.event_id);
        expected_envelope.causation_id = Some(base.event_id.clone());
        expected_envelope.source = f.worker.endpoint.clone();
        expected_envelope.target = target.clone();
        expected_envelope.class = *class;
        expected_envelope.sequence = replay_id;
        expected_envelope.payload_content_type = (*content_type).into();
        let expected_replay = Event {
            envelope: expected_envelope,
            payload: body.clone(),
        };
        f.worker.effects.push_back(replay);
        let snapshot = Arc::clone(&f.worker.snapshot);
        let shutdown = Arc::clone(&f.worker.shutting_down);
        let (done, finished) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            // Catch stale-ticket assert panics so the mutation reports an
            // ordinary bounded test failure and returns the worker for audit.
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.worker.flush_effects()));
            let _ = done.send((f, outcome));
        });
        let mut mailbox = Some(mailbox);
        let mut recovered_occupant = None;
        let completed = match finished.recv_timeout(Duration::from_secs(1)) {
            Ok(completed) => completed,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // A removed revalidation can wait forever on Full. Giving it
                // room exposes stale reforward/assert behavior without hanging
                // the test or relaxing the no-reforward expectation below.
                let Poll::Event(occupied) = mailbox.as_ref().unwrap().try_take() else {
                    panic!("a blocked replay lost the historical completion")
                };
                recovered_occupant = Some(occupied);
                match finished.recv_timeout(Duration::from_secs(1)) {
                    Ok(completed) => completed,
                    Err(_) => {
                        shutdown.store(true, Ordering::SeqCst);
                        drop(mailbox.take());
                        finished
                            .recv_timeout(Duration::from_secs(2))
                            .expect("flush did not return after bounded shutdown and Closed")
                    }
                }
            }
            Err(error) => panic!("effect worker exited without returning its result: {error}"),
        };
        thread
            .join()
            .expect("effect fixture thread must be collected");
        drop(sender);
        let (mut f, outcome) = completed;
        assert_eq!(
            snapshot.lock().unwrap().as_str(),
            "completion_queue_full:waiting",
            "the replay must encounter actual Full before consuming the ACK"
        );
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(_) => panic!("{kind:?}: a stale pre-Full ticket panicked after ACK retirement"),
        };
        assert_eq!(
            outcome,
            Err("committed head control could not be delivered".into())
        );
        assert!(f.worker.effects_fenced);
        assert_eq!(
            *f.trace.lock().unwrap(),
            native,
            "ACK service must not repeat native apply"
        );
        assert_eq!(f.worker.state.next_event, replay_id + 1);
        assert_eq!(f.worker.active_publications, 0);
        assert!(f.worker.deferred_ack_error.is_none());
        assert!(f.worker.held_input.is_none());
        assert!(f.worker.state.pending_releases.is_empty());
        assert!(f.worker.state.pending_settlements.is_empty());
        match kind {
            Kind::Release => {
                assert_eq!(
                    f.worker
                        .state
                        .free_sequences
                        .iter()
                        .copied()
                        .collect::<Vec<_>>(),
                    [0]
                );
                assert_eq!(f.worker.effects.len(), 2);
                assert!(matches!(
                    f.worker.effects[1],
                    CommittedEffect::ReleaseReceipt { .. }
                ));
            }
            Kind::Settle => {
                let request = &f.worker.state.requests[&key(0)];
                assert_eq!(request.generated, 2);
                assert!(request.after_settlement.is_none());
                let ready = request
                    .ready
                    .as_ref()
                    .expect("ACK must commit its continuation");
                assert_eq!(
                    (ready.phase, ready.position, ready.tokens.as_slice()),
                    (Phase::Decode, 2, &[31][..])
                );
                assert!(!f.worker.state.verify_fenced());
                assert_eq!(f.worker.effects.len(), 1);
            }
        }
        let CommittedEffect::Publication {
            event,
            after: super::effects::PublicationAfter::HeadControl,
        } = &f.worker.effects[0]
        else {
            panic!("the stale original Event must be retained without a reusable authority ticket")
        };
        assert_eq!(event, &expected_replay);
        assert_eq!(event.payload.as_ptr() as usize, original_allocation);
        let occupied = recovered_occupant.unwrap_or_else(|| {
            let Poll::Event(event) = mailbox.as_ref().unwrap().try_take() else {
                panic!("historical completion must remain in the mailbox")
            };
            event
        });
        assert_eq!(
            p4_protocol::event::encode(&occupied).unwrap(),
            historical_wire
        );
        assert!(
            matches!(mailbox.as_ref().unwrap().try_take(), Poll::Empty),
            "the stale replay must not be forwarded after its ACK removed authority"
        );
        assert!(f.worker.flush_effects().unwrap_err().contains("fenced"));
        assert_eq!(*f.trace.lock().unwrap(), native);
        assert!(matches!(mailbox.as_ref().unwrap().try_take(), Poll::Empty));
    }
}

#[test]
fn closed_mailbox_or_event_id_exhaustion_cannot_promote_locally_applied_controls() {
    for kind in [Kind::Release, Kind::Settle] {
        for closed in [false, true] {
            let mut f = fixture(kind, ReplyMode::Exact, 2);
            f.local();
            let pending_releases = f.worker.state.pending_releases.clone();
            let pending_settles = f.worker.state.pending_settlements.clone();
            let native = f.trace.lock().unwrap().clone();
            if closed {
                drop(f.mailbox.take());
            } else {
                f.worker.state.next_event = u64::MAX;
            }
            f.worker.effects.push_back(f.forward_effect());
            let queued = format!("{:?}", f.worker.effects);
            let CommittedEffect::ForwardHeadControl {
                base,
                target,
                class,
                content_type,
                body,
            } = &f.worker.effects[0]
            else {
                panic!()
            };
            let mut expected_envelope = base.clone();
            let old_id = f.worker.state.next_event;
            expected_envelope.event_id = format!("{}:llamacpp:{old_id}", base.event_id);
            expected_envelope.causation_id = Some(base.event_id.clone());
            expected_envelope.source = f.worker.endpoint.clone();
            expected_envelope.target = target.clone();
            expected_envelope.class = *class;
            expected_envelope.sequence = old_id;
            expected_envelope.payload_content_type = (*content_type).into();
            let expected_event = Event {
                envelope: expected_envelope,
                payload: body.clone(),
            };
            let allocation = body.as_ptr();
            assert!(
                f.worker
                    .flush_effects()
                    .unwrap_err()
                    .contains("could not be delivered")
            );
            assert_eq!(f.worker.state.pending_releases, pending_releases);
            assert_eq!(f.worker.state.pending_settlements, pending_settles);
            for slot in 0..2 {
                assert_eq!(f.phase(slot), ControlDispatchPhase::LocalApplied);
            }
            assert_eq!(*f.trace.lock().unwrap(), native);
            if closed {
                assert_eq!(f.worker.effects.len(), 1);
                let CommittedEffect::Publication {
                    event,
                    after: super::effects::PublicationAfter::HeadControl,
                } = &f.worker.effects[0]
                else {
                    panic!("closed publication lost its fixed Event")
                };
                assert_eq!(event, &expected_event);
                assert_eq!(event.payload.as_ptr(), allocation);
                assert_eq!(f.worker.state.next_event, old_id + 1);
            } else {
                assert_eq!(format!("{:?}", f.worker.effects), queued);
            }
            assert!(f.worker.effects_fenced);
            if !closed {
                assert_eq!(f.worker.state.next_event, u64::MAX);
            }
            assert!(f.drain().is_empty());
        }
    }
}

#[test]
fn queued_control_cannot_skip_native_and_go_straight_to_forward() {
    for kind in [Kind::Release, Kind::Settle] {
        let mut f = fixture(kind, ReplyMode::Exact, 2);
        f.rejected_without_effect(f.forward_effect(), "not locally applied");
        for slot in 0..2 {
            assert_eq!(f.phase(slot), ControlDispatchPhase::Queued);
        }
    }
}

#[test]
fn stale_scope_and_changed_pending_identity_are_rejected_before_native() {
    for kind in [Kind::Release, Kind::Settle] {
        for fault in 0..3 {
            let mut f = fixture(kind, ReplyMode::Exact, 1);
            let mut effect = f.local_effect(0);
            match &mut effect {
                CommittedEffect::Release {
                    load_generation,
                    session_id,
                    sequence,
                } => match fault {
                    0 => *load_generation = 2,
                    1 => *session_id = "unknown".into(),
                    2 => sequence.incarnation += 1,
                    _ => unreachable!(),
                },
                CommittedEffect::Settle {
                    load_generation,
                    session_id,
                    sequence,
                } => match fault {
                    0 => *load_generation = 2,
                    1 => *session_id = "unknown".into(),
                    2 => sequence.retain_from += 1,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }
            f.rejected_without_effect(
                effect,
                match fault {
                    0 => "scope is stale",
                    1 => "session is missing",
                    _ => "pending authority",
                },
            );
        }
    }
}

#[test]
fn wrong_forward_route_class_and_stale_scope_never_publish() {
    for kind in [Kind::Release, Kind::Settle] {
        for fault in 0..3 {
            let mut f = fixture(kind, ReplyMode::Exact, 2);
            f.local();
            let mut effect = f.forward_effect();
            if let CommittedEffect::ForwardHeadControl {
                target,
                class,
                body,
                ..
            } = &mut effect
            {
                match fault {
                    0 => *target = f.worker.endpoint.clone(),
                    1 => *class = EventClass::Data,
                    2 => {
                        let mut value: serde_json::Value = serde_json::from_slice(body).unwrap();
                        value["load_generation"] = 2.into();
                        *body = serde_json::to_vec(&value).unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            f.rejected_without_effect(
                effect,
                match fault {
                    0 => "declared next stage",
                    1 => "wrong event class",
                    _ => "scope is stale",
                },
            );
        }
    }
}

#[test]
fn bad_last_forward_member_preserves_all_earlier_members_and_queue() {
    for kind in [Kind::Release, Kind::Settle] {
        for fault in 0..3 {
            let mut f = fixture(kind, ReplyMode::Exact, 2);
            f.local();
            let mut effect = f.forward_effect();
            if let CommittedEffect::ForwardHeadControl { body, .. } = &mut effect {
                let mut value: serde_json::Value = serde_json::from_slice(body).unwrap();
                match fault {
                    0 => value["sequences"][1]["operation_id"] = 999.into(),
                    1 => value["sequences"][1] = value["sequences"][0].clone(),
                    2 => match kind {
                        Kind::Release => {
                            f.worker
                                .state
                                .pending_releases
                                .get_mut(&key(1))
                                .unwrap()
                                .dispatch
                                .phase = ControlDispatchPhase::Queued
                        }
                        Kind::Settle => {
                            f.worker
                                .state
                                .pending_settlements
                                .get_mut(&key(1))
                                .unwrap()
                                .dispatch
                                .phase = ControlDispatchPhase::Queued
                        }
                    },
                    _ => unreachable!(),
                }
                *body = serde_json::to_vec(&value).unwrap();
            }
            f.rejected_without_effect(
                effect,
                if fault == 0 {
                    "pending authority"
                } else {
                    "not locally applied or repeats"
                },
            );
            assert_eq!(f.phase(0), ControlDispatchPhase::LocalApplied);
        }
    }
}
