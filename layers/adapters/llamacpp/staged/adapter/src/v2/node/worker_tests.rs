//! The worker's settlement path, driven through a real capsule.
//!
//! `settle_fragment` is shared with the simulator, and a unit test on it says
//! both call the same function. It does not say the worker reaches it, or that
//! its refusals survive the decode, the owner grouping and the session checks
//! that stand in front of it - and until this file, nothing did: deleting the
//! bound from the shared transition broke one unit test and no worker test at
//! all.
//!
//! So these drive `Worker::tail` with an encoded `CapsuleSet`, which is what
//! arrives from the stage over the wire.

use super::state::{AdapterState, PipelineSession, RequestState};
use super::worker::Worker;
use crate::v2::capsule::{CapsuleSet, Invocation, PhysicalCapsule, RowOwner};
use crate::v2::commands::{NodeAddress, SessionCommand};
use crate::v2::scheduler::Phase;
use p4_adapter::node_adapter::{CompletionMailbox, Poll, completion_mailbox};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

const SESSION: &str = "session";
const REQUEST: &str = "request";
const GENERATION: u64 = 1;

fn address() -> NodeAddress {
    NodeAddress {
        agent: "127.0.0.1:42001".into(),
        node: "n0".into(),
        generation: 1,
    }
}

fn worker_with(request: RequestState) -> Worker {
    worker_and_mailbox(vec![request]).0
}

fn worker_and_mailbox(requests: Vec<RequestState>) -> (Worker, Arc<CompletionMailbox>) {
    worker_and_mailbox_at(requests, "n0")
}

fn worker_and_mailbox_at(
    requests: Vec<RequestState>,
    node: &str,
) -> (Worker, Arc<CompletionMailbox>) {
    let (worker, mailbox, _) = worker_with_status(requests, node, 8);
    (worker, mailbox)
}

fn worker_with_status(
    requests: Vec<RequestState>,
    node: &str,
    completion_capacity: usize,
) -> (Worker, Arc<CompletionMailbox>, Arc<Mutex<String>>) {
    let own = Address::tcp("127.0.0.1", 42001);
    let endpoint = Endpoint::node(own, node, 1);
    let (_input, receiver) = std::sync::mpsc::channel();
    let (publisher, mailbox) = completion_mailbox(completion_capacity);
    let snapshot = Arc::new(Mutex::new(String::new()));
    let mut worker = Worker::new(
        endpoint.clone(),
        receiver,
        publisher,
        Arc::clone(&snapshot),
        Arc::new(AtomicBool::new(false)),
    );
    let state: &mut AdapterState = worker.state_for_test();
    state.load_generation = GENERATION;
    // This fixture stands in for a loaded head. Its return path must enforce
    // the same nonzero physical limit that LOAD negotiates in production.
    state.physical_capacity = 32;
    state.sessions.insert(
        SESSION.to_owned(),
        PipelineSession {
            command: SessionCommand {
                load_generation: GENERATION,
                session_id: SESSION.into(),
                stages: vec![
                    address(),
                    NodeAddress {
                        node: "n1".into(),
                        ..address()
                    },
                ],
                stage_index: usize::from(node == "n1"),
            },
            next: (node == "n0").then(|| Endpoint::node(Address::tcp("127.0.0.1", 42001), "n1", 1)),
            first: Endpoint::node(Address::tcp("127.0.0.1", 42001), "n0", 1),
            previous: (node == "n1")
                .then(|| Endpoint::node(Address::tcp("127.0.0.1", 42001), "n0", 1)),
            last: Endpoint::node(Address::tcp("127.0.0.1", 42001), "n1", 1),
        },
    );
    for request in requests {
        state.requests.insert(
            super::state::request_key(SESSION, &request.command.request_id),
            request,
        );
    }
    (worker, mailbox, snapshot)
}

/// One terminal capsule carrying `rows` prefill rows for the request.
fn prefill_capsule(rows: usize) -> Event {
    let owners: Vec<RowOwner> = (0..rows)
        .map(|index| RowOwner {
            incarnation: 1,
            load_generation: GENERATION,
            request_id: REQUEST.into(),
            sequence_key: super::state::request_key(SESSION, REQUEST),
            session_id: SESSION.into(),
            reply: "reply".into(),
            sequence_id: 0,
            phase: Phase::Prefill,
            position: index as u32,
            max_tokens: 16,
            generated_tokens: 0,
            output: false,
            input_token: 7,
            speculative_id: 0,
            speculative_index: 0,
            speculative_count: 0,
            options: String::new(),
        })
        .collect();
    let capsules = CapsuleSet(vec![PhysicalCapsule {
        execution_id: 1,
        terminal: true,
        invocation: Invocation {
            flags: 0,
            n_seq_tokens: rows as u32,
            n_seqs: 1,
            n_seqs_unq: 1,
            n_pos: 1,
            positions: (0..rows as i32).collect(),
            sequence_counts: vec![1; rows],
            sequence_ids: vec![0; rows],
            output: vec![false; rows],
        },
        owners,
        tensors: Vec::new(),
        outcomes: Vec::new(),
    }]);
    let mut event = crate::v2::tests::request_state(vec![7; 1]).template;
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "n1", 1);
    event.envelope.target = Endpoint::node(Address::tcp("127.0.0.1", 42001), "n0", 1);
    event.payload = capsules.encode().expect("a capsule set encodes");
    event
}

