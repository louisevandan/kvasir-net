//! L1 prepared/attempted/accepted issue API regressions (T15/T18).
//! These call the real AdapterState and FlightLedger, not Worker::drive or
//! transport. The event-worker/fake-native path remains a separate B2 gate.

use super::flight::FlightLedger;
use super::state::{AdapterState, IssueProgress, ReadyRows, RequestState, request_key};
use crate::v2::capsule::{CapsuleSet, Invocation, PhysicalCapsule, RowOwner};
use crate::v2::logical::{LogicalBatch, LogicalRow};
use crate::v2::scheduler::Phase;
use serde_json::{Value, json};

fn request(name: &str, sequence: u32, tokens: Vec<i32>) -> RequestState {
    let mut request = crate::v2::tests::request_state(tokens);
    request.command.request_id = name.into();
    request.command.options = "{\"temperature\":0.0}".into();
    request.command.max_tokens = 8;
    request.sequence_id = Some(sequence);
    request.template.envelope.event_id = format!("submit-{name}");
    request.template.envelope.correlation_id = name.into();
    let mut reply: crate::v2::ReplySpec = serde_json::from_str(&request.reply).unwrap();
    reply.correlation_id = name.into();
    request.reply = serde_json::to_string(&reply).unwrap();
    request
}

/// Construct independent expected logical ownership from the admitted input.
/// No production planner/ownership builder is used as its own test oracle.
fn logical_rows(request: &RequestState, count: usize) -> Vec<LogicalRow> {
    let (phase, position, tokens, speculative_id) = match &request.ready {
        Some(ready) => (
            ready.phase,
            ready.position,
            ready.tokens.as_slice(),
            ready.speculative_id,
        ),
        None => (
            Phase::Prefill,
            request.prompt_issued as u32,
            &request.command.tokens[request.prompt_issued..],
            0,
        ),
    };
    (0..count)
        .map(|offset| {
            let atomic = matches!(phase, Phase::Verify | Phase::Replay);
            let output = match phase {
                Phase::Prefill => position as usize + offset + 1 == request.command.tokens.len(),
                Phase::Replay => false,
                _ => true,
            };
            LogicalRow {
                token: tokens[offset],
                owner: RowOwner {
                    incarnation: request.incarnation,
                    load_generation: request.command.load_generation,
                    session_id: request.command.session_id.clone(),
                    request_id: request.command.request_id.clone(),
                    sequence_key: request_key(
                        &request.command.session_id,
                        &request.command.request_id,
                    ),
                    reply: request.reply.clone(),
                    sequence_id: request.sequence_id.unwrap(),
                    phase,
                    position: position + offset as u32,
                    input_token: tokens[offset],
                    output,
                    max_tokens: request.command.max_tokens,
                    generated_tokens: request.generated,
                    speculative_id,
                    speculative_index: if atomic { offset as u32 } else { 0 },
                    speculative_count: if atomic { count as u32 } else { 0 },
                    options: request.command.options.clone(),
                },
            }
        })
        .collect()
}

fn fixture() -> (AdapterState, LogicalBatch) {
    let mut state = AdapterState::default();
    state.load_generation = 1;
    let mut rows = Vec::new();
    for (name, sequence, start) in [("a", 0, 10), ("b", 1, 20)] {
        let request = request(name, sequence, (start..start + 6).collect());
        rows.extend(logical_rows(&request, 2));
        state.requests.insert(request_key("session", name), request);
    }
    (state, LogicalBatch(rows))
}

fn capsule(execution: u64, rows: &[LogicalRow]) -> PhysicalCapsule {
    let owners: Vec<_> = rows.iter().map(|row| row.owner.clone()).collect();
    let sequences: std::collections::BTreeSet<_> =
        owners.iter().map(|owner| owner.sequence_id).collect();
    PhysicalCapsule {
        execution_id: execution,
        terminal: false,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: (rows.len() / sequences.len()) as u32,
            n_seqs: sequences.len() as u32,
            n_seqs_unq: sequences.len() as u32,
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
        tensors: Vec::new(),
        outcomes: Vec::new(),
    }
}

