//! B2 local actor-turn regressions. SESSION/PREFILL traverse the event codec,
//! actual input queue and Worker::run; only the native Frame boundary is fake.
//! There is one real head worker and no tail: this proves a control-processing
//! opportunity between logical issues, not pipeline completion/graceful drain,
//! LOAD negotiation, EventNode transport, GPU parallelism or performance.
use super::*;
use crate::process::{ReadyInfo, ServerControl};
use crate::v2::capsule::{Invocation, PhysicalCapsule, Tensor, TensorDescriptor};
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use p4_protocol::event::{Envelope, OuterEndpoint};

#[derive(Clone, Copy)]
enum TurnCase {
    SessionBetweenIssues,
    StopDuringFirstNative,
    InputAlreadyDisconnected,
    IdleInputDisconnected,
    IdleInputAndUnloadFail,
    IdleUnloadCommandFails,
    UnloadFails,
    NativeAndUnloadFail,
    RejectedInputDisconnected,
}

#[derive(Default, Debug)]
struct Trace {
    native_rows: Vec<(String, u32, i32)>,
    second_native_saw_session_ack: Option<bool>,
    observed: Vec<Event>,
    shutdowns: usize,
    final_snapshot: String,
    snapshot_during_native_shutdown: String,
}

struct TurnNative {
    case: TurnCase,
    sender: Option<mpsc::SyncSender<WorkerInput>>,
    mailbox: Arc<CompletionMailbox>,
    trace: Arc<Mutex<Trace>>,
    stop: Arc<AtomicBool>,
    snapshot: Arc<Mutex<String>>,
}

fn address() -> Address {
    Address::tcp("127.0.0.1", 42997)
}

fn node(index: usize) -> NodeAddress {
    NodeAddress {
        agent: address().to_string(),
        node: format!("turn-{index}"),
        generation: 1,
    }
}