#[test]
fn the_worker_refuses_a_settlement_for_rows_it_never_issued() {
    // Four rows were issued; the tail returns six. The refusal has to survive
    // the whole path, not just exist in the transition.
    let mut request = crate::v2::tests::request_state(vec![7; 10]);
    request.command.session_id = SESSION.into();
    request.command.request_id = REQUEST.into();
    request.outstanding = 1;
    request.prompt_issued = 4;
    request.reply = "reply".into();
    let mut worker = worker_with(request);
    register_issue(&mut worker, &[partial_prefill(REQUEST, 0, 1, 0, 4)]);

    worker
        .tail_for_test(prefill_capsule(6))
        .expect_err("six rows back against four issued must be refused");
    // With issued identity available, a shape/membership mismatch may reject
    // before the request row-count check. The contract is refusal with no
    // progress, not the order or wording of those independent checks.

    let request = worker.request_for_test();
    assert_eq!(
        request.prompt_cursor, 0,
        "a refused settlement moves nothing"
    );
    assert_eq!(request.outstanding, 1, "and consumes no fragment");
}

#[test]
fn the_worker_refuses_a_settlement_with_nothing_in_flight() {
    let mut request = crate::v2::tests::request_state(vec![7; 10]);
    request.command.session_id = SESSION.into();
    request.command.request_id = REQUEST.into();
    request.outstanding = 0;
    request.prompt_issued = 4;
    let mut worker = worker_with(request);

    let before = bookkeeping(&mut worker);
    worker
        .tail_for_test(prefill_capsule(4))
        .expect_err("a settlement against an empty ledger must be refused");
    assert_eq!(bookkeeping(&mut worker), before);
}

#[test]
fn the_worker_accepts_the_rows_it_issued() {
    let mut request = crate::v2::tests::request_state(vec![7; 10]);
    request.command.session_id = SESSION.into();
    request.command.request_id = REQUEST.into();
    request.outstanding = 1;
    request.prompt_issued = 4;
    request.reply = "reply".into();
    let mut worker = worker_with(request);
    register_issue(&mut worker, &[partial_prefill(REQUEST, 0, 1, 0, 4)]);

    worker
        .tail_for_test(prefill_capsule(4))
        .expect("four rows back against four issued settles");

    let request = worker.request_for_test();
    assert_eq!(request.prompt_cursor, 4);
    assert_eq!(request.outstanding, 0);
}

// B1 regression fixtures deliberately record the issued capsule separately
// from the returned capsule. Editing a return must never edit its authority.
fn issued_request(name: &str, sequence_id: u32, rows: usize) -> RequestState {
    let mut request = crate::v2::tests::request_state(vec![7; 20]);
    request.command.session_id = SESSION.into();
    request.command.request_id = name.into();
    request.sequence_id = Some(sequence_id);
    request.reply = "reply".into();
    request.outstanding = 1;
    request.prompt_issued = rows;
    request
}

fn partial_prefill(
    request: &str,
    sequence: u32,
    execution: u64,
    start: u32,
    rows: usize,
) -> PhysicalCapsule {
    let mut capsule = CapsuleSet::decode(&prefill_capsule(rows).payload)
        .unwrap()
        .0
        .remove(0);
    capsule.execution_id = execution;
    for (offset, owner) in capsule.owners.iter_mut().enumerate() {
        owner.request_id = request.into();
        owner.sequence_key = super::state::request_key(SESSION, request);
        owner.sequence_id = sequence;
        owner.position = start + offset as u32;
        capsule.invocation.positions[offset] = owner.position as i32;
        capsule.invocation.sequence_ids[offset] = sequence as i32;
    }
    capsule
}

fn tail_event(id: &str, capsules: Vec<PhysicalCapsule>) -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template;
    event.envelope.source = Endpoint::node(Address::tcp("127.0.0.1", 42001), "n1", 1);
    event.envelope.target = Endpoint::node(Address::tcp("127.0.0.1", 42001), "n0", 1);
    event.envelope.event_id = id.into();
    event.envelope.payload_content_type = crate::v2::TAIL_BATCH_CONTENT_TYPE.into();
    event.payload = CapsuleSet(capsules)
        .encode()
        .expect("the wire fixture is well formed");
    event
}

fn register_issue(worker: &mut Worker, capsules: &[PhysicalCapsule]) {
    let authority = CapsuleSet(
        capsules
            .iter()
            .cloned()
            .map(|mut capsule| {
                capsule.terminal = false;
                capsule.outcomes.clear();
                capsule.tensors.clear();
                capsule
            })
            .collect(),
    );
    worker
        .state_for_test()
        .register_issued_batch(&authority)
        .expect("the independently recorded issued capsule is valid");
}