fn split(logical: &LogicalBatch, execution: u64) -> CapsuleSet {
    CapsuleSet(vec![
        capsule(execution, &logical.0[..1]),
        capsule(execution + 1, &logical.0[1..2]),
        capsule(execution + 2, &logical.0[2..]),
    ])
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    committed: Value,
    flight: FlightLedger,
    prepared: Option<(u64, IssueProgress, LogicalBatch)>,
}

fn snapshot(state: &AdapterState) -> Snapshot {
    let requests: Vec<_> = state.requests.iter().map(|(key, request)| json!({
        "key": key, "command": request.command, "sequence": request.sequence_id,
        "reply": request.reply, "cursor": request.prompt_cursor,
        "issued": request.prompt_issued, "outstanding": request.outstanding,
        "generated": request.generated, "ready": format!("{:?}", request.ready),
        "issued_work": format!("{:?}", request.issued_work),
        "continuation": match &request.after_settlement {
            None => "none".into(),
            Some(super::state::SettlementContinuation::Proposal { position, token }) => format!("proposal:{position}:{token}"),
            Some(super::state::SettlementContinuation::Replay(rows)) => format!("replay:{rows:?}"),
        },
        "fenced": state.verify_fence_matches(key),
    })).collect();
    Snapshot {
        committed: json!({
            "requests": requests, "open": state.open_batches,
            "next_ordinal": state.next_open_batch, "next_event": state.next_event,
            "pending": state.pending, "free_sequences": state.free_sequences,
            "next_speculative": state.next_speculative_id, "fenced": state.verify_fenced(),
        }),
        flight: state.flights.clone(),
        prepared: state
            .prepared_issue
            .as_ref()
            .map(|issue| (issue.ordinal, issue.progress, issue.logical.clone())),
    }
}