fn input(name: &str, content: &str, payload: Vec<u8>) -> Event {
    let event = Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: name.into(),
            correlation_id: name.into(),
            causation_id: None,
            source: Endpoint::outer(address(), "turn-output", 1),
            target: Endpoint::node(address(), "turn-0", 1),
            return_route: Some(OuterEndpoint {
                ingress_agent: address(),
                channel: "turn-output".into(),
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
    };
    p4_protocol::event::decode(&p4_protocol::event::encode(&event).unwrap()).unwrap()
}

fn session(name: &str) -> Event {
    session_generation(name, 1)
}

fn session_generation(name: &str, generation: u64) -> Event {
    input(
        name,
        SESSION_CONTENT_TYPE,
        serde_json::to_vec(&SessionCommand {
            load_generation: generation,
            session_id: "turn-session".into(),
            stages: vec![node(0), node(1)],
            stage_index: 0,
        })
        .unwrap(),
    )
}

fn request(name: &str, token: i32) -> Event {
    input(
        name,
        PREFILL_CONTENT_TYPE,
        serde_json::to_vec(&InferenceCommand {
            load_generation: 1,
            session_id: "turn-session".into(),
            request_id: name.into(),
            tokens: vec![token],
            prompt: None,
            options: String::new(),
            session_key: None,
            max_tokens: 2,
        })
        .unwrap(),
    )
}

fn is_redelivery_ack(event: &Event) -> bool {
    if event.envelope.payload_content_type != SESSION_READY_CONTENT_TYPE
        || event.envelope.causation_id.as_deref() != Some("turn-session-redelivery")
    {
        return false;
    }
    let payload: serde_json::Value = serde_json::from_slice(&event.payload).unwrap();
    assert_eq!(
        event.envelope.target,
        Endpoint::outer(address(), "turn-output", 1)
    );
    assert_eq!(payload["session_id"], "turn-session");
    assert_eq!(payload["state"], "ready");
    assert_eq!(payload["load_generation"], 1);
    true
}

fn drain(mailbox: &CompletionMailbox, trace: &mut Trace) {
    while let Poll::Event(event) = mailbox.try_take() {
        // Preserve physical forwards and telemetry too. This observer is not
        // an E2E routing pump, and does not pretend those events reached a tail.
        trace.observed.push(event);
    }
}

fn native_wire(frame: Frame) -> Result<Frame, String> {
    let limits = crate::ProtocolLimits::default();
    let bytes = frame.encode(limits).map_err(|e| e.to_string())?;
    Frame::decode(&bytes, limits).map_err(|e| e.to_string())
}

impl ServerControl for TurnNative {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        Ok(Some(ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: crate::PROTOCOL_REVISION,
            server_id: "turn-fake-no-model".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 256,
            n_batch: 1,
            n_ubatch: 1,
            n_seq_max: 2,
            upstream_commit: "fixture".into(),
            patch_set: "fixture".into(),
            backend_inventory: "no-engine".into(),
        }))
    }

    fn request(&mut self, frame: Frame) -> Result<Frame, String> {
        let frame = native_wire(frame)?;
        assert_eq!(frame.header.operation, Operation::LogicalBatch);
        let logical = LogicalBatch::decode(&frame.body).map_err(|e| format!("{e:?}"))?;
        assert_eq!(logical.0.len(), 1, "capacity one must create two issues");
        let owner = logical.0[0].owner.clone();
        assert_eq!(owner.phase, Phase::Prefill);
        assert_eq!(owner.position, 0);
        assert_eq!(owner.session_id, "turn-session");
        assert_eq!(
            owner.sequence_key,
            format!("turn-session\0{}", owner.request_id)
        );
        let expected_token = match owner.request_id.as_str() {
            "a" => 10,
            "b" => 11,
            other => panic!("unexpected native request {other}"),
        };
        assert_eq!(owner.input_token, expected_token);
        assert!(owner.output);
        let call = {
            let mut trace = self.trace.lock().unwrap();
            trace
                .native_rows
                .push((owner.request_id.clone(), owner.position, owner.input_token));
            let call = trace.native_rows.len();
            if call == 2 {
                drain(&self.mailbox, &mut trace);
                trace.second_native_saw_session_ack =
                    Some(trace.observed.iter().any(is_redelivery_ack));
            }
            call
        };
        if call == 1 {
            match self.case {
                TurnCase::SessionBetweenIssues | TurnCase::UnloadFails => self
                    .sender
                    .as_ref()
                    .unwrap()
                    .try_send(WorkerInput::Event(session("turn-session-redelivery")))
                    .map_err(|_| "valid control did not fit the bounded input queue")?,
                TurnCase::StopDuringFirstNative => {
                    self.stop.store(true, Ordering::Release);
                    self.sender.take();
                }
                TurnCase::InputAlreadyDisconnected
                | TurnCase::IdleInputDisconnected
                | TurnCase::IdleInputAndUnloadFail
                | TurnCase::IdleUnloadCommandFails
                | TurnCase::RejectedInputDisconnected => {}
                TurnCase::NativeAndUnloadFail => {
                    // The independently recorded native row was touched before
                    // the response was lost. Cleanup must not hide uncertainty.
                    self.sender.take();
                    return Err("turn-native-response-lost".into());
                }
            }
        }
        if call == 2 {
            // Keep input connected until the second issue, so the SESSION
            // ordering test does not accidentally test EOF cancellation.
            self.sender.take();
        }
        let capsule = PhysicalCapsule {
            execution_id: call as u64,
            terminal: false,
            invocation: Invocation {
                flags: 0,
                n_seq_tokens: 1,
                n_seqs: 1,
                n_seqs_unq: 1,
                n_pos: 1,
                positions: vec![owner.position as i32],
                sequence_counts: vec![1],
                sequence_ids: vec![owner.sequence_id as i32],
                output: vec![owner.output],
            },
            owners: vec![owner],
            tensors: vec![Tensor {
                descriptor: TensorDescriptor {
                    tensor_type: 0,
                    dimensions: vec![1],
                    strides: vec![4],
                    nbytes: 4,
                    view_offset: 0,
                    alias_of: None,
                    name: "turn-activation".into(),
                },
                data: (call as u32).to_le_bytes().to_vec(),
            }],
            outcomes: vec![],
        };
        native_wire(
            Frame::new(
                Operation::PhysicalResult,
                CapsuleSet(vec![capsule])
                    .encode()
                    .map_err(|e| format!("{e:?}"))?,
            )
            .map_err(|e| e.to_string())?,
        )
    }

    fn shutdown(&mut self) -> Result<(), String> {
        let mut trace = self.trace.lock().unwrap();
        trace.shutdowns += 1;
        trace.snapshot_during_native_shutdown = self.snapshot.lock().unwrap().clone();
        if matches!(
            self.case,
            TurnCase::UnloadFails
                | TurnCase::NativeAndUnloadFail
                | TurnCase::IdleInputAndUnloadFail
                | TurnCase::IdleUnloadCommandFails
        ) {
            Err("turn-native-unload-failed".into())
        } else {
            Ok(())
        }
    }
}

