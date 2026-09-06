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
use crate::v2::commands::{NodeAddress, NodeRole, SessionCommand};
use crate::v2::scheduler::Phase;
use p4_adapter::node_adapter::completion_mailbox;
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
    let own = Address::tcp("127.0.0.1", 42001);
    let endpoint = Endpoint::node(own, "n0", 1);
    let (_input, receiver) = std::sync::mpsc::channel();
    std::mem::forget(_input);
    let (publisher, _mailbox) = completion_mailbox(8);
    let mut worker = Worker::new(
        endpoint.clone(),
        receiver,
        publisher,
        Arc::new(Mutex::new(String::new())),
        Arc::new(AtomicBool::new(false)),
    );
    let state: &mut AdapterState = worker.state_for_test();
    state.load_generation = GENERATION;
    state.sessions.insert(
        SESSION.to_owned(),
        PipelineSession {
            command: SessionCommand {
                load_generation: GENERATION,
                session_id: SESSION.into(),
                role: NodeRole::First,
                next: None,
                first: address(),
            },
            next: None,
            first: endpoint,
        },
    );
    state
        .requests
        .insert(super::state::request_key(SESSION, REQUEST), request);
    worker
}

/// One terminal capsule carrying `rows` prefill rows for the request.
fn prefill_capsule(rows: usize) -> Event {
    let owners: Vec<RowOwner> = (0..rows)
        .map(|index| RowOwner {
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
    let mut worker = worker_with(request);

    let error = worker
        .tail_for_test(prefill_capsule(6))
        .expect_err("six rows back against four issued must be refused");
    assert_eq!(error, "tail completed more prompt rows than were issued");

    let request = worker.request_for_test();
    assert_eq!(request.prompt_cursor, 0, "a refused settlement moves nothing");
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

    let error = worker
        .tail_for_test(prefill_capsule(4))
        .expect_err("a settlement against an empty ledger must be refused");
    assert_eq!(
        error,
        "tail completed a request with no fragment in flight",
    );
}

#[test]
fn the_worker_accepts_the_rows_it_issued() {
    let mut request = crate::v2::tests::request_state(vec![7; 10]);
    request.command.session_id = SESSION.into();
    request.command.request_id = REQUEST.into();
    request.outstanding = 1;
    request.prompt_issued = 4;
    let mut worker = worker_with(request);

    worker
        .tail_for_test(prefill_capsule(4))
        .expect("four rows back against four issued settles");

    let request = worker.request_for_test();
    assert_eq!(request.prompt_cursor, 4);
    assert_eq!(request.outstanding, 0);
}