/// Capture every currently represented settlement-owned field, not just the
/// one request cursor that the original tests inspected. Reservations/credits
/// do not exist in this path yet; their gates must extend this observation.
fn bookkeeping(worker: &mut Worker) -> serde_json::Value {
    let effects = worker.effects_for_test();
    let effect_intents = worker.effect_intents_for_test();
    let state = worker.state_for_test();
    let requests: Vec<_> = state.requests.iter().map(|(key, request)| {
        serde_json::json!({
            "key": key,
            "command": request.command,
            "sequence": request.sequence_id,
            "reply": request.reply,
            "prompt_cursor": request.prompt_cursor,
            "prompt_issued": request.prompt_issued,
            "outstanding": request.outstanding,
            "generated": request.generated,
            "issued_work": request.issued_work.map(|witness| witness.proof()),
                "verify_fence_member": state.verify_fence_matches(key),
            "ready": format!("{:?}", request.ready),
            "after_settlement": match &request.after_settlement {
                None => "none".to_owned(),
                    Some(super::state::SettlementContinuation::Proposal { position, token }) => format!("proposal:{position}:{token}"),
                Some(super::state::SettlementContinuation::Replay(rows)) => format!("replay:{rows:?}"),
            },
        })
    }).collect();
    serde_json::json!({
        "requests": requests,
        "open_batches": state.open_batches,
        "flights": format!("{:?}", state.flights),
        "committed_effects": effects,
        "committed_effect_intents": effect_intents,
        "next_open_batch": state.next_open_batch,
        "pending": state.pending,
        "pending_releases": format!("{:?}", state.pending_releases),
        "free_sequences": state.free_sequences,
        "next_speculative_id": state.next_speculative_id,
        "verify_fenced": state.verify_fenced(),
    })
}

#[test]
fn t10_rejected_tail_preserves_the_open_batch_and_all_request_bookkeeping() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    register_issue(&mut worker, &[partial_prefill(REQUEST, 0, 11, 0, 4)]);
    let before = bookkeeping(&mut worker);
    let malformed = tail_event("too-many-rows", vec![partial_prefill(REQUEST, 0, 11, 0, 6)]);

    assert!(worker.tail_for_test(malformed).is_err());
    assert_eq!(
        bookkeeping(&mut worker),
        before,
        "a rejected tail cannot release its batch gate"
    );
    assert_eq!(
        mailbox.try_take(),
        Poll::Empty,
        "a rejected tail emits no token or control effect"
    );
}

#[test]
fn t11_one_bad_request_rejects_the_whole_tail_in_either_capsule_order() {
    for (reversed, invalid_sorts_first) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let (mut worker, mailbox) =
            worker_and_mailbox(vec![issued_request("a", 0, 4), issued_request("b", 1, 4)]);
        let issued = vec![
            partial_prefill("a", 0, 11, 0, 4),
            partial_prefill("b", 1, 12, 0, 4),
        ];
        register_issue(&mut worker, &issued);
        let before = bookkeeping(&mut worker);
        let mut returned = issued.clone();
        if invalid_sorts_first {
            returned[0] = partial_prefill("a", 0, 11, 0, 6);
        } else {
            returned[1] = partial_prefill("b", 1, 12, 0, 6);
        }
        if reversed {
            returned.reverse();
        }

        assert!(
            worker
                .tail_for_test(tail_event("mixed-invalid", returned))
                .is_err()
        );
        assert_eq!(
            bookkeeping(&mut worker),
            before,
            "valid members cannot settle before event validation; reversed={reversed}, invalid_sorts_first={invalid_sorts_first}"
        );
        assert_eq!(mailbox.try_take(), Poll::Empty);
    }
}

#[test]
fn t11_a_late_outcome_error_cannot_settle_any_other_request() {
    let a = issued_request("a", 0, 4);
    let mut b = issued_request("b", 1, 4);
    b.command.tokens = vec![7; 4];
    let (mut worker, mailbox) = worker_and_mailbox(vec![a, b]);
    let mut b_capsule = partial_prefill("b", 1, 12, 0, 4);
    b_capsule.owners[3].output = true;
    b_capsule.invocation.output[3] = true;
    let issued = vec![partial_prefill("a", 0, 11, 0, 4), b_capsule];
    register_issue(&mut worker, &issued);
    let before = bookkeeping(&mut worker);
    let mut returned = issued.clone();
    // The codec accepts this, but a continuing generated token with neither
    // a proposal nor a retain boundary is not an executable continuation.
    returned[1]
        .outcomes
        .push(crate::v2::capsule::PhysicalOutcome {
            owner_index: 3,
            generated: vec![crate::v2::capsule::GeneratedToken {
                token: 8,
                text: "next".into(),
                position: 4,
                stop: None,
            }],
            proposal: Vec::new(),
            retain_from: None,
            replay_tokens: Vec::new(),
            replay_position: 0,
        });
    assert!(
        worker
            .tail_for_test(tail_event("late-bad-outcome", returned))
            .is_err()
    );
    assert_eq!(
        bookkeeping(&mut worker),
        before,
        "valid A and B's generated count remain unchanged after a late decision refusal"
    );
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t12_old_execution_in_a_new_event_never_consumes_the_next_fragment() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    let first = partial_prefill(REQUEST, 0, 11, 0, 4);
    register_issue(&mut worker, std::slice::from_ref(&first));
    worker
        .tail_for_test(tail_event("first-return", vec![first.clone()]))
        .unwrap();
    {
        let request = worker
            .state_for_test()
            .requests
            .get_mut(&super::state::request_key(SESSION, REQUEST))
            .unwrap();
        request.prompt_issued = 8;
        request.outstanding = 1;
    }
    let second = partial_prefill(REQUEST, 0, 12, 4, 4);
    register_issue(&mut worker, std::slice::from_ref(&second));
    let before = bookkeeping(&mut worker);

    worker
        .tail_for_test(tail_event("same-first-new-envelope", vec![first]))
        .expect("same execution and same result is an idempotent receipt");
    assert_eq!(
        bookkeeping(&mut worker),
        before,
        "the duplicate must not consume F2"
    );
    assert_eq!(
        mailbox.try_take(),
        Poll::Empty,
        "a duplicate publishes no new logical result"
    );
    worker
        .tail_for_test(tail_event("second-return", vec![second]))
        .expect("F2 must remain settleable");
    assert_eq!(worker.request_for_test().prompt_cursor, 8);
    assert_eq!(worker.request_for_test().outstanding, 0);
    assert!(worker.state_for_test().open_batches.is_empty());
}