fn run(case: TurnCase) -> Trace {
    let (sender, receiver) = mpsc::sync_channel(8);
    let (publisher, mailbox) = completion_mailbox(32);
    let trace = Arc::new(Mutex::new(Trace::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let snapshot = Arc::new(Mutex::new(String::new()));
    let mut worker = Worker::new(
        Endpoint::node(address(), "turn-0", 1),
        receiver,
        publisher,
        Arc::clone(&snapshot),
        Arc::clone(&stop),
    )
    .with_stage_for_test(Box::new(TurnNative {
        case,
        sender: (!matches!(
            case,
            TurnCase::InputAlreadyDisconnected
                | TurnCase::IdleInputDisconnected
                | TurnCase::IdleInputAndUnloadFail
                | TurnCase::IdleUnloadCommandFails
                | TurnCase::RejectedInputDisconnected
        ))
        .then(|| sender.clone()),
        mailbox: Arc::clone(&mailbox),
        trace: Arc::clone(&trace),
        stop: Arc::clone(&stop),
        snapshot: Arc::clone(&snapshot),
    }))
    .unwrap();
    // Explicit post-LOAD fixture, not evidence of LOAD parsing/negotiation.
    worker.state.load_generation = 1;
    worker.state.physical_receives =
        crate::v2::node::physical_receive::PhysicalReceiveLedger::new(1).unwrap();
    worker.state.batch_capacity = 1;
    worker.state.physical_capacity = 1;
    worker.state.context_size = 128;
    worker.state.sequence_capacity = 2;
    worker.state.free_sequences = (0..2).collect();
    worker.state.max_atomic_sequences = 1;
    worker.state.equal_sequence_ubatch = false;
    worker.state.atomic_batch_exclusive = false;
    worker.state.min_batch_rows = 0;
    worker.state.max_open_batches = 0;
    worker.state.max_issue_rows = 0;
    worker.state.prefill_fragments = 1;
    let mut initial = vec![if matches!(case, TurnCase::RejectedInputDisconnected) {
        // Fully valid event/command encoding, rejected only by load authority.
        session_generation("turn-session-stale", 2)
    } else {
        session("turn-session-initial")
    }];
    if !matches!(
        case,
        TurnCase::IdleInputDisconnected
            | TurnCase::IdleInputAndUnloadFail
            | TurnCase::IdleUnloadCommandFails
            | TurnCase::RejectedInputDisconnected
    ) {
        initial.extend([request("a", 10), request("b", 11)]);
    }
    if matches!(case, TurnCase::IdleUnloadCommandFails) {
        initial.push(input(
            "turn-unload-fails",
            UNLOAD_CONTENT_TYPE,
            serde_json::to_vec(&UnloadCommand { load_generation: 1 }).unwrap(),
        ));
        // This is valid for the old loaded session. A native cleanup failure
        // must fence the worker before this already-queued command can ACK.
        initial.push(session("turn-session-after-failed-unload"));
    }
    for event in initial {
        sender.try_send(WorkerInput::Event(event)).unwrap();
    }
    drop(sender);
    let (done_tx, done_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        worker.run();
        done_tx.send(()).unwrap();
    });
    let finished = done_rx.recv_timeout(Duration::from_secs(2));
    if finished.is_err() {
        stop.store(true, Ordering::Release);
    }
    assert!(
        finished.is_ok(),
        "finite actual worker did not terminate: {snapshot:?}"
    );
    thread.join().unwrap();
    let mut trace = Arc::try_unwrap(trace).unwrap().into_inner().unwrap();
    trace.final_snapshot = snapshot.lock().unwrap().clone();
    drain(&mailbox, &mut trace);
    let errors = trace
        .observed
        .iter()
        .filter(|e| e.envelope.payload_content_type == ERROR_CONTENT_TYPE)
        .collect::<Vec<_>>();
    if matches!(case, TurnCase::NativeAndUnloadFail) {
        assert_eq!(
            errors.len(),
            1,
            "the one ambiguous native call must be reported"
        );
        let error: serde_json::Value = serde_json::from_slice(&errors[0].payload).unwrap();
        assert_eq!(error["code"], "LLAMA_LOGICAL_BATCH_FAILED");
        assert!(
            error["detail"]
                .as_str()
                .unwrap()
                .contains("turn-native-response-lost")
        );
    } else if matches!(case, TurnCase::IdleUnloadCommandFails) {
        assert_eq!(
            errors.len(),
            1,
            "the actual UNLOAD failure must be reported"
        );
        let error: serde_json::Value = serde_json::from_slice(&errors[0].payload).unwrap();
        assert_eq!(error["code"], "LLAMA_ADAPTER_EVENT_REJECTED");
        assert!(
            error["detail"]
                .as_str()
                .unwrap()
                .contains("stage unload failed")
                && error["detail"]
                    .as_str()
                    .unwrap()
                    .contains("turn-native-unload-failed")
        );
    } else if matches!(case, TurnCase::RejectedInputDisconnected) {
        assert_eq!(errors.len(), 1);
        let error: serde_json::Value = serde_json::from_slice(&errors[0].payload).unwrap();
        assert_eq!(error["code"], "LLAMA_ADAPTER_EVENT_REJECTED");
        assert_eq!(error["detail"], "session load generation is stale");
    } else {
        assert!(
            errors.is_empty(),
            "only supported valid control and valid native results were supplied: {trace:?}"
        );
    }
    assert_eq!(trace.shutdowns, 1);
    // Outstanding head fragments are intentionally not returned. Receiver
    // closure here is teardown, NEVER a claim of a gracefully drained pipeline.
    trace
}

fn final_work(trace: &Trace) -> serde_json::Value {
    let (_, work) = trace
        .final_snapshot
        .split_once(";work=")
        .expect("exit must preserve pre-cleanup local work evidence");
    let work = work
        .split_once(";unload_failed:")
        .map_or(work, |(work, _)| work);
    serde_json::from_str(work).expect("fixture cleanup must preserve complete work JSON")
}

#[test]
fn a_valid_control_enqueued_during_issue_is_handled_before_the_next_logical_issue() {
    let trace = run(TurnCase::SessionBetweenIssues);
    assert_eq!(
        trace.native_rows,
        [("a".into(), 0, 10), ("b".into(), 0, 11)]
    );
    assert_eq!(
        trace
            .observed
            .iter()
            .filter(|e| is_redelivery_ack(e))
            .count(),
        1,
        "the exact SESSION must produce one genuine ready acknowledgement"
    );
    assert_eq!(
        trace.second_native_saw_session_ack,
        Some(true),
        "the second logical native call overtook a queued, valid SESSION: {trace:?}"
    );
    assert!(
        trace.final_snapshot.starts_with("abandoned:input_closed"),
        "two outstanding head fragments are not a drained pipeline: {}",
        trace.final_snapshot
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 2);
    assert_eq!(work["flight_batches"], 2);
    assert_eq!(work["flight_executions"], 2);
}

#[test]
fn a_stop_requested_inside_native_does_not_start_a_second_logical_issue() {
    let trace = run(TurnCase::StopDuringFirstNative);
    assert_eq!(
        trace.native_rows,
        [("a".into(), 0, 10)],
        "stop is observed before another irreversible native mutation"
    );
    assert!(
        trace
            .final_snapshot
            .starts_with("stopped:shutdown_requested"),
        "a shutdown request is not graceful completion: {}",
        trace.final_snapshot
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 2);
    assert_eq!(work["flight_batches"], 1);
    assert_eq!(work["flight_executions"], 1);
}

#[test]
fn input_disconnect_observed_with_runnable_requests_does_not_start_native() {
    let trace = run(TurnCase::InputAlreadyDisconnected);
    assert!(
        trace.native_rows.is_empty(),
        "queued input must not authorize new native work after the input disconnect: {trace:?}"
    );
    assert!(
        trace.final_snapshot.starts_with("abandoned:input_closed"),
        "two admitted requests remain unexecuted after input loss: {}",
        trace.final_snapshot
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 2);
    assert_eq!(work["flight_batches"], 0);
    assert_eq!(work["flight_executions"], 0);
}

#[test]
fn idle_input_eof_reports_local_work_empty_without_claiming_outer_delivery() {
    let trace = run(TurnCase::IdleInputDisconnected);
    assert!(trace.native_rows.is_empty());
    assert!(trace.final_snapshot.starts_with("closed:local_work_empty;"));
    assert_eq!(trace.observed.len(), 1, "only SESSION_READY was published");
    assert_eq!(
        trace.observed[0].envelope.payload_content_type,
        SESSION_READY_CONTENT_TYPE
    );
    let work = final_work(&trace);
    for field in [
        "requests",
        "pending",
        "pending_releases",
        "pending_settlements",
        "flight_batches",
        "flight_executions",
        "open_batch_view",
        "effects",
        "active_owners",
        "active_frontiers",
    ] {
        assert_eq!(work[field], 0, "idle exit unexpectedly retained {field}");
    }
    assert_eq!(work["prepared_issue"], serde_json::Value::Null);
    assert_eq!(work["effects_fenced"], false);
    assert!(!trace.final_snapshot.contains("unload_failed"));
    assert!(
        trace
            .snapshot_during_native_shutdown
            .starts_with("closing:input_closed;"),
        "the native has not returned from cleanup, so closed cannot yet be published: {}",
        trace.snapshot_during_native_shutdown
    );
}

#[test]
fn unload_failure_appends_to_abandoned_work_instead_of_overwriting_it() {
    let trace = run(TurnCase::UnloadFails);
    assert_eq!(
        trace.native_rows,
        [("a".into(), 0, 10), ("b".into(), 0, 11)]
    );
    assert_eq!(trace.second_native_saw_session_ack, Some(true));
    assert!(
        trace
            .final_snapshot
            .starts_with("failed:cleanup;prior=abandoned:input_closed;"),
        "cleanup must not replace the reason or pre-cleanup counts: {}",
        trace.final_snapshot
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 2);
    assert_eq!(work["flight_batches"], 2);
    assert_eq!(work["flight_executions"], 2);
    let (_, failure) = trace
        .final_snapshot
        .split_once(";unload_failed:")
        .expect("native shutdown failure must survive run exit");
    assert!(failure.contains("turn-native-unload-failed"));
}

#[test]
fn unload_failure_keeps_the_original_native_failure_and_uncertain_work() {
    let trace = run(TurnCase::NativeAndUnloadFail);
    assert_eq!(trace.native_rows, [("a".into(), 0, 10)]);
    assert!(trace.final_snapshot.starts_with("failed:"));
    assert!(
        trace.final_snapshot.contains("turn-native-response-lost"),
        "the primary native failure was overwritten: {}",
        trace.final_snapshot
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 2);
    assert_eq!(work["flight_batches"], 0);
    assert_eq!(work["flight_executions"], 0);
    assert_eq!(work["prepared_issue"], "Uncertain");
    assert_eq!(work["effects_fenced"], true);
    let (_, failure) = trace
        .final_snapshot
        .split_once(";unload_failed:")
        .expect("secondary native shutdown failure must also survive");
    assert!(failure.contains("turn-native-unload-failed"));
}

#[test]
fn idle_unload_failure_is_top_level_failed_with_empty_prior_work_preserved() {
    let trace = run(TurnCase::IdleInputAndUnloadFail);
    assert!(trace.native_rows.is_empty());
    assert!(
        trace
            .final_snapshot
            .starts_with("failed:cleanup;prior=closed:local_work_empty;"),
        "cleanup failure must never retain a success prefix: {}",
        trace.final_snapshot
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 0);
    assert_eq!(work["flight_batches"], 0);
    assert_eq!(work["active_owners"], 0);
    let (_, failure) = trace
        .final_snapshot
        .split_once(";unload_failed:")
        .expect("the actual failed native cleanup must remain inspectable");
    assert!(failure.contains("turn-native-unload-failed"));
}

#[test]
fn an_idle_unload_command_failure_fences_before_a_queued_session_can_ack() {
    // Unlike IdleInputAndUnloadFail, this executes an UNLOAD command while the
    // worker is healthy and idle. This is a fatal native-cleanup boundary, NOT
    // the reversible "busy UNLOAD" refusal tested by the pipeline harness.
    let trace = run(TurnCase::IdleUnloadCommandFails);
    assert!(trace.native_rows.is_empty());
    assert_eq!(
        trace.shutdowns, 1,
        "cleanup must not retry an uncertain UNLOAD"
    );
    assert_eq!(trace.snapshot_during_native_shutdown, "unloading");
    assert!(
        trace.observed.iter().all(|event| {
            event.envelope.causation_id.as_deref() != Some("turn-session-after-failed-unload")
        }),
        "native UNLOAD failed, but the queued SESSION still produced an acknowledgement: {trace:?}"
    );
    assert!(
        trace
            .observed
            .iter()
            .all(|event| { event.envelope.payload_content_type != UNLOADED_CONTENT_TYPE }),
        "UNLOADED must not be published after failed native cleanup"
    );
    assert_eq!(
        trace
            .observed
            .iter()
            .filter(|event| { event.envelope.payload_content_type == SESSION_READY_CONTENT_TYPE })
            .count(),
        1,
        "only the initial healthy SESSION was acknowledged"
    );
    assert!(trace.final_snapshot.starts_with("failed:"));
    assert!(
        trace.final_snapshot.contains("stage unload failed")
            && trace.final_snapshot.contains("turn-native-unload-failed"),
        "the initiating UNLOAD error must remain the terminal cause: {}",
        trace.final_snapshot
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 0);
    assert_eq!(work["pending"], 0);
    assert_eq!(work["flight_batches"], 0);
    assert_eq!(work["active_owners"], 0);
    assert_eq!(work["active_frontiers"], 0);
    assert_eq!(work["effects_fenced"], true);
}

#[test]
fn a_rejected_event_then_idle_eof_retains_the_rejection_without_fatal_classification() {
    let trace = run(TurnCase::RejectedInputDisconnected);
    assert!(trace.native_rows.is_empty());
    assert!(trace.final_snapshot.starts_with("closed:local_work_empty;"));
    let (_, after_previous) = trace
        .final_snapshot
        .split_once(";previous=")
        .expect("nonfatal rejection evidence must survive local idle exit");
    let (previous, _) = after_previous.split_once(";work=").unwrap();
    assert_eq!(
        serde_json::from_str::<String>(previous).unwrap(),
        "failed:session load generation is stale"
    );
    let work = final_work(&trace);
    assert_eq!(work["requests"], 0);
    assert_eq!(work["flight_batches"], 0);
    assert_eq!(work["effects_fenced"], false);
    assert!(
        trace
            .snapshot_during_native_shutdown
            .starts_with("closing:input_closed;")
    );
}
