//! L1 failure atomicity of the production incremental evidence ledger.
//! This is not another aggregation implementation. Actual worker-capture tests
//! independently exercise hashing, transport and output.
use super::*;
use crate::run::Sender;
use p4_llamacpp_staged_adapter::v2::{
    BatchRequestObservation, IssuedRow, OutcomePayload, Phase, PhysicalBatchObservation,
};
use p4_protocol::Address;

fn request(id: &str) -> (Event, InferenceCommand) {
    let outer = OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 49990),
        channel: "atomic".into(),
        connection_generation: 7,
    };
    let command = InferenceCommand {
        load_generation: 1,
        session_id: "s".into(),
        request_id: id.into(),
        prompt: Some("Normal prompt".into()),
        tokens: Vec::new(),
        options: String::new(),
        session_key: None,
        max_tokens: 1,
    };
    let mut sender = Sender::new(outer);
    sender.sequence = if id == "a" { 1 } else { 2 };
    let event = sender.event(
        Endpoint::node(Address::tcp("127.0.0.1", 49991), "head", 1),
        EventClass::Data,
        p4_llamacpp_staged_adapter::v2::PREFILL_CONTENT_TYPE,
        serde_json::to_vec(&command).unwrap(),
        id,
    );
    (event, command)
}

fn carrier(input: &Event) -> Event {
    let mut result = input.clone();
    result.envelope.source = input.envelope.target.clone();
    result.envelope.target = input.envelope.source.clone();
    result.envelope.causation_id = Some(input.envelope.event_id.clone());
    result
}

fn observed(input: &Event, id: &str, slot: u32, rows: usize) -> BatchRequestObservation {
    BatchRequestObservation {
        request_id: id.into(),
        submission_event_id: input.envelope.event_id.clone(),
        sequence_id: slot,
        incarnation: 1,
        request_issue_index: 1,
        rows: (0..rows)
            .map(|position| IssuedRow {
                phase: Phase::Prefill,
                position: position as u32,
            })
            .collect(),
        prefill_rows: rows,
        decode_rows: 0,
        verify_rows: 0,
        replay_rows: 0,
    }
}

fn observation(owners: Vec<BatchRequestObservation>) -> BatchObservation {
    let rows = owners.iter().map(|owner| owner.prefill_rows).sum();
    BatchObservation {
        scheduling: None,
        observation_id: "logical-1".into(),
        load_generation: 1,
        session_id: "s".into(),
        logical_ordinal: 1,
        logical_rows: rows,
        physical_batches: vec![PhysicalBatchObservation {
            execution_id: 1,
            rows,
            prefill_rows: rows,
            decode_rows: 0,
            verify_rows: 0,
            replay_rows: 0,
            request_count: owners.len(),
            sequence_count: owners.len(),
            owned_requests: owners,
        }],
        mixed_physical_batches: 0,
        stage_ms: 0,
        idle_ms: 0,
        idle_gated: 0,
        ready_rows: rows,
        ready_sequences: 2,
    }
}

#[test]
fn a_later_bad_boundary_cannot_install_the_first_requests_counts() {
    let mut ledger = EvidenceLedger::new(2);
    let (a, ca) = request("a");
    let (b, cb) = request("b");
    for (event, command) in [(&a, &ca), (&b, &cb)] {
        ledger.register(SubmittedAuthority::from_event(event, command).unwrap());
    }
    let owners = vec![observed(&a, "a", 0, 4), observed(&b, "b", 1, 7)];
    for (input, owner, first) in [(&a, &owners[0], 4), (&b, &owners[1], 8)] {
        let authority = ledger.requests[&owner.request_id]
            .authority
            .issue(owner.sequence_id, 1);
        // Oracle here is all-or-none state, not the hash algorithm.
        let proof = IssueWitness::new(&authority)
            .unwrap()
            .advanced(
                &authority,
                &IssuedWork {
                    logical_ordinal: 1,
                    executions: vec![IssuedExecution {
                        execution_id: 1,
                        rows: owner.rows.clone(),
                    }],
                },
            )
            .unwrap()
            .proof();
        let output = ApprovedOutputPayload {
            outcome: OutcomePayload {
                load_generation: 1,
                session_id: "s".into(),
                request_id: owner.request_id.clone(),
                sequence_id: owner.sequence_id,
                token: 42,
                text: "normal".into(),
                position: first,
                stop: Some("length".into()),
            },
            submission_event_id: input.envelope.event_id.clone(),
            incarnation: 1,
            release_operation_id: Some(1),
            issued_work: Some(proof),
        };
        ledger.output(&carrier(input), &output).unwrap();
    }
    let before = format!("{ledger:?}");
    assert!(
        ledger
            .observation(&carrier(&a), observation(owners.clone()))
            .unwrap_err()
            .contains("first output")
    );
    assert_eq!(format!("{ledger:?}"), before);
    ledger
        .requests
        .get_mut("b")
        .unwrap()
        .progress
        .first_position = Some(7);
    ledger
        .observation(&carrier(&a), observation(owners))
        .unwrap();
    assert_eq!(ledger.requests["a"].progress.rows[0], 4);
    assert_eq!(ledger.requests["b"].progress.rows[0], 7);
    assert_eq!(
        ledger.status(),
        EvidenceStatus::Missing {
            requests: 0,
            stage_executions: 2
        }
    );
}

#[test]
fn row_overflow_is_an_error_without_partial_counter_installation() {
    let mut ledger = EvidenceLedger::new(2);
    let (event, command) = request("a");
    ledger.register(SubmittedAuthority::from_event(&event, &command).unwrap());
    // Test-only impossible predecessor: production exposes no such mutator.
    ledger.requests.get_mut("a").unwrap().progress.rows[0] = usize::MAX;
    let before = format!("{ledger:?}");
    assert_eq!(
        ledger
            .observation(
                &carrier(&event),
                observation(vec![observed(&event, "a", 0, 1)])
            )
            .unwrap_err(),
        "request row count overflow"
    );
    assert_eq!(format!("{ledger:?}"), before);
}