#[test]
fn t13_partial_prefill_refuses_a_different_sequence_without_an_outcome() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    register_issue(&mut worker, &[partial_prefill(REQUEST, 0, 11, 0, 4)]);
    let before = bookkeeping(&mut worker);
    let result = worker.tail_for_test(tail_event(
        "wrong-sequence",
        vec![partial_prefill(REQUEST, 99, 11, 0, 4)],
    ));
    assert!(result.is_err(), "sequence 99 was not admitted or issued");
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t13_partial_prefill_refuses_a_different_position_range() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    register_issue(&mut worker, &[partial_prefill(REQUEST, 0, 11, 0, 4)]);
    let before = bookkeeping(&mut worker);
    let result = worker.tail_for_test(tail_event(
        "wrong-position",
        vec![partial_prefill(REQUEST, 0, 11, 5, 4)],
    ));
    assert!(result.is_err(), "the issued [0,4) range is not [5,9)");
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t13_partial_prefill_refuses_an_unregistered_execution() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    register_issue(&mut worker, &[partial_prefill(REQUEST, 0, 11, 0, 4)]);
    let before = bookkeeping(&mut worker);
    let result = worker.tail_for_test(tail_event(
        "unknown-execution",
        vec![partial_prefill(REQUEST, 0, 99, 0, 4)],
    ));
    assert!(
        result.is_err(),
        "an active request does not authorize every execution ID"
    );
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

