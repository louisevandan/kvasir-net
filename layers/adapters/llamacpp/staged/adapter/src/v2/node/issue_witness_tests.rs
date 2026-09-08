//! Actual L1 prepare/begin/accept authority tests for the issued-work witness.
//! Native results are independent literal capsules. This is not Worker::run,
//! a transport test or proof that a simulator executes native physical work.

use super::flight::FlightLedger;
use super::state::{AdapterState, RequestState, request_key};
use crate::v2::capsule::{CapsuleSet, Invocation, PhysicalCapsule, RowOwner};
use crate::v2::issue_witness::{IssueAuthority, IssueWitness};
use crate::v2::logical::{LogicalBatch, LogicalRow};
use crate::v2::scheduler::{Demand, Phase, Scheduler};
use crate::v2::{PREFILL_CONTENT_TYPE, ReplySpec};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, EventClass, OuterEndpoint};
use serde_json::{Value, json};

const SESSION: &str = "witness-session";

fn authority(name: &str, sequence: u32) -> IssueAuthority {
    IssueAuthority {
        head: Endpoint::node(Address::tcp("127.0.0.1", 42001), "n0", 1),
        outer: OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 42000),
            channel: "proof".into(),
            connection_generation: 7,
        },
        load_generation: 1,
        session_id: SESSION.into(),
        request_id: name.into(),
        submission_event_id: format!("submit-{name}"),
        sequence_id: sequence,
        incarnation: u64::from(sequence) + 17,
    }
}

fn request(name: &str, sequence: u32) -> RequestState {
    let mut request = crate::v2::tests::request_state((10..18).collect());
    let authority = authority(name, sequence);
    request.input_mut_for_test().command.session_id = SESSION.into();
    request.input_mut_for_test().command.request_id = name.into();
    request.input_mut_for_test().command.options = "{}".into();
    request.input_mut_for_test().command.max_tokens = 8;
    request.incarnation = authority.incarnation;
    request.sequence_id = Some(sequence);
    request.input_mut_for_test().template.envelope.event_id = authority.submission_event_id;
    request
        .input_mut_for_test()
        .template
        .envelope
        .correlation_id = name.into();
    request.input_mut_for_test().template.envelope.source =
        Endpoint::Outer(authority.outer.clone());
    request.input_mut_for_test().template.envelope.target = authority.head;
    request.input_mut_for_test().template.envelope.return_route = Some(authority.outer.clone());
    request.input_mut_for_test().template.envelope.class = EventClass::Data;
    request
        .input_mut_for_test()
        .template
        .envelope
        .payload_content_type = PREFILL_CONTENT_TYPE.into();
    request
        .input_mut_for_test()
        .template
        .envelope
        .deadline_unix_ms = Some(123456789);
    let payload = serde_json::to_vec(&request.command).unwrap();
    request.input_mut_for_test().template.payload = payload;
    request.input_mut_for_test().reply = serde_json::to_string(&ReplySpec {
        ingress_agent: authority.outer.ingress_agent.to_string(),
        channel: authority.outer.channel,
        connection_generation: authority.outer.connection_generation,
        correlation_id: name.into(),
        deadline_unix_ms: Some(123456789),
    })
    .unwrap();
    request
}

fn rows(request: &RequestState, count: usize) -> Vec<LogicalRow> {
    (request.prompt_issued..request.prompt_issued + count)
        .map(|position| LogicalRow {
            token: request.command.tokens[position],
            owner: RowOwner {
                incarnation: request.incarnation,
                load_generation: 1,
                session_id: SESSION.into(),
                request_id: request.command.request_id.clone(),
                sequence_key: request_key(SESSION, &request.command.request_id),
                reply: request.reply.clone(),
                sequence_id: request.sequence_id.unwrap(),
                phase: Phase::Prefill,
                position: position as u32,
                input_token: request.command.tokens[position],
                output: position + 1 == request.command.tokens.len(),
                max_tokens: 8,
                generated_tokens: request.generated,
                speculative_id: 0,
                speculative_index: 0,
                speculative_count: 0,
                options: "{}".into(),
            },
        })
        .collect()
}

fn fixture(two: bool) -> (AdapterState, LogicalBatch) {
    let mut state = AdapterState::default();
    state.load_generation = 1;
    for (name, sequence) in if two {
        vec![("a", 0), ("b", 1)]
    } else {
        vec![("a", 0)]
    } {
        state
            .requests
            .insert(request_key(SESSION, name), request(name, sequence));
    }
    let logical = LogicalBatch(
        state
            .requests
            .values()
            .flat_map(|request| rows(request, 4))
            .collect(),
    );
    (state, logical)
}

