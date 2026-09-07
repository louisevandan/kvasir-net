//! The observer reads the actual Worker::run state; it cannot mutate it. The
//! native fixture logs the real LogicalBatch/PhysicalResult bytes independently
//! of RequestState and the witness. These are post-LOAD, model-free tests, not
//! wire completion-proof or native llama/GPU evidence.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeFault {
    FailSecond,
    ReuseExecution,
}

impl NativeFault {
    pub(super) fn error_code(self) -> &'static str {
        match self {
            Self::FailSecond => "LLAMA_LOGICAL_BATCH_FAILED",
            Self::ReuseExecution => "LLAMA_PHYSICAL_RESULT_INVALID",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct NativeRecord {
    pub input: Vec<u8>,
    pub result: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestView {
    submission: String,
    slot: Option<u32>,
    incarnation: u64,
    witness: Option<crate::v2::issue_witness::IssueWitness>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    point: &'static str,
    requests: BTreeMap<String, RequestView>,
    prepared: Option<String>,
}

type Observations = Arc<Mutex<Vec<Snapshot>>>;

fn observer() -> (IssueObserver, Observations) {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let stored = Arc::clone(&observations);
    let observer: IssueObserver = Arc::new(move |point, state| {
        let snapshot = Snapshot {
            point,
            requests: state
                .requests
                .values()
                .map(|request| {
                    (
                        request.command.request_id.clone(),
                        RequestView {
                            submission: request.template.envelope.event_id.clone(),
                            slot: request.sequence_id,
                            incarnation: request.incarnation,
                            witness: request.issued_work,
                        },
                    )
                })
                .collect(),
            prepared: state
                .prepared_issue
                .as_ref()
                .map(|issue| format!("{:?}", issue.progress)),
        };
        let mut stored = stored.lock().unwrap();
        assert!(
            stored.len() < 4096,
            "test-only observation history is bounded"
        );
        stored.push(snapshot);
    });
    (observer, observations)
}

fn snapshots(observations: &Observations, point: &str) -> Vec<Snapshot> {
    observations
        .lock()
        .unwrap()
        .iter()
        .filter(|value| value.point == point)
        .cloned()
        .collect()
}

fn with_inputs(
    stages: usize,
    capacity: usize,
    inputs: &[Event],
    fault: Option<NativeFault>,
) -> (Harness, Observations) {
    let (observer, observations) = observer();
    (
        Harness::observed_events(stages, 1, capacity, inputs, 0, None, Some(observer), fault),
        observations,
    )
}

fn assert_request_authority(value: &RequestView, original: &Event, slot: u32, incarnation: u64) {
    assert_eq!(value.submission, original.envelope.event_id);
    assert_eq!(value.slot, Some(slot));
    assert_eq!(value.incarnation, incarnation);
}

fn assert_literal_vector(
    witness: &crate::v2::issue_witness::IssueWitness,
    name: &str,
    index: usize,
) {
    // Independently generated from literal authorities/rows by Node crypto.
    // No production IssueWitness::new/advanced/projection helper is an oracle.
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../../issue_witness/vectors-v1.json")).unwrap();
    let vector = &vectors[name];
    let hex = |bytes: [u8; 32]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    assert_eq!(
        hex(witness.authority_digest()),
        vector["authority_digest"]
            .as_str()
            .expect("independent authority vector")
    );
    assert_eq!(
        hex(witness.digest()),
        vector["steps"][index]["digest"]
            .as_str()
            .expect("independent issued-work vector")
    );
}

#[test]
fn actual_run_issues_count_logical_work_not_native_physical_chunks() {
    for stages in [2, 4, 8] {
        let command = request("one", 7, 5);
        let input = submission_event(&command, 1, default_route());
        let (mut h, observations) = with_inputs(stages, 8, std::slice::from_ref(&input), None);
        h.finish(&[command]);
        release_notifications::assert_complete(&h);
        let approved = snapshots(&observations, "after_issue_accepted");
        let before = snapshots(&observations, "before_native_issue");
        let tails = snapshots(&observations, "before_tail_settlement");
        assert_eq!(approved.len(), 6);
        assert_eq!(before.len(), 6);
        assert_eq!(tails.len(), 6);
        let native = h.nodes[0].native.lock().unwrap();
        assert_eq!(native.issued_native.len(), 6);
        // Fixed independent execution geometry for batch=4, ubatch=2. Native
        // records must match it before the witness is checked against it.
        let expected: &[&[(u64, Phase, &[u32])]] = &[
            &[(1, Phase::Prefill, &[0, 1]), (2, Phase::Prefill, &[2, 3])],
            &[(3, Phase::Prefill, &[4, 5]), (4, Phase::Prefill, &[6])],
            &[(5, Phase::Decode, &[7])],
            &[(6, Phase::Decode, &[8])],
            &[(7, Phase::Decode, &[9])],
            &[(8, Phase::Decode, &[10])],
        ];
        for (index, record) in native.issued_native.iter().enumerate() {
            let logical = LogicalBatch::decode(&record.input).unwrap();
            let result = CapsuleSet::decode(record.result.as_ref().unwrap()).unwrap();
            let actual: Vec<_> = result
                .0
                .iter()
                .map(|capsule| {
                    let first = &capsule.owners[0];
                    assert!(capsule.owners.iter().all(|owner| owner.request_id == "one"
                        && owner.sequence_id == 0
                        && owner.incarnation == 1
                        && owner.phase == first.phase));
                    (
                        capsule.execution_id,
                        first.phase,
                        capsule
                            .owners
                            .iter()
                            .map(|owner| owner.position)
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();
            let expected: Vec<_> = expected[index]
                .iter()
                .map(|(id, phase, positions)| (*id, *phase, positions.to_vec()))
                .collect();
            assert_eq!(actual, expected);
            assert_eq!(
                logical.0.len(),
                result
                    .0
                    .iter()
                    .map(|capsule| capsule.owners.len())
                    .sum::<usize>()
            );
            assert_request_authority(&approved[index].requests["one"], &input, 0, 1);
            let witness = approved[index].requests["one"]
                .witness
                .as_ref()
                .expect("native success must be accepted into the request witness");
            assert_eq!(witness.issue_count(), index as u64 + 1);
            assert_eq!(witness.last_ordinal(), index as u64 + 1);
            assert_literal_vector(witness, "ordinary", index);
            assert_eq!(tails[index].requests["one"].witness.as_ref(), Some(witness));
            if index == 0 {
                assert!(
                    before[index].requests["one"].witness.is_none(),
                    "prepared native work is not yet accepted"
                );
            } else {
                assert_eq!(
                    before[index].requests["one"].witness,
                    approved[index - 1].requests["one"].witness
                );
                assert_ne!(
                    witness.digest(),
                    approved[index - 1].requests["one"]
                        .witness
                        .as_ref()
                        .unwrap()
                        .digest()
                );
            }
        }
    }
}

#[test]
fn actual_run_mixed_owners_keep_distinct_issue_authority_during_full_and_duplicate_return() {
    for stages in [2, 4] {
        let commands = [
            request("issue-owner-a", 1, 1),
            request("issue-owner-b", 1, 1),
        ];
        let inputs: Vec<_> = commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                submission_event(
                    command,
                    1,
                    OuterEndpoint {
                        ingress_agent: Address::tcp("127.0.0.1", 42991 + index as u16),
                        channel: format!("issue-owner-{}", if index == 0 { "a" } else { "b" }),
                        connection_generation: 11 + index as u64,
                    },
                )
            })
            .collect();
        let (mut h, observations) = with_inputs(stages, 1, &inputs, None);
        // Before this pump drains anything, SESSION_READY occupies the head's
        // sole completion slot. The first accepted Forward must really wait.
        let deadline = Instant::now() + Duration::from_secs(2);
        while snapshots(&observations, "after_issue_accepted").len() != 1
            || *h.nodes[0].snapshot.lock().unwrap() != "completion_queue_full:waiting"
        {
            assert!(
                Instant::now() < deadline,
                "accepted issue did not reach actual completion Full"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(h.nodes[0].native.lock().unwrap().logical_calls, 1);
        h.hold_tail = true;
        h.until("one mixed terminal held before head settlement", |h| {
            !h.held_tail.is_empty()
        });
        assert_eq!(h.held_tail.len(), 1);
        let mut duplicate = h.held_tail.front().unwrap().clone();
        let terminal = CapsuleSet::decode(&duplicate.payload).unwrap();
        assert_eq!(terminal.0.len(), 1);
        assert_eq!(terminal.0[0].owners.len(), 2);
        assert_eq!(terminal.0[0].outcomes.len(), 2);
        let approved = snapshots(&observations, "after_issue_accepted");
        assert_eq!(approved.len(), 1);
        for (index, command) in commands.iter().enumerate() {
            let owner = terminal.0[0]
                .owners
                .iter()
                .find(|owner| owner.request_id == command.request_id)
                .unwrap();
            assert_eq!(owner.sequence_id, index as u32);
            assert_eq!(owner.incarnation, index as u64 + 1);
            let value = &approved[0].requests[&command.request_id];
            assert_request_authority(value, &inputs[index], owner.sequence_id, owner.incarnation);
            let witness = value.witness.as_ref().unwrap();
            assert_eq!(witness.issue_count(), 1);
            assert_eq!(witness.last_ordinal(), 1);
            assert_literal_vector(witness, if index == 0 { "mixed_a" } else { "mixed_b" }, 0);
        }
        assert_ne!(
            approved[0].requests["issue-owner-a"]
                .witness
                .as_ref()
                .unwrap()
                .digest(),
            approved[0].requests["issue-owner-b"]
                .witness
                .as_ref()
                .unwrap()
                .digest()
        );
        h.resume_tail();
        h.finish(&commands);
        release_notifications::assert_complete(&h);
        let native_calls: Vec<_> = h
            .nodes
            .iter()
            .map(|node| {
                let native = node.native.lock().unwrap();
                (
                    native.logical_calls,
                    native.physical_calls,
                    native.sampler_calls,
                    native.release_bodies.clone(),
                )
            })
            .collect();
        let output_count = h.outputs.len();
        duplicate
            .envelope
            .event_id
            .push_str(":fresh-delivery-duplicate");
        h.pending.push_back(event_wire(duplicate));
        h.until("duplicate terminal is handled by real head", |_| {
            snapshots(&observations, "before_tail_settlement").len() == 2
        });
        h.pump_for(Duration::from_millis(10));
        assert_eq!(snapshots(&observations, "after_issue_accepted"), approved);
        assert_eq!(h.outputs.len(), output_count);
        for (node, before) in h.nodes.iter().zip(native_calls) {
            let native = node.native.lock().unwrap();
            assert_eq!(
                (
                    native.logical_calls,
                    native.physical_calls,
                    native.sampler_calls,
                    native.release_bodies.clone()
                ),
                before
            );
        }
    }
}

#[test]
fn actual_run_native_failure_or_invalid_split_cannot_advance_the_last_accepted_witness() {
    for fault in [NativeFault::FailSecond, NativeFault::ReuseExecution] {
        let command = request("one", 7, 5);
        let input = submission_event(&command, 1, default_route());
        let (mut h, observations) = with_inputs(2, 8, &[input], Some(fault));
        h.until("failed head stops after its second native attempt", |h| {
            h.nodes[0].thread.as_ref().unwrap().is_finished()
        });
        // Drain the actual failure publication without replacing it with a
        // handler return value. Only this fixture's exact error is permitted.
        h.pump_for(Duration::from_millis(5));
        let accepted = snapshots(&observations, "after_issue_accepted");
        assert_eq!(accepted.len(), 1);
        assert_eq!(
            accepted[0].requests["one"]
                .witness
                .as_ref()
                .expect("the first issue was really accepted")
                .issue_count(),
            1
        );
        assert_literal_vector(
            accepted[0].requests["one"].witness.as_ref().unwrap(),
            "ordinary",
            0,
        );
        let before = snapshots(&observations, "before_native_issue");
        assert_eq!(before.len(), 2);
        assert!(before[0].requests["one"].witness.is_none());
        assert_eq!(
            before[1].requests["one"].witness,
            accepted[0].requests["one"].witness
        );
        let stopped = snapshots(&observations, "run_stopping");
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].prepared.as_deref(), Some("Uncertain"));
        assert_eq!(
            stopped[0].requests["one"].witness,
            accepted[0].requests["one"].witness
        );
        let errors: Vec<_> = h
            .received
            .iter()
            .filter(|event| event.envelope.payload_content_type == ERROR_CONTENT_TYPE)
            .collect();
        assert_eq!(errors.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&errors[0].payload).unwrap();
        assert_eq!(body["code"], fault.error_code());
        assert_eq!(
            body["detail"],
            match fault {
                NativeFault::FailSecond =>
                    "stage request failed: Process(RequestFailed(\"issue-witness fixture lost the second native reply\"))",
                NativeFault::ReuseExecution => "invalid or reused physical issue identity",
            }
        );
        assert!(h.outputs.is_empty());
        let head = h.nodes[0].native.lock().unwrap();
        assert_eq!(head.logical_calls, 2);
        assert_eq!(head.issued_native.len(), 2);
        assert_eq!(
            head.written.values().next().unwrap().len(),
            7,
            "second native really touched KV before returning its bad/lost reply"
        );
        assert!(head.releases.is_empty());
        assert_eq!(
            h.nodes[1]
                .native
                .lock()
                .unwrap()
                .written
                .values()
                .next()
                .unwrap()
                .len(),
            4
        );
    }
}