fn assert_partial_prefill_identity_refused(field: &str) {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    let issued = partial_prefill(REQUEST, 0, 11, 0, 4);
    register_issue(&mut worker, std::slice::from_ref(&issued));
    let before = bookkeeping(&mut worker);
    let mut returned = issued;
    for owner in &mut returned.owners {
        match field {
            // A changed key alone is no longer encodable by the strict codec.
            // Alter bytes after encoding below to exercise the real decoder.
            "key" => {}
            "generation" => owner.load_generation += 1,
            "phase" => owner.phase = Phase::Decode,
            "request" => {
                owner.request_id = "unregistered-request".into();
                owner.sequence_key =
                    super::state::request_key(&owner.session_id, &owner.request_id);
            }
            "invocation" => {}
            _ => unreachable!(),
        }
    }
    if field == "invocation" {
        returned.invocation.sequence_ids.fill(99);
    }
    let mut event = tail_event(field, vec![returned]);
    if field == "key" {
        let key = super::state::request_key(SESSION, REQUEST);
        let locations: Vec<_> = event
            .payload
            .windows(key.len())
            .enumerate()
            .filter_map(|(offset, bytes)| (bytes == key.as_bytes()).then_some(offset))
            .collect();
        assert_eq!(
            locations.len(),
            4,
            "one exact key per returned row before wire corruption"
        );
        for offset in locations {
            event.payload[offset] ^= 1;
        }
    }
    assert!(
        worker.tail_for_test(event).is_err(),
        "partial prefill must check {field} against issued authority without an outcome"
    );
    assert_eq!(
        bookkeeping(&mut worker),
        before,
        "{field} changed ledger state"
    );
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t13_partial_prefill_refuses_wrong_key_without_an_outcome() {
    assert_partial_prefill_identity_refused("key");
}

#[test]
fn t13_partial_prefill_refuses_wrong_generation_without_an_outcome() {
    assert_partial_prefill_identity_refused("generation");
}

#[test]
fn t13_partial_prefill_refuses_wrong_phase_without_an_outcome() {
    assert_partial_prefill_identity_refused("phase");
}

#[test]
fn t13_partial_prefill_refuses_unknown_request_without_an_outcome() {
    assert_partial_prefill_identity_refused("request");
}

#[test]
fn t13_partial_prefill_refuses_owner_invocation_disagreement() {
    assert_partial_prefill_identity_refused("invocation");
}

#[test]
fn t10_handle_refusal_leaves_authority_for_a_subsequent_valid_return() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    let issued = partial_prefill(REQUEST, 0, 11, 0, 4);
    register_issue(&mut worker, std::slice::from_ref(&issued));
    let before = bookkeeping(&mut worker);
    let bad = tail_event(
        "handle-too-many-rows",
        vec![partial_prefill(REQUEST, 0, 11, 0, 6)],
    );

    worker
        .handle_for_test(bad)
        .expect("an event rejection is reported without killing the worker");
    assert_eq!(
        bookkeeping(&mut worker),
        before,
        "handle must not continue from partially settled state"
    );
    let Poll::Event(error) = mailbox.try_take() else {
        panic!("handle must publish the explicit event refusal");
    };
    assert_eq!(
        error.envelope.payload_content_type,
        crate::v2::ERROR_CONTENT_TYPE
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&error.payload).unwrap()["code"],
        "LLAMA_ADAPTER_EVENT_REJECTED"
    );
    assert_eq!(
        mailbox.try_take(),
        Poll::Empty,
        "one refusal, no token or release effect"
    );

    worker
        .handle_for_test(tail_event("handle-valid-retry", vec![issued]))
        .unwrap();
    assert_eq!(worker.request_for_test().prompt_cursor, 4);
    assert_eq!(worker.request_for_test().outstanding, 0);
    assert!(worker.state_for_test().open_batches.is_empty());
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t12_same_execution_with_a_different_result_is_a_conflict() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    let first = partial_prefill(REQUEST, 0, 11, 0, 4);
    register_issue(&mut worker, std::slice::from_ref(&first));
    worker
        .tail_for_test(tail_event("first-result", vec![first.clone()]))
        .unwrap();
    let before = bookkeeping(&mut worker);
    let mut different = first;
    different.owners[0].input_token += 1;

    assert!(
        worker
            .tail_for_test(tail_event("conflicting-result", vec![different]))
            .is_err()
    );
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t13_duplicate_capsules_in_one_tail_event_are_not_two_completions() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    let issued = partial_prefill(REQUEST, 0, 11, 0, 4);
    register_issue(&mut worker, std::slice::from_ref(&issued));
    let before = bookkeeping(&mut worker);
    let event = tail_event("duplicate-capsule", vec![issued.clone(), issued]);
    assert!(worker.tail_for_test(event).is_err());
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t13_a_capsule_missing_one_issued_row_cannot_partially_settle() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    register_issue(&mut worker, &[partial_prefill(REQUEST, 0, 11, 0, 4)]);
    let before = bookkeeping(&mut worker);
    assert!(
        worker
            .tail_for_test(tail_event(
                "missing-row",
                vec![partial_prefill(REQUEST, 0, 11, 0, 3)]
            ))
            .is_err()
    );
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t14_a_split_fragment_settles_only_after_its_last_capsule_in_either_delivery_order() {
    for reversed in [false, true] {
        let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 8)]);
        let issued = vec![
            partial_prefill(REQUEST, 0, 11, 0, 4),
            partial_prefill(REQUEST, 0, 12, 4, 4),
        ];
        register_issue(&mut worker, &issued);
        let (first, last) = if reversed { (1, 0) } else { (0, 1) };

        worker
            .tail_for_test(tail_event(
                "one-physical-return",
                vec![issued[first].clone()],
            ))
            .unwrap();
        assert_eq!(
            worker.request_for_test().prompt_cursor,
            0,
            "receipt buffering is not logical fragment settlement; reversed={reversed}"
        );
        assert_eq!(worker.request_for_test().outstanding, 1);
        assert_eq!(worker.state_for_test().open_batches.len(), 1);
        assert!(worker.state_for_test().free_sequences.is_empty());
        assert_eq!(mailbox.try_take(), Poll::Empty);

        let after_first = bookkeeping(&mut worker);
        worker
            .tail_for_test(tail_event(
                "same-physical-return-new-envelope",
                vec![issued[first].clone()],
            ))
            .unwrap();
        assert_eq!(
            bookkeeping(&mut worker),
            after_first,
            "a duplicate cannot unlock the last capsule"
        );

        worker
            .tail_for_test(tail_event(
                "last-physical-return",
                vec![issued[last].clone()],
            ))
            .unwrap();
        assert_eq!(worker.request_for_test().prompt_cursor, 8);
        assert_eq!(worker.request_for_test().outstanding, 0);
        assert!(worker.state_for_test().open_batches.is_empty());
        assert!(
            worker.state_for_test().free_sequences.is_empty(),
            "a partial prompt still owns its sequence"
        );
        assert_eq!(mailbox.try_take(), Poll::Empty);
    }
}