fn capsule(execution_id: u64, rows: &[LogicalRow]) -> PhysicalCapsule {
    let sequence = rows[0].owner.sequence_id;
    assert!(rows.iter().all(|row| row.owner.sequence_id == sequence));
    PhysicalCapsule {
        execution_id,
        terminal: false,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: rows.len() as u32,
            n_seqs: 1,
            n_seqs_unq: 1,
            n_pos: 1,
            positions: rows.iter().map(|row| row.owner.position as i32).collect(),
            sequence_counts: vec![1; rows.len()],
            sequence_ids: vec![sequence as i32; rows.len()],
            output: rows.iter().map(|row| row.owner.output).collect(),
        },
        owners: rows.iter().map(|row| row.owner.clone()).collect(),
        tensors: Vec::new(),
        outcomes: Vec::new(),
    }
}

fn split(logical: &LogicalBatch, first: u64) -> CapsuleSet {
    let boundaries = if logical.0.len() == 4 {
        [0, 2, 3, 4]
    } else {
        [0, 2, 4, 8]
    };
    CapsuleSet(
        (0..3)
            .map(|index| {
                capsule(
                    first + index as u64,
                    &logical.0[boundaries[index]..boundaries[index + 1]],
                )
            })
            .collect(),
    )
}

fn request_snapshot(request: &RequestState) -> Value {
    json!({
        "command": request.command,
        "incarnation": request.incarnation,
        "sequence_id": request.sequence_id,
        "template": format!("{:?}", request.template),
        "reply": request.reply,
        "prompt_cursor": request.prompt_cursor,
        "prompt_issued": request.prompt_issued,
        "outstanding": request.outstanding,
        "generated": request.generated,
        "ready": format!("{:?}", request.ready),
        "after_settlement": match &request.after_settlement {
            None => "none".into(),
            Some(super::state::SettlementContinuation::Proposal { position, token }) => format!("proposal:{position}:{token}"),
            Some(super::state::SettlementContinuation::Replay(rows)) => format!("replay:{rows:?}"),
        },
        "issued_work": request.issued_work.map(|work| json!({
            "count": work.issue_count(), "ordinal": work.last_ordinal(),
            "digest": work.digest(), "authority_digest": work.authority_digest(),
        })),
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    committed: Value,
    prepared: Option<Value>,
    flight: FlightLedger,
}

fn snapshot(state: &AdapterState) -> Snapshot {
    Snapshot {
        committed: json!({
            "requests": state.requests.iter().map(|(key, request)| (key, request_snapshot(request))).collect::<Vec<_>>(),
            "open": state.open_batches, "next_ordinal": state.next_open_batch,
            "next_event": state.next_event, "next_incarnation": state.next_incarnation,
            "next_operation": state.next_control_operation, "next_speculative": state.next_speculative_id,
            "pending": state.pending, "free_sequences": state.free_sequences,
            "releases": format!("{:?}", state.pending_releases),
            "settlements": format!("{:?}", state.pending_settlements),
            "fences": state.requests.keys().map(|key| (key, state.verify_fence_matches(key))).collect::<Vec<_>>(),
            "owners": format!("{:?}", state.stage_owners),
            "frontiers": format!("{:?}", state.stage_frontiers),
            "physical_receipts": format!("{:?}", state.physical_receives),
        }),
        prepared: state.prepared_issue.as_ref().map(|issue| json!({
            "ordinal": issue.ordinal, "progress": format!("{:?}", issue.progress),
            "logical": format!("{:?}", issue.logical),
            "candidates": issue.candidate_requests().iter().map(|(key, request)| (key, request_snapshot(request))).collect::<Vec<_>>(),
        })),
        flight: state.flights.clone(),
    }
}

fn no_witnesses(state: &AdapterState) {
    assert!(
        state
            .requests
            .values()
            .all(|request| request.issued_work.is_none())
    );
    if let Some(prepared) = &state.prepared_issue {
        assert!(
            prepared
                .candidate_requests()
                .values()
                .all(|request| request.issued_work.is_none())
        );
    }
}

fn assert_literal_vector(work: IssueWitness, step: usize) {
    // Independently encoded literal authority and execution/phase/position
    // inputs, hashed by Node crypto in generate_vectors.mjs. The expected
    // digest is not produced by RequestState or IssueWitness::advanced.
    let vectors: Value =
        serde_json::from_str(include_str!("../issue_witness/vectors-v1.json")).unwrap();
    assert_eq!(vectors["format"], 1);
    let expected = &vectors["accept"];
    let hex =
        |bytes: [u8; 32]| -> String { bytes.iter().map(|byte| format!("{byte:02x}")).collect() };
    assert_eq!(hex(work.authority_digest()), expected["authority_digest"]);
    assert_eq!(hex(work.digest()), expected["steps"][step]["digest"]);
    assert_eq!(
        work.issue_count(),
        expected["steps"][step]["issue_count"].as_u64().unwrap()
    );
    assert_eq!(
        work.last_ordinal(),
        expected["steps"][step]["logical_ordinal"].as_u64().unwrap()
    );
}

#[test]
fn prepared_attempted_and_uncertain_work_is_not_accepted_evidence() {
    let (mut state, logical) = fixture(false);
    let before = snapshot(&state);
    state.prepare_issue(logical.clone()).unwrap();
    no_witnesses(&state);
    assert_eq!(snapshot(&state).committed, before.committed);
    state.cancel_prepared_issue().unwrap();
    assert_eq!(snapshot(&state), before);
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    no_witnesses(&state);
    state.mark_issue_uncertain();
    let uncertain = snapshot(&state);
    no_witnesses(&state);
    assert!(state.accept_prepared_issue(&split(&logical, 11)).is_err());
    assert_eq!(snapshot(&state), uncertain);
}

#[test]
fn one_accepted_logical_issue_can_bind_three_physical_executions() {
    let (mut state, logical) = fixture(false);
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    state.accept_prepared_issue(&split(&logical, 11)).unwrap();
    let request = &state.requests[&request_key(SESSION, "a")];
    let work = request
        .issued_work
        .expect("accepted native work must be witnessed");
    assert_literal_vector(work, 0);
    assert_eq!(work.issue_count(), 1);
    assert_eq!(work.last_ordinal(), 1);
    assert_eq!(request.outstanding, 1);
    assert_eq!(state.flights.active_counts(), (1, 3));
    assert_eq!(
        state.open_batches[&1].iter().copied().collect::<Vec<_>>(),
        vec![11, 12, 13]
    );
    let accepted = snapshot(&state);
    assert!(state.accept_prepared_issue(&split(&logical, 11)).is_err());
    assert_eq!(snapshot(&state), accepted);
}

#[test]
fn other_requests_issues_create_a_valid_gap_without_inflating_this_witness() {
    let (mut state, logical) = fixture(false);
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    state.accept_prepared_issue(&split(&logical, 11)).unwrap();
    let first = state.requests[&request_key(SESSION, "a")]
        .issued_work
        .unwrap();
    assert_literal_vector(first, 0);
    state
        .requests
        .insert(request_key(SESSION, "b"), request("b", 1));
    // Consume real logical ordinals 2 and 3 for B rather than assigning a
    // counter to manufacture a gap. A's evidence must stay byte-identical.
    for execution in [14, 15] {
        let logical = LogicalBatch(rows(&state.requests[&request_key(SESSION, "b")], 4));
        state.prepare_issue(logical.clone()).unwrap();
        state.begin_native_issue().unwrap();
        state
            .accept_prepared_issue(&CapsuleSet(vec![capsule(execution, &logical.0)]))
            .unwrap();
        assert_eq!(
            state.requests[&request_key(SESSION, "a")].issued_work,
            Some(first)
        );
    }
    assert_eq!(state.next_open_batch, 4);
    let logical = LogicalBatch(rows(&state.requests[&request_key(SESSION, "a")], 4));
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    state
        .accept_prepared_issue(&CapsuleSet(vec![
            capsule(21, &logical.0[..2]),
            capsule(22, &logical.0[2..]),
        ]))
        .unwrap();
    let work = state.requests[&request_key(SESSION, "a")]
        .issued_work
        .unwrap();
    assert_literal_vector(work, 1);
    assert_eq!((work.issue_count(), work.last_ordinal()), (2, 4));
    assert_eq!(state.flights.active_counts(), (4, 7));
}

#[test]
fn later_count_overflow_preserves_all_witnesses_and_prepared_candidates() {
    let (mut state, logical) = fixture(true);
    state.next_open_batch = 2;
    // Explicit fault injection into a test-only private-counter constructor.
    // This is not a claim to have executed u64::MAX legitimate prior issues.
    state
        .requests
        .get_mut(&request_key(SESSION, "b"))
        .unwrap()
        .issued_work = Some(
        IssueWitness::new(&authority("b", 1))
            .unwrap()
            .with_test_counters(u64::MAX, 1),
    );
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    let before = snapshot(&state);
    assert_eq!(
        state
            .accept_prepared_issue(&split(&logical, 11))
            .unwrap_err(),
        "issued-work count exhausted"
    );
    assert_eq!(snapshot(&state), before);
    assert!(
        state.requests[&request_key(SESSION, "a")]
            .issued_work
            .is_none()
    );
}

#[test]
fn refused_physical_split_preserves_committed_and_prepared_witnesses() {
    for corruption in 0..3 {
        let (mut state, logical) = fixture(false);
        state.prepare_issue(logical.clone()).unwrap();
        state.begin_native_issue().unwrap();
        let before = snapshot(&state);
        let mut physical = split(&logical, 11);
        match corruption {
            0 => {
                physical.0.pop();
            }
            1 => physical.0.push(physical.0[0].clone()),
            _ => physical.0[2].owners[0].input_token += 1,
        }
        assert!(
            state.accept_prepared_issue(&physical).is_err(),
            "corruption {corruption}"
        );
        assert_eq!(snapshot(&state), before, "corruption {corruption}");
        no_witnesses(&state);
    }
}

#[test]
fn later_bad_original_provenance_cannot_commit_the_first_requests_witness() {
    let (mut state, _) = fixture(true);
    // RequestState shares its input behind an Arc and exposes it read-only,
    // so this fixture has to ask for the copy-on-write explicitly. Mutating
    // through Deref would silently need DerefMut, which production must not
    // have.
    state
        .requests
        .get_mut(&request_key(SESSION, "b"))
        .unwrap()
        .input_mut_for_test()
        .template
        .envelope
        .return_route = None;
    let logical = LogicalBatch(
        state
            .requests
            .values()
            .flat_map(|request| rows(request, 4))
            .collect(),
    );
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    let before = snapshot(&state);
    assert_eq!(
        state
            .accept_prepared_issue(&split(&logical, 11))
            .unwrap_err(),
        "issued work requires the original OUTER route"
    );
    assert_eq!(snapshot(&state), before);
    no_witnesses(&state);
}

#[test]
fn a_witness_from_another_request_cannot_be_rebound_during_acceptance() {
    let (mut state, logical) = fixture(true);
    state
        .requests
        .get_mut(&request_key(SESSION, "b"))
        .unwrap()
        .issued_work = Some(IssueWitness::new(&authority("a", 0)).unwrap());
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    let before = snapshot(&state);
    assert!(state.accept_prepared_issue(&split(&logical, 11)).is_err());
    assert_eq!(snapshot(&state), before);
    assert!(
        state.requests[&request_key(SESSION, "a")]
            .issued_work
            .is_none()
    );
}

#[test]
fn reused_native_execution_registration_cannot_publish_a_candidate_witness() {
    let (mut state, logical) = fixture(false);
    // Real flight registration, deliberately without the request acceptance
    // path: it must not invent an issued-work transcript for the request.
    state.register_issued_batch(&split(&logical, 100)).unwrap();
    no_witnesses(&state);
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    let before = snapshot(&state);
    assert_eq!(
        state
            .accept_prepared_issue(&split(&logical, 11))
            .unwrap_err(),
        "invalid or reused physical issue identity"
    );
    assert_eq!(snapshot(&state), before);
    no_witnesses(&state);
}

#[test]
fn returns_and_selector_bookkeeping_do_not_mint_new_issue_evidence() {
    let (mut state, logical) = fixture(false);
    let before = snapshot(&state);
    let mut scheduler = Scheduler::new();
    let plan = scheduler
        .prepare_plan(
            &[Demand {
                request_id: "a".into(),
                sequence_id: 0,
                compatibility: "fixture".into(),
                phase: Phase::Prefill,
                available_rows: 8,
                atomic: false,
            }],
            4,
        )
        .unwrap();
    assert_eq!(scheduler.commit_plan(plan).unwrap()[0].rows, 4);
    assert_eq!(snapshot(&state), before);
    // This is the actual shared transition used by the completion model, not
    // a claim that the test runs Simulation or accepts a native result.
    let mut modeled = request("modeled", 2);
    modeled.issue_fragment(Phase::Prefill, 4).unwrap();
    assert!(modeled.issued_work.is_none());
    modeled.settle_fragment(Phase::Prefill, 4).unwrap();
    assert!(modeled.issued_work.is_none());
    state.prepare_issue(logical.clone()).unwrap();
    state.begin_native_issue().unwrap();
    let mut terminal = split(&logical, 11);
    state.accept_prepared_issue(&terminal).unwrap();
    let accepted = state.requests[&request_key(SESSION, "a")].issued_work;
    for capsule in &mut terminal.0 {
        capsule.terminal = true;
    }
    let returned = state.flights.prepare_return(&terminal).unwrap();
    assert_eq!(returned.fragments.len(), 1);
    state.commit_flight_return(returned);
    let request = state.requests.get_mut(&request_key(SESSION, "a")).unwrap();
    request.settle_fragment(Phase::Prefill, 4).unwrap();
    assert_eq!(request.issued_work, accepted);
    assert_eq!(state.flights.active_counts(), (0, 0));
}