type RowMutation = (&'static str, fn(&mut LogicalRow));

#[test]
fn t15_planned_rows_are_checked_against_resident_authority_before_prepare() {
    let mutations: &[RowMutation] = &[
        ("sequence", |row| row.owner.sequence_id += 99),
        ("generation", |row| row.owner.load_generation += 1),
        ("key", |row| row.owner.sequence_key.push('x')),
        ("session", |row| row.owner.session_id.push('x')),
        ("request", |row| row.owner.request_id.push('x')),
        ("token", |row| row.token += 99),
        ("owner token", |row| row.owner.input_token += 99),
        ("phase", |row| row.owner.phase = Phase::Decode),
        ("position", |row| row.owner.position += 1),
        ("reply", |row| row.owner.reply.push('x')),
        ("max tokens", |row| row.owner.max_tokens += 1),
        ("generated", |row| row.owner.generated_tokens += 1),
        ("options", |row| row.owner.options = "{}".into()),
        ("output", |row| row.owner.output = !row.owner.output),
        ("unexpected atomic id", |row| row.owner.speculative_id = 7),
        ("unexpected atomic index", |row| {
            row.owner.speculative_index = 1
        }),
        ("unexpected atomic count", |row| {
            row.owner.speculative_count = 2
        }),
    ];
    for (name, corrupt) in mutations {
        for bad_index in [0, 2] {
            let (mut state, mut logical) = fixture();
            let before = snapshot(&state);
            corrupt(&mut logical.0[bad_index]);
            assert!(
                state.prepare_issue(logical).is_err(),
                "{name}, row {bad_index} was accepted"
            );
            assert_eq!(
                snapshot(&state),
                before,
                "{name}, row {bad_index} partially prepared"
            );
        }
    }
}

#[test]
fn t15_atomic_ready_rows_keep_their_round_and_complete_membership() {
    for phase in [Phase::Verify, Phase::Replay] {
        for corrupt in 0..5 {
            let mut state = AdapterState::default();
            state.load_generation = 1;
            let mut request = request("atomic", 7, vec![10, 11, 12, 13]);
            request.prompt_cursor = 4;
            request.prompt_issued = 4;
            request.generated = 1;
            request.ready = Some(ReadyRows {
                phase,
                tokens: vec![90, 91],
                position: 4,
                speculative_id: 37,
            });
            let mut logical = LogicalBatch(logical_rows(&request, 2));
            state
                .requests
                .insert(request_key("session", "atomic"), request);
            match corrupt {
                0 => logical.0[1].owner.speculative_id += 1,
                1 => logical.0[1].owner.speculative_index = 0,
                2 => logical.0[1].owner.speculative_count = 1,
                3 => {
                    logical.0.pop();
                }
                _ => logical.0[1].token += 1,
            }
            let before = snapshot(&state);
            assert!(
                state.prepare_issue(logical).is_err(),
                "{phase:?} corruption {corrupt}"
            );
            assert_eq!(snapshot(&state), before);
        }
    }
}

#[test]
fn t15_ready_decode_verify_and_replay_use_the_shared_issue_transition() {
    for phase in [Phase::Decode, Phase::Verify, Phase::Replay] {
        let mut state = AdapterState::default();
        state.load_generation = 1;
        let mut request = request("ready", 7, vec![10, 11, 12, 13]);
        request.prompt_cursor = 4;
        request.prompt_issued = 4;
        request.generated = 1;
        let tokens = if phase == Phase::Decode {
            vec![90]
        } else {
            vec![90, 91]
        };
        let count = tokens.len();
        request.ready = Some(ReadyRows {
            phase,
            tokens,
            position: 4,
            speculative_id: if phase == Phase::Decode { 0 } else { 37 },
        });
        let logical = LogicalBatch(logical_rows(&request, count));
        state
            .requests
            .insert(request_key("session", "ready"), request);
        state.prepare_issue(logical.clone()).unwrap();
        state.begin_native_issue().unwrap();
        state
            .accept_prepared_issue(&CapsuleSet(vec![capsule(11, &logical.0)]))
            .unwrap();
        let request = &state.requests[&request_key("session", "ready")];
        assert_eq!(request.outstanding, 1);
        assert_eq!(
            (
                request.prompt_cursor,
                request.prompt_issued,
                request.generated
            ),
            (4, 4, 1)
        );
        assert_eq!(request.ready.as_ref().unwrap().phase, phase);
        assert_eq!(state.verify_fenced(), phase == Phase::Verify);
    }
}

#[test]
fn t15_prepare_changes_only_progress_and_can_be_cancelled_before_attempt() {
    let (mut state, logical) = fixture();
    let before = snapshot(&state);
    assert!(state.begin_native_issue().is_err());
    state.prepare_issue(logical.clone()).unwrap();
    let prepared = snapshot(&state);
    assert_eq!(prepared.committed, before.committed);
    assert_eq!(prepared.flight, before.flight);
    assert_eq!(
        prepared.prepared,
        Some((1, IssueProgress::Prepared, logical.clone()))
    );
    assert!(state.accept_prepared_issue(&split(&logical, 11)).is_err());
    assert!(state.prepare_issue(logical).is_err());
    assert_eq!(snapshot(&state), prepared);
    state.cancel_prepared_issue().unwrap();
    assert_eq!(snapshot(&state), before);
}

#[test]
fn t15_atomic_membership_cannot_be_repartitioned_across_physical_executions() {
    for phase in [Phase::Verify, Phase::Replay] {
        let mut state = AdapterState::default();
        state.load_generation = 1;
        let mut ready = request("atomic", 0, vec![7; 4]);
        ready.prompt_cursor = 4;
        ready.prompt_issued = 4;
        ready.generated = 1;
        ready.ready = Some(ReadyRows {
            phase,
            position: 4,
            tokens: vec![8, 9],
            speculative_id: 1,
        });
        let logical = LogicalBatch(logical_rows(&ready, 2));
        state
            .requests
            .insert(request_key("session", "atomic"), ready);
        state.prepare_issue(logical.clone()).unwrap();
        state.begin_native_issue().unwrap();
        let before = snapshot(&state);
        let split = CapsuleSet(vec![
            capsule(1, &logical.0[..1]),
            capsule(2, &logical.0[1..]),
        ]);
        assert!(
            state.accept_prepared_issue(&split).is_err(),
            "atomic group was split across native executions"
        );
        assert_eq!(snapshot(&state), before);
        state
            .accept_prepared_issue(&CapsuleSet(vec![capsule(1, &logical.0)]))
            .unwrap();
    }
}

#[test]
fn t15_uncertain_native_issue_cannot_be_cancelled_or_reissued_as_new() {
    let (mut state, logical) = fixture();
    let before = snapshot(&state);
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    assert!(state.cancel_prepared_issue().is_err());
    assert!(state.begin_native_issue().is_err());
    state.mark_issue_uncertain();
    let uncertain = snapshot(&state);
    assert_eq!(uncertain.committed, before.committed);
    assert_eq!(uncertain.flight, before.flight);
    assert_eq!(
        uncertain.prepared,
        Some((1, IssueProgress::Uncertain, logical.clone()))
    );
    assert!(state.prepare_issue(logical.clone()).is_err());
    assert!(state.begin_native_issue().is_err());
    assert!(state.cancel_prepared_issue().is_err());
    assert!(state.accept_prepared_issue(&split(&logical, 11)).is_err());
    state.mark_issue_uncertain();
    assert_eq!(snapshot(&state), uncertain);
}

#[test]
fn t15_accepting_exact_physical_split_commits_one_logical_fragment_per_request() {
    let (mut state, logical) = fixture();
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    state.accept_prepared_issue(&split(&logical, 11)).unwrap();
    assert!(state.prepared_issue.is_none());
    assert_eq!(state.next_open_batch, 2);
    assert_eq!(
        state.open_batches[&1].iter().copied().collect::<Vec<_>>(),
        vec![11, 12, 13]
    );
    for request in state.requests.values() {
        assert_eq!(request.prompt_issued, 2);
        assert_eq!(request.prompt_cursor, 0);
        assert_eq!(
            request.outstanding, 1,
            "capsule count is not logical fragment count"
        );
        assert_eq!(request.generated, 0);
    }
    let after = snapshot(&state);
    assert!(state.accept_prepared_issue(&split(&logical, 11)).is_err());
    assert_eq!(snapshot(&state), after);
}

#[test]
fn t15_bad_split_preserves_prepared_request_and_execution_authority() {
    for mutation in 0..6 {
        let (mut state, logical) = fixture();
        state.prepare_issue(logical.clone()).unwrap();
        state.begin_native_issue().unwrap();
        let before = snapshot(&state);
        let mut physical = split(&logical, 11);
        match mutation {
            0 => {
                physical.0.pop();
            }
            1 => physical.0.push(physical.0[0].clone()),
            2 => physical.0[2].owners[0].input_token += 1,
            3 => physical.0[2].invocation.positions[0] += 1,
            4 => physical.0[2].terminal = true,
            _ => physical.0[2].execution_id = physical.0[0].execution_id,
        }
        assert!(
            state.accept_prepared_issue(&physical).is_err(),
            "split mutation {mutation}"
        );
        assert_eq!(
            snapshot(&state),
            before,
            "split mutation {mutation} partially committed"
        );
        state.accept_prepared_issue(&split(&logical, 11)).unwrap();
    }
}

#[test]
fn t18_issue_ordinals_refuse_zero_exhaustion_and_pending_identity_change() {
    for ordinal in [0, u64::MAX] {
        let (mut state, logical) = fixture();
        state.next_open_batch = ordinal;
        let before = snapshot(&state);
        assert!(
            state.prepare_issue(logical).is_err(),
            "invalid ordinal {ordinal} reached native preparation"
        );
        assert_eq!(snapshot(&state), before);
    }
    let (mut state, logical) = fixture();
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    state.next_open_batch = 9;
    let before = snapshot(&state);
    assert!(state.accept_prepared_issue(&split(&logical, 11)).is_err());
    assert_eq!(snapshot(&state), before);
}

#[test]
fn t18_execution_high_water_never_wraps_or_reuses_an_accepted_id() {
    for execution in [0, 1, u64::MAX] {
        // Each failure gets a fresh state: an attempted issue cannot safely
        // be cancelled merely to make the next test input possible.
        let (mut state, logical) = fixture();
        state.prepare_issue(logical.clone()).unwrap();
        state.begin_native_issue().unwrap();
        state
            .accept_prepared_issue(&CapsuleSet(vec![capsule(u64::MAX, &logical.0)]))
            .unwrap();
        let next = LogicalBatch(
            state
                .requests
                .values()
                .flat_map(|request| logical_rows(request, 2))
                .collect(),
        );
        state.prepare_issue(next.clone()).unwrap();
        state.begin_native_issue().unwrap();
        let before = snapshot(&state);
        assert!(
            state
                .accept_prepared_issue(&CapsuleSet(vec![capsule(execution, &next.0)]))
                .is_err()
        );
        assert_eq!(snapshot(&state), before);
    }
}