#[test]
fn t14_later_logical_prefill_waits_for_the_earlier_range_before_settling() {
    let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
    let earlier = partial_prefill(REQUEST, 0, 11, 0, 4);
    register_issue(&mut worker, std::slice::from_ref(&earlier));
    {
        let request = worker
            .state_for_test()
            .requests
            .get_mut(&super::state::request_key(SESSION, REQUEST))
            .unwrap();
        request.prompt_issued = 8;
        request.outstanding = 2;
    }
    let later = partial_prefill(REQUEST, 0, 12, 4, 4);
    register_issue(&mut worker, std::slice::from_ref(&later));

    worker
        .tail_for_test(tail_event("later-returned-first", vec![later.clone()]))
        .unwrap();
    assert_eq!(
        worker.request_for_test().prompt_cursor,
        0,
        "a delivered suffix is not a settled prefix"
    );
    assert_eq!(worker.request_for_test().outstanding, 2);
    assert_eq!(worker.state_for_test().open_batches.len(), 2);
    let buffered = bookkeeping(&mut worker);
    worker
        .tail_for_test(tail_event("later-duplicate", vec![later]))
        .unwrap();
    assert_eq!(bookkeeping(&mut worker), buffered);

    worker
        .tail_for_test(tail_event("earlier-returned-last", vec![earlier]))
        .unwrap();
    assert_eq!(worker.request_for_test().prompt_cursor, 8);
    assert_eq!(worker.request_for_test().outstanding, 0);
    assert!(worker.state_for_test().open_batches.is_empty());
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

/// The native engine is not part of this test. Its well-formed output enters
/// the actual tail publisher, traverses a completion mailbox as TAIL_BATCH,
/// and is accepted by the actual head event handler before OUTER can see it.
#[test]
fn t12_tail_publishes_only_to_the_head_then_head_publishes_the_token_once() {
    let mut request = issued_request(REQUEST, 0, 4);
    request.command.tokens = vec![7; 4];
    request.reply = serde_json::to_string(&crate::v2::ReplySpec {
        ingress_agent: Address::tcp("127.0.0.1", 42001).to_string(),
        channel: "wave-output".into(),
        connection_generation: 7,
        correlation_id: "wave-request".into(),
        deadline_unix_ms: None,
    })
    .unwrap();
    let mut issued = partial_prefill(REQUEST, 0, 11, 0, 4);
    for owner in &mut issued.owners {
        owner.reply.clone_from(&request.reply);
    }
    issued.owners[3].output = true;
    issued.invocation.output[3] = true;
    let (mut head, head_mailbox) = worker_and_mailbox(vec![request]);
    register_issue(&mut head, std::slice::from_ref(&issued));
    let before_head = bookkeeping(&mut head);

    let (mut tail, tail_mailbox) = worker_and_mailbox_at(Vec::new(), "n1");
    let mut tail_session = tail.state_for_test().sessions[SESSION].clone();
    tail_session.command.stage_index = 1;
    tail_session.first = Endpoint::node(Address::tcp("127.0.0.1", 42001), "n0", 1);
    let mut completed = issued;
    completed.outcomes.push(crate::v2::PhysicalOutcome {
        owner_index: 3,
        generated: vec![crate::v2::GeneratedToken {
            token: 8,
            text: "answer".into(),
            position: 4,
            stop: None,
        }],
        proposal: vec![8],
        retain_from: None,
        replay_tokens: Vec::new(),
        replay_position: 0,
    });
    let capsules = CapsuleSet(vec![completed]);
    let body = capsules.encode().unwrap();
    let base = tail_event("native-terminal", capsules.0.clone());
    tail.emit_tail_results_for_test(&base, &tail_session, capsules.clone(), body)
        .unwrap();

    let Poll::Event(for_head) = tail_mailbox.try_take() else {
        panic!("tail must publish its terminal capsule");
    };
    assert_eq!(
        for_head.envelope.payload_content_type,
        crate::v2::TAIL_BATCH_CONTENT_TYPE
    );
    assert_eq!(for_head.envelope.target, tail_session.first);
    assert_eq!(CapsuleSet::decode(&for_head.payload).unwrap(), capsules);
    assert_eq!(
        tail_mailbox.try_take(),
        Poll::Empty,
        "the tail cannot publish a token before head approval"
    );
    assert_eq!(head_mailbox.try_take(), Poll::Empty);
    assert_eq!(
        bookkeeping(&mut head),
        before_head,
        "tail publication itself grants no head progress"
    );

    head.handle_for_test(for_head.clone()).unwrap();
    let Poll::Event(output) = head_mailbox.try_take() else {
        panic!("head approval publishes the token");
    };
    assert_eq!(
        output.envelope.payload_content_type,
        crate::v2::OUTPUT_CONTENT_TYPE
    );
    assert_eq!(output.envelope.correlation_id, "wave-request");
    assert_eq!(
        output.envelope.target,
        Endpoint::outer(Address::tcp("127.0.0.1", 42001), "wave-output", 7)
    );
    let payload: serde_json::Value = serde_json::from_slice(&output.payload).unwrap();
    assert_eq!(payload["request_id"], REQUEST);
    assert_eq!(payload["token"], 8);
    assert_eq!(payload["position"], 4);
    assert_eq!(payload["text"], "answer");
    assert_eq!(head_mailbox.try_take(), Poll::Empty);
    assert_eq!(head.request_for_test().generated, 1);
    assert_eq!(head.request_for_test().prompt_cursor, 4);
    assert_eq!(head.effects_for_test(), (0, false));

    let after_approval = bookkeeping(&mut head);
    let mut duplicate = for_head;
    duplicate.envelope.event_id = "transport-redelivered-with-new-id".into();
    head.handle_for_test(duplicate).unwrap();
    assert_eq!(bookkeeping(&mut head), after_approval);
    assert_eq!(
        head_mailbox.try_take(),
        Poll::Empty,
        "the same receipt cannot emit the token twice"
    );
}

fn output_ready_fragments(count: usize) -> (Vec<RequestState>, Vec<PhysicalCapsule>) {
    let mut requests = Vec::new();
    let mut capsules = Vec::new();
    for index in 0..count {
        let name = format!("output-{index}");
        let mut request = issued_request(&name, index as u32, 4);
        request.command.tokens = vec![7; 4];
        request.reply = serde_json::to_string(&crate::v2::ReplySpec {
            ingress_agent: Address::tcp("127.0.0.1", 42001).to_string(),
            channel: "wave-output".into(),
            connection_generation: 7,
            correlation_id: name.clone(),
            deadline_unix_ms: None,
        })
        .unwrap();
        let mut capsule = partial_prefill(&name, index as u32, 11 + index as u64, 0, 4);
        for owner in &mut capsule.owners {
            owner.reply.clone_from(&request.reply);
        }
        capsule.owners[3].output = true;
        capsule.invocation.output[3] = true;
        capsule.outcomes.push(crate::v2::PhysicalOutcome {
            owner_index: 3,
            generated: vec![crate::v2::GeneratedToken {
                token: 8,
                text: name,
                position: 4,
                stop: None,
            }],
            proposal: vec![8],
            retain_from: None,
            replay_tokens: Vec::new(),
            replay_position: 0,
        });
        requests.push(request);
        capsules.push(capsule);
    }
    (requests, capsules)
}

#[test]
fn terminal_return_cannot_mint_a_witness_from_registered_capsules() {
    let (mut requests, mut capsules) = output_ready_fragments(1);
    requests[0].command.max_tokens = 1;
    for owner in &mut capsules[0].owners {
        owner.max_tokens = 1;
    }
    capsules[0].outcomes[0].generated[0].stop = Some("length".into());
    capsules[0].outcomes[0].proposal.clear();
    let (mut worker, mailbox) = worker_and_mailbox(requests);
    // Deliberate raw registration: no native issue acceptance occurred. A
    // return must not synthesize the missing authority from its own payload.
    register_issue(&mut worker, &capsules);
    let before = bookkeeping(&mut worker);
    assert_eq!(
        worker
            .tail_for_test(tail_event("terminal-without-accepted-work", capsules))
            .unwrap_err(),
        "terminal request has no accepted issued-work witness"
    );
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);
    assert!(worker.request_for_test().issued_work.is_none());
}

fn assert_outputs_committed_once(worker: &mut Worker, requests: usize) {
    let state = worker.state_for_test();
    assert!(state.open_batches.is_empty());
    assert_eq!(state.requests.len(), requests);
    for request in state.requests.values() {
        assert_eq!(
            (
                request.prompt_cursor,
                request.generated,
                request.outstanding
            ),
            (4, 1, 0)
        );
        assert_eq!(request.ready.as_ref().unwrap().position, 4);
    }
}

#[test]
fn t24_head_rejects_overwide_proposal_before_any_return_or_output_commit() {
    let (requests, mut capsules) = output_ready_fragments(2);
    let (mut worker, mailbox) = worker_and_mailbox(requests);
    worker.state_for_test().physical_capacity = 2;
    register_issue(&mut worker, &capsules);
    let before = bookkeeping(&mut worker);
    capsules[1].outcomes[0].proposal = vec![8, 9, 10];
    // Well-formed and within the request budget: the local physical limit is
    // the rejecting contract, not the codec or a different token constraint.
    let error = worker
        .tail_for_test(tail_event("overwide-second-decision", capsules.clone()))
        .unwrap_err();
    assert!(error.contains("physical capacity"), "{error}");
    assert_eq!(bookkeeping(&mut worker), before);
    assert_eq!(mailbox.try_take(), Poll::Empty);

    // The good prefix was not consumed. The same issued identities can now
    // return width one and exactly the capacity, with each token emitted once.
    capsules[1].outcomes[0].proposal.pop();
    worker
        .tail_for_test(tail_event("legal-widths", capsules))
        .unwrap();
    assert_outputs_committed_once(&mut worker, 2);
    for _ in 0..2 {
        let Poll::Event(output) = mailbox.try_take() else {
            panic!("approved request must publish one output");
        };
        assert_eq!(
            output.envelope.payload_content_type,
            crate::v2::OUTPUT_CONTENT_TYPE
        );
    }
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn t23_a_closed_first_output_keeps_the_committed_intent_and_fences_redelivery() {
    let (requests, capsules) = output_ready_fragments(1);
    let (mut worker, mailbox) = worker_and_mailbox(requests);
    register_issue(&mut worker, &capsules);
    drop(mailbox);

    assert!(
        worker
            .tail_for_test(tail_event("closed-output", capsules.clone()))
            .is_err()
    );
    assert_outputs_committed_once(&mut worker, 1);
    assert_eq!(
        worker.effects_for_test(),
        (1, true),
        "Closed retains the unacknowledged intent"
    );
    let after_failure = bookkeeping(&mut worker);
    assert!(
        worker
            .tail_for_test(tail_event("redelivery-after-closed", capsules))
            .is_err()
    );
    assert_eq!(
        bookkeeping(&mut worker),
        after_failure,
        "redelivery cannot settle twice or drop the intent"
    );
}

#[test]
fn t23_output_event_id_exhaustion_keeps_the_intent_without_publishing_or_resettling() {
    let (requests, capsules) = output_ready_fragments(1);
    let (mut worker, mailbox) = worker_and_mailbox(requests);
    register_issue(&mut worker, &capsules);
    worker
        .tail_commit_for_test(tail_event("id-exhausted", capsules.clone()))
        .unwrap();
    // Delivery fault after commit, not a shortage before the transaction.
    worker.state_for_test().next_event = u64::MAX;
    assert!(worker.flush_for_test().is_err());
    assert_outputs_committed_once(&mut worker, 1);
    assert_eq!(worker.effects_for_test(), (1, true));
    assert_eq!(worker.state_for_test().next_event, u64::MAX);
    assert_eq!(
        mailbox.try_take(),
        Poll::Empty,
        "an exhausted event ID cannot escape to the mailbox"
    );
    let after_failure = bookkeeping(&mut worker);
    assert!(
        worker
            .tail_for_test(tail_event("redelivery-after-exhaustion", capsules))
            .is_err()
    );
    assert_eq!(bookkeeping(&mut worker), after_failure);
    assert_eq!(mailbox.try_take(), Poll::Empty);
}

#[test]
fn output_id_obligations_refuse_whole_return_before_commit_and_accept_exact_room() {
    for (next, accepted) in [(u64::MAX - 1, false), (u64::MAX - 2, true)] {
        let (requests, capsules) = output_ready_fragments(2);
        let (mut worker, mailbox) = worker_and_mailbox(requests);
        register_issue(&mut worker, &capsules);
        worker.state_for_test().next_event = next;
        let before = bookkeeping(&mut worker);
        let result = worker.tail_commit_for_test(tail_event("id-preflight", capsules));
        if accepted {
            result.unwrap();
            assert_outputs_committed_once(&mut worker, 2);
            assert_eq!(worker.effects_for_test(), (2, false));
            assert_eq!(
                worker.state_for_test().next_event,
                next,
                "reserve count, not sequence numbers"
            );
            worker.flush_for_test().unwrap();
            for id in [next, next + 1] {
                let Poll::Event(event) = mailbox.try_take() else {
                    panic!("committed output missing")
                };
                assert_eq!(event.envelope.sequence, id);
            }
        } else {
            assert!(result.unwrap_err().contains("event ID is exhausted"));
            assert_eq!(bookkeeping(&mut worker), before);
            assert_eq!(worker.effects_for_test(), (0, false));
        }
        assert_eq!(mailbox.try_take(), Poll::Empty);
    }
}

#[test]
fn t23_disconnect_after_one_accepted_output_preserves_remaining_intents_and_joins() {
    let (requests, capsules) = output_ready_fragments(3);
    let (mut worker, mailbox, status) = worker_with_status(requests, "n0", 1);
    register_issue(&mut worker, &capsules);
    let event = tail_event("three-output-intents", capsules.clone());
    let (finished, completion) = std::sync::mpsc::channel();
    let join = std::thread::spawn(move || {
        let result = worker.tail_for_test(event);
        finished.send((worker, result)).unwrap();
    });

    // No other producer exists. A full capacity-one mailbox means output one
    // was accepted and output two is waiting. Do not take then close: taking
    // releases capacity and races the second publication. Consumer disconnect
    // deliberately abandons its unread first output; this is not a delivery ACK.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut saw_full = false;
    while std::time::Instant::now() < deadline {
        if status.lock().unwrap().as_str() == "completion_queue_full:waiting" {
            saw_full = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    drop(mailbox);
    let (mut worker, result) = completion
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("closing the mailbox releases the bounded publication wait");
    join.join().unwrap();
    assert!(
        saw_full,
        "the test must actually reach publication one then Full on publication two"
    );
    assert!(result.is_err());
    assert_outputs_committed_once(&mut worker, 3);
    assert_eq!(
        worker.effects_for_test(),
        (2, true),
        "only the accepted first intent is retired"
    );
    let after_failure = bookkeeping(&mut worker);
    assert!(
        worker
            .tail_for_test(tail_event("redelivery-after-partial-publish", capsules))
            .is_err()
    );
    assert_eq!(bookkeeping(&mut worker), after_failure);
}

#[test]
fn t19_request_counters_cannot_override_issued_identity_and_repair_keeps_the_valid_return() {
    for corruption in ["too-high", "too-low", "ghost-request"] {
        let (mut worker, mailbox) = worker_and_mailbox(vec![issued_request(REQUEST, 0, 4)]);
        let issued = partial_prefill(REQUEST, 0, 11, 0, 4);
        register_issue(&mut worker, std::slice::from_ref(&issued));
        let key = super::state::request_key(SESSION, REQUEST);
        {
            let state = worker.state_for_test();
            match corruption {
                "too-high" => state.requests.get_mut(&key).unwrap().outstanding = 2,
                "too-low" => state.requests.get_mut(&key).unwrap().outstanding = 0,
                "ghost-request" => {
                    state.requests.insert(
                        super::state::request_key(SESSION, "ghost"),
                        issued_request("ghost", 1, 4),
                    );
                }
                _ => unreachable!(),
            }
        }
        let before = bookkeeping(&mut worker);
        assert!(
            worker
                .tail_for_test(tail_event(corruption, vec![issued.clone()]))
                .is_err(),
            "{corruption} must disagree with independently registered execution membership"
        );
        assert_eq!(
            bookkeeping(&mut worker),
            before,
            "refusing {corruption} cannot consume the genuine return"
        );
        assert_eq!(mailbox.try_take(), Poll::Empty);

        // Repair only the injected counter/ghost. Do not rebuild, retire or
        // edit issued authority to make the comparison agree with corruption.
        worker
            .state_for_test()
            .requests
            .get_mut(&key)
            .unwrap()
            .outstanding = 1;
        worker
            .state_for_test()
            .requests
            .remove(&super::state::request_key(SESSION, "ghost"));
        worker
            .tail_for_test(tail_event("valid-after-counter-repair", vec![issued]))
            .unwrap();
        assert_eq!(
            (
                worker.request_for_test().prompt_cursor,
                worker.request_for_test().outstanding
            ),
            (4, 0)
        );
        assert!(worker.state_for_test().open_batches.is_empty());
        assert_eq!(mailbox.try_take(), Poll::Empty);
    }
}
