//! Real OUTER EventWire -> inference::drive -> acceptance regressions.
//!
//! The peer validates actual submitted PREFILL commands, then supplies scripted
//! head-approved envelopes. It is not a native/model/GPU implementation. Prompt
//! rows [4, 7], token counts and response text are independent fixture facts.
//! Release members are bound to the actual submitted event and independently
//! assigned scripted owner/operation, not learned by the consumer from receipts.
//! Native KV deletion and real transport remain outside this bounded fixture.
use super::*;
use p4_llamacpp_staged_adapter::v2::{
    ApprovedOutputPayload, BATCH_OBSERVATION_CONTENT_TYPE, BatchRequestObservation,
    InferenceCommand, IssueAuthority, IssueWitness, IssuedExecution, IssuedRow, IssuedWork,
    OUTPUT_CONTENT_TYPE, PREFILL_CONTENT_TYPE, Phase, PhysicalBatchObservation,
    RELEASE_RECEIPT_CONTENT_TYPE, ReleaseMember, ReleaseReceipt, STAGE_SPAN_CONTENT_TYPE,
    StageExecutionObservation, StageRequestObservation, StageSpan,
};
use std::time::Instant;
use tokio::io::duplex;

#[derive(Clone, Copy, Debug)]
enum Case {
    Normal,
    ExceedCap,
    ShiftBoundary,
    MissingObservation,
    ExactObservationReplay,
    ReusedExecution,
    ConflictingObservation,
    ReorderedObservations,
    LateObservations,
    EmptyEos,
    EarlyStop,
    EarlyEos,
    PrematureLength,
    UnknownStop,
    OutputAfterStop,
    DuplicateAOnly,
    ExactMemberReplay,
    GroupedReceipt,
    ReversedGroupedReceipt,
    MixedReplayFresh,
    MalformedLaterMember,
    DuplicateMember,
    ReceiptBeforeTerminal,
    StaleReceiptSubmission,
    StaleReceiptIncarnation,
    StaleReceiptOperation,
    WrongReceiptSlot,
    ForeignReceiptRequest,
    StaleReceiptLoad,
    StaleReceiptSession,
    StaleOutputSubmission,
    ChangedOutputIncarnation,
    MissingTerminalRelease,
    UnexpectedNonterminalRelease,
    ScalarRelease,
    LegacyOutput,
    ForeignKnownCorrelation,
    DuplicateReceiptEnvelope,
    ChangedMemberReplay,
    ForeignObservationCarrier,
    ForeignSpanCarrier,
}

impl Case {
    fn single(self) -> bool {
        matches!(
            self,
            Self::ExceedCap
                | Self::EmptyEos
                | Self::EarlyStop
                | Self::EarlyEos
                | Self::PrematureLength
                | Self::UnknownStop
                | Self::OutputAfterStop
        )
    }

    fn max_tokens(self) -> u32 {
        match self {
            Self::ExceedCap | Self::EmptyEos => 1,
            Self::EarlyStop
            | Self::EarlyEos
            | Self::PrematureLength
            | Self::UnknownStop
            | Self::OutputAfterStop => 4,
            _ => 2,
        }
    }
}

fn config(case: Case) -> RunConfig {
    let count = if case.single() { 1 } else { 2 };
    let node = |name: &str| config::NodeConfig {
        agent: "tcp://127.0.0.1:52501".into(),
        node: name.into(),
        generation: 3,
        binary: "not-started".into(),
        endpoint: "tcp://127.0.0.1:52502".into(),
        plan: "not-loaded".into(),
        args: vec![],
        environment: vec![],
        n_batch: 32,
        n_ubatch: 32,
        context_size: 128,
        total_context_size: 256,
        sequence_capacity: 2,
    };
    RunConfig {
        ingress_agent: "tcp://127.0.0.1:52501".into(),
        channel: "budget-boundary-test".into(),
        connection_generation: 7,
        load_generation: 9,
        session_id: "session".into(),
        request_id: "request".into(),
        nodes: vec![node("head"), node("tail")],
        prompt: String::new(),
        prompts: if count == 1 {
            vec!["Explain Rust.".into()]
        } else {
            vec![
                "Explain Rust.".into(),
                "Explain Rust ownership and borrowing.".into(),
            ]
        },
        session_key_template: String::new(),
        max_tokens: case.max_tokens(),
        waves: vec![ArrivalWave { after_ms: 0, count }],
        options: String::new(),
        pre_inference_hold_ms: 0,
        timeout_ms: 2_000,
        acceptance: AcceptanceConfig {
            minimum_generated_tokens: 1,
            expected_prefill_rows: None,
            // Protocol stop vocabulary is mandatory even without an optional
            // scenario-specific stop allowlist. UnknownStop exercises that.
            allowed_stop_reasons: Vec::new(),
            responses: (0..count)
                .map(|_| ResponseExpectation {
                    // RunConfig correctly forbids an exact empty-response
                    // expectation. EmptyEos instead tests that the unchanged
                    // useful-response gate rejects the empty result.
                    exact_response: (!matches!(case, Case::EmptyEos))
                        .then(|| "Normal response.".into()),
                    ..Default::default()
                })
                .collect(),
        },
    }
}

fn response(
    request: &Event,
    source: &Endpoint,
    serial: u64,
    class: EventClass,
    content_type: &str,
    payload: Vec<u8>,
) -> Event {
    let outer = request.envelope.return_route.clone().unwrap();
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: format!("fresh-head-{serial}"),
            correlation_id: request.envelope.correlation_id.clone(),
            causation_id: Some(request.envelope.event_id.clone()),
            source: source.clone(),
            target: Endpoint::Outer(outer.clone()),
            return_route: Some(outer),
            class,
            sequence: serial,
            deadline_unix_ms: None,
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: content_type.into(),
        },
        payload,
    }
}

fn observation(index: usize, event: &Event, request_id: &str, step: usize) -> BatchObservation {
    let prompt_rows = if index == 0 { 4 } else { 7 };
    let rows = if step == 0 { prompt_rows } else { 1 };
    let prefill = if step == 0 { prompt_rows } else { 0 };
    let decode = usize::from(step != 0);
    let execution = index as u64 * 10 + step as u64 + 1;
    BatchObservation {
        observation_id: format!("obs-{index}-{step}"),
        logical_ordinal: execution,
        load_generation: 9,
        session_id: "session".into(),
        logical_rows: rows,
        physical_batches: vec![PhysicalBatchObservation {
            execution_id: execution,
            rows,
            prefill_rows: prefill,
            decode_rows: decode,
            verify_rows: 0,
            replay_rows: 0,
            request_count: 1,
            sequence_count: 1,
            owned_requests: vec![BatchRequestObservation {
                request_id: request_id.into(),
                submission_event_id: event.envelope.event_id.clone(),
                sequence_id: index as u32,
                incarnation: 20 + index as u64,
                request_issue_index: step as u64 + 1,
                rows: (0..rows)
                    .map(|offset| IssuedRow {
                        phase: if step == 0 {
                            Phase::Prefill
                        } else {
                            Phase::Decode
                        },
                        position: if step == 0 {
                            offset as u32
                        } else {
                            (prompt_rows + step - 1) as u32
                        },
                    })
                    .collect(),
                prefill_rows: prefill,
                decode_rows: decode,
                verify_rows: 0,
                replay_rows: 0,
            }],
        }],
        mixed_physical_batches: 0,
        stage_ms: 1,
        idle_ms: 0,
        idle_gated: 0,
        ready_rows: rows,
        ready_sequences: 1,
    }
}

fn tokens(case: Case) -> Vec<(&'static str, Option<&'static str>)> {
    match case {
        Case::EmptyEos => vec![("", Some("eos"))],
        Case::EarlyStop => vec![("Normal response.", Some("stop"))],
        Case::EarlyEos => vec![("Normal response.", None), ("", Some("eos"))],
        Case::PrematureLength => vec![("Normal response.", Some("length"))],
        Case::UnknownStop => vec![("Normal response.", Some("completed"))],
        Case::OutputAfterStop => vec![("Normal response.", Some("stop")), ("extra", None)],
        _ => vec![("Normal ", None), ("response.", Some("length"))],
    }
}

struct Approved {
    run: inference::InferenceResult,
    acceptance: acceptance::AcceptanceSummary,
}

async fn exercise(case: Case) -> Result<Approved, String> {
    let config = config(case);
    config::validate(&config).unwrap();
    let count = config.waves[0].count;
    let head = node_endpoint(&config.nodes[0]).unwrap();
    let (client, peer) = duplex(128 * 1024);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = wire::EventWire::new(reader, writer);
    let outer = OuterEndpoint {
        ingress_agent: Address::from_str(&config.ingress_agent).unwrap(),
        channel: config.channel.clone(),
        connection_generation: config.connection_generation,
    };
    let mut sender = Sender::new(outer);
    let peer_task = tokio::spawn(async move {
        let (reader, writer) = tokio::io::split(peer);
        let mut wire = wire::EventWire::new(reader, writer);
        let mut submitted = Vec::new();
        for _ in 0..count {
            let event = wire
                .receive(Instant::now() + Duration::from_secs(2))
                .await
                .unwrap();
            assert_eq!(event.envelope.payload_content_type, PREFILL_CONTENT_TYPE);
            let command: InferenceCommand = serde_json::from_slice(&event.payload).unwrap();
            command.validate().unwrap();
            assert_eq!(event.envelope.correlation_id, command.request_id);
            assert_eq!(command.max_tokens, case.max_tokens());
            submitted.push((event, command));
        }
        let mut serial = 1;
        let mut observations = Vec::new();
        let mut all_observations = Vec::new();
        let mut outputs = Vec::new();
        let mut release_members = Vec::new();
        for (index, (base, command)) in submitted.iter().enumerate() {
            release_members.push(ReleaseMember {
                request_id: command.request_id.clone(),
                submission_event_id: base.envelope.event_id.clone(),
                sequence_id: index as u32,
                incarnation: 20 + index as u64,
                operation_id: 30 + index as u64,
            });
            let authority = IssueAuthority {
                head: base.envelope.target.clone(),
                outer: base.envelope.return_route.clone().unwrap(),
                load_generation: 9,
                session_id: "session".into(),
                request_id: command.request_id.clone(),
                submission_event_id: base.envelope.event_id.clone(),
                sequence_id: index as u32,
                incarnation: 20 + index as u64,
            };
            // Scripted protocol fixture, not the independent witness oracle.
            // Actual Worker captures and literal-vector tests guard that chain.
            let mut witness = IssueWitness::new(&authority).unwrap();
            let mut request_observations = Vec::new();
            for step in 0..tokens(case).len() {
                let value = observation(index, base, &command.request_id, step);
                witness = witness
                    .advanced(
                        &authority,
                        &IssuedWork {
                            logical_ordinal: value.logical_ordinal,
                            executions: value
                                .physical_batches
                                .iter()
                                .map(|physical| IssuedExecution {
                                    execution_id: physical.execution_id,
                                    rows: physical.owned_requests[0].rows.clone(),
                                })
                                .collect(),
                        },
                    )
                    .unwrap();
                request_observations.push((index, value));
            }
            all_observations.extend(request_observations.clone());
            let value = request_observations[0].1.clone();
            if !matches!(case, Case::MissingObservation) || index == 0 {
                observations.extend(request_observations);
            }
            if index == 0 {
                match case {
                    Case::ExactObservationReplay => observations.push((index, value.clone())),
                    Case::ReusedExecution => {
                        let mut duplicate = value.clone();
                        duplicate.observation_id.push_str("-new-name");
                        observations.push((index, duplicate));
                    }
                    Case::ConflictingObservation => {
                        let mut changed = value.clone();
                        // Keep dimensions valid; conflict is identity/body,
                        // not an incidental malformed dimension rejection.
                        changed.stage_ms += 1;
                        observations.push((index, changed));
                    }
                    _ => {}
                }
            }
            for (step, (text, stop)) in tokens(case).into_iter().enumerate() {
                let shifted = usize::from(matches!(case, Case::ShiftBoundary) && index == 1);
                let value = OutcomePayload {
                    load_generation: 9,
                    session_id: "session".into(),
                    request_id: command.request_id.clone(),
                    sequence_id: index as u32,
                    token: 100 + step as i32,
                    text: text.into(),
                    position: (if index == 0 { 4 } else { 7 } + shifted + step) as u32,
                    stop: stop.map(str::to_owned),
                };
                let mut approved = ApprovedOutputPayload {
                    issued_work: value.stop.as_ref().map(|_| witness.proof()),
                    release_operation_id: value.stop.as_ref().map(|_| 30 + index as u64),
                    outcome: value,
                    submission_event_id: base.envelope.event_id.clone(),
                    incarnation: 20 + index as u64,
                };
                if index == 0 {
                    match case {
                        Case::StaleOutputSubmission => {
                            approved.submission_event_id.push_str("-old")
                        }
                        Case::ChangedOutputIncarnation if step == 1 => approved.incarnation += 1,
                        Case::MissingTerminalRelease if approved.outcome.stop.is_some() => {
                            approved.release_operation_id = None
                        }
                        Case::UnexpectedNonterminalRelease if approved.outcome.stop.is_none() => {
                            approved.release_operation_id = Some(30)
                        }
                        _ => {}
                    }
                }
                outputs.push((base.clone(), approved));
            }
        }
        if matches!(case, Case::ReorderedObservations | Case::LateObservations) {
            observations.reverse();
        }
        let mut events = Vec::new();
        for (index, value) in observations {
            events.push((
                submitted[if matches!(case, Case::ForeignObservationCarrier) && index == 0 {
                    1
                } else {
                    index
                }]
                .0
                .clone(),
                EventClass::Telemetry,
                BATCH_OBSERVATION_CONTENT_TYPE,
                serde_json::to_vec(&value).unwrap(),
            ));
        }
        let output_events = outputs.into_iter().map(|(base, value)| {
            let payload = if matches!(case, Case::LegacyOutput) {
                serde_json::to_vec(&value.outcome).unwrap()
            } else {
                serde_json::to_vec(&value).unwrap()
            };
            (base, EventClass::Output, OUTPUT_CONTENT_TYPE, payload)
        });
        if matches!(case, Case::LateObservations) {
            let mut late = output_events.collect::<Vec<_>>();
            late.extend(events);
            events = late;
        } else {
            events.extend(output_events);
        }
        let first = release_members[0].clone();
        let receipt_groups = match case {
            Case::DuplicateAOnly => vec![vec![first.clone()], vec![first.clone()]],
            Case::ExactMemberReplay
            | Case::DuplicateReceiptEnvelope
            | Case::ChangedMemberReplay => vec![
                vec![first.clone()],
                vec![first.clone()],
                vec![release_members[1].clone()],
            ],
            Case::GroupedReceipt | Case::MalformedLaterMember => vec![release_members.clone()],
            Case::ReversedGroupedReceipt => vec![release_members.iter().rev().cloned().collect()],
            Case::MixedReplayFresh => vec![vec![first.clone()], release_members.clone()],
            Case::DuplicateMember => vec![
                vec![first.clone(), first.clone()],
                vec![release_members[1].clone()],
            ],
            _ => release_members
                .iter()
                .cloned()
                .map(|member| vec![member])
                .collect(),
        };
        let mut receipts = Vec::new();
        for (index, members) in receipt_groups.into_iter().enumerate() {
            let base = if matches!(case, Case::ForeignKnownCorrelation) && index == 0 {
                &submitted[1].0
            } else {
                &submitted
                    .iter()
                    .find(|(_, command)| command.request_id == members[0].request_id)
                    .unwrap()
                    .0
            };
            let mut value = ReleaseReceipt {
                load_generation: 9,
                session_id: "session".into(),
                members,
            };
            if matches!(case, Case::ChangedMemberReplay) && index == 1 {
                value.members[0].operation_id += 1;
            }
            if index == 0 {
                match case {
                    Case::MalformedLaterMember => value.members[1].operation_id += 1,
                    Case::StaleReceiptSubmission => {
                        value.members[0].submission_event_id.push_str("-old")
                    }
                    Case::StaleReceiptIncarnation => value.members[0].incarnation += 1,
                    Case::StaleReceiptOperation => value.members[0].operation_id += 1,
                    Case::WrongReceiptSlot => value.members[0].sequence_id += 100,
                    Case::ForeignReceiptRequest => value.members[0].request_id = "foreign".into(),
                    Case::StaleReceiptLoad => value.load_generation += 1,
                    Case::StaleReceiptSession => value.session_id.push_str("-old"),
                    _ => {}
                }
            }
            let (content_type, payload) = if matches!(case, Case::ScalarRelease) {
                (
                    "application/vnd.p4.llamacpp.released-v4+json",
                    serde_json::to_vec(&serde_json::json!({
                        "load_generation": 9, "session_id": "session", "released": 1,
                    }))
                    .unwrap(),
                )
            } else {
                (
                    RELEASE_RECEIPT_CONTENT_TYPE,
                    serde_json::to_vec(&value).unwrap(),
                )
            };
            receipts.push((base.clone(), EventClass::Telemetry, content_type, payload));
        }
        if matches!(case, Case::ReceiptBeforeTerminal) {
            // The first receipt is authentic in identity but arrives before
            // OUTPUT established its independently checked expectation.
            events.insert(0, receipts.remove(0));
        }
        events.extend(receipts);
        // The complete script fits the bounded duplex buffer even when drive
        // rejects early. This avoids making peer BrokenPipe a rejection oracle.
        let mut first_receipt_serial = None;
        for (base, class, content_type, payload) in events {
            let mut event_serial = serial;
            if matches!(case, Case::DuplicateReceiptEnvelope)
                && content_type == RELEASE_RECEIPT_CONTENT_TYPE
            {
                if let Some(first) = first_receipt_serial {
                    event_serial = first;
                } else {
                    first_receipt_serial = Some(serial);
                }
            }
            wire.send(response(
                &base,
                &head,
                event_serial,
                class,
                content_type,
                payload,
            ))
            .await
            .unwrap();
            serial += 1;
        }
        // Stage evidence is intentionally later than the last release receipt.
        // It uses scripted independent stage positions, not wall-clock overlap.
        for (index, observation) in all_observations {
            let physical = &observation.physical_batches[0];
            for node in ["head", "tail"] {
                let span = StageSpan {
                    load_generation: 9,
                    session_id: "session".into(),
                    execution_ids: vec![physical.execution_id],
                    executions: vec![StageExecutionObservation {
                        execution_id: physical.execution_id,
                        owned_requests: physical
                            .owned_requests
                            .iter()
                            .map(|owner| StageRequestObservation {
                                request_id: owner.request_id.clone(),
                                sequence_id: owner.sequence_id,
                                incarnation: owner.incarnation,
                            })
                            .collect(),
                    }],
                    rows: physical.rows,
                    ingress_unix_ms: 1,
                    start_unix_ms: 2,
                    end_unix_ms: 3,
                    forward_unix_ms: 4,
                };
                let source = Endpoint::node(Address::tcp("127.0.0.1", 52501), node, 3);
                let carrier = if matches!(case, Case::ForeignSpanCarrier) && index == 0 {
                    1
                } else {
                    index
                };
                wire.send(response(
                    &submitted[carrier].0,
                    &source,
                    serial,
                    EventClass::Telemetry,
                    STAGE_SPAN_CONTENT_TYPE,
                    serde_json::to_vec(&span).unwrap(),
                ))
                .await
                .unwrap();
                serial += 1;
            }
        }
    });
    let result = inference::drive(&config, &mut wire, &mut sender)
        .await
        .map_err(|error| error.to_string());
    peer_task.await.unwrap();
    let run = result?;
    // Aggregation belongs to production drive. Do not repair its result here
    // or count accepted observations a second time in the test.
    let acceptance = acceptance::evaluate(&config, &run.requests);
    Ok(Approved { run, acceptance })
}

async fn approved(case: Case) -> Approved {
    let result = exercise(case)
        .await
        .unwrap_or_else(|error| panic!("{case:?}: normal stream was rejected: {error}"));
    assert!(result.run.error.is_none());
    assert_eq!(result.run.completed_count, result.run.request_count);
    assert_eq!(result.run.released_count, result.run.request_count);
    assert!(
        result.acceptance.passed,
        "{case:?}: {:?}",
        result.acceptance
    );
    result
}

async fn rejected(case: Case, diagnostic: &[&str]) {
    match exercise(case).await {
        Err(error) => assert!(
            diagnostic.iter().any(|needle| error.contains(needle)),
            "{case:?}: expected protocol rejection, got {error}"
        ),
        Ok(result) => panic!(
            "{case:?}: invalid stream reached completion: sampled={:?}, acceptance={:?}",
            result
                .run
                .requests
                .iter()
                .map(|r| r.outcomes.len())
                .collect::<Vec<_>>(),
            result.acceptance,
        ),
    }
}

#[tokio::test]
async fn normal_different_prompt_boundaries_are_accepted() {
    let result = approved(Case::Normal).await;
    assert_eq!(
        result
            .run
            .requests
            .iter()
            .map(|r| r.outcomes[0].position)
            .collect::<Vec<_>>(),
        [4, 7]
    );
    assert_eq!(
        result
            .run
            .requests
            .iter()
            .map(|r| r.outcomes.len())
            .collect::<Vec<_>>(),
        [2, 2]
    );
}

#[tokio::test]
async fn cap_one_cannot_admit_two_contiguous_outputs() {
    rejected(Case::ExceedCap, &["max_tokens", "budget"]).await;
}

#[tokio::test]
async fn first_output_is_bound_to_each_observed_prompt_end() {
    rejected(Case::ShiftBoundary, &["prefill", "boundary"]).await;
}

#[tokio::test]
async fn completion_requires_each_requests_prompt_observation() {
    rejected(
        Case::MissingObservation,
        &["prefill", "observation", "boundary"],
    )
    .await;
}

#[tokio::test]
async fn exact_observation_replay_is_idempotent_and_aggregated_once() {
    let result = approved(Case::ExactObservationReplay).await;
    // Complete issue evidence now includes one prefill and one decode per request.
    assert_eq!(result.run.batch_observations.len(), 4);
    assert_eq!(
        result
            .run
            .requests
            .iter()
            .map(|r| r.prefill_rows)
            .collect::<Vec<_>>(),
        [4, 7]
    );
}

#[tokio::test]
async fn a_fresh_observation_name_cannot_reuse_a_physical_execution() {
    rejected(Case::ReusedExecution, &["execution", "observation"]).await;
}

#[tokio::test]
async fn an_observation_identity_cannot_change_its_body() {
    rejected(Case::ConflictingObservation, &["observation"]).await;
}

#[tokio::test]
async fn reordered_request_observations_preserve_their_own_rows() {
    let result = approved(Case::ReorderedObservations).await;
    assert_eq!(
        result
            .run
            .requests
            .iter()
            .map(|r| r.prefill_rows)
            .collect::<Vec<_>>(),
        [4, 7]
    );
}

#[tokio::test]
async fn observations_after_outputs_before_final_release_are_valid() {
    let result = approved(Case::LateObservations).await;
    assert_eq!(
        result
            .run
            .requests
            .iter()
            .map(|r| r.prefill_rows)
            .collect::<Vec<_>>(),
        [4, 7]
    );
}

#[tokio::test]
async fn empty_eos_uses_one_sample_not_two_and_keeps_quality_gate_separate() {
    let result = exercise(Case::EmptyEos).await.unwrap();
    assert_eq!(result.run.completed_count, 1);
    assert_eq!(result.run.requests[0].outcomes.len(), 1);
    assert_eq!(result.acceptance.requests[0].sampled_tokens, 1);
    assert_eq!(result.acceptance.requests[0].generated_tokens, 0);
    // Protocol-valid terminal EOS is not evidence of a useful normal response.
    // Preserve the existing nonempty/minimum-generated-token acceptance gates.
    assert!(!result.acceptance.passed);
    assert!(
        result.acceptance.requests[0]
            .failures
            .iter()
            .any(|f| f.contains("empty"))
    );
}

#[tokio::test]
async fn a_stop_sequence_can_finish_before_the_token_budget() {
    let result = approved(Case::EarlyStop).await;
    assert_eq!(result.run.requests[0].outcomes.len(), 1);
}

#[tokio::test]
async fn empty_eos_after_normal_text_can_finish_before_the_budget() {
    let result = approved(Case::EarlyEos).await;
    assert_eq!(result.acceptance.requests[0].sampled_tokens, 2);
    assert_eq!(result.acceptance.requests[0].generated_tokens, 1);
}

#[tokio::test]
async fn length_cannot_finish_before_the_submitted_token_budget() {
    rejected(Case::PrematureLength, &["length", "max_tokens", "budget"]).await;
}

#[tokio::test]
async fn unknown_stop_is_invalid_even_without_a_scenario_allowlist() {
    rejected(Case::UnknownStop, &["stop"]).await;
}

#[tokio::test]
async fn no_output_is_applied_after_a_terminal_stop() {
    rejected(Case::OutputAfterStop, &["terminal"]).await;
}

#[tokio::test]
async fn duplicate_a_release_cannot_complete_the_unreleased_b() {
    // Same normal outputs/rows as the historical scalar-count RED. Only A is
    // released twice with fresh envelopes; B never has a receipt.
    rejected(Case::DuplicateAOnly, &["event stream ended"]).await;
}

#[tokio::test]
async fn exact_member_replay_on_fresh_envelopes_is_idempotent() {
    for case in [Case::ExactMemberReplay, Case::MixedReplayFresh] {
        let result = approved(case).await;
        assert_eq!(result.run.released_count, 2);
        assert!(result.run.requests.iter().all(|request| request.released));
    }
}

#[tokio::test]
async fn grouped_receipt_members_can_be_in_either_order() {
    for case in [Case::GroupedReceipt, Case::ReversedGroupedReceipt] {
        let result = approved(case).await;
        for (index, request) in result.run.requests.iter().enumerate() {
            let member = request.release_member.as_ref().unwrap();
            assert_eq!(member.submission_event_id, request.submission_event_id);
            assert_eq!(member.sequence_id, index as u32);
            assert_eq!(member.incarnation, 20 + index as u64);
            assert_eq!(member.operation_id, 30 + index as u64);
            assert!(request.released);
        }
    }
}

#[tokio::test]
async fn malformed_or_repeated_receipt_members_are_not_partially_approved() {
    rejected(Case::MalformedLaterMember, &["approved terminal member"]).await;
    rejected(Case::DuplicateMember, &["repeated members"]).await;
}

#[tokio::test]
async fn receipt_cannot_precede_its_terminal_expectation() {
    rejected(Case::ReceiptBeforeTerminal, &["approved terminal member"]).await;
}

#[tokio::test]
async fn release_members_are_bound_to_the_sent_attempt_owner_and_operation() {
    for case in [
        Case::StaleReceiptSubmission,
        Case::StaleReceiptIncarnation,
        Case::StaleReceiptOperation,
        Case::WrongReceiptSlot,
    ] {
        rejected(case, &["approved terminal member"]).await;
    }
    rejected(
        Case::ForeignReceiptRequest,
        &["correlation is not one of its members"],
    )
    .await;
}

#[tokio::test]
async fn release_receipt_load_and_session_remain_current() {
    for case in [Case::StaleReceiptLoad, Case::StaleReceiptSession] {
        rejected(case, &["load or session identity is stale"]).await;
    }
}

#[tokio::test]
async fn outputs_cannot_change_the_submission_or_incarnation() {
    rejected(Case::StaleOutputSubmission, &["submission identity"]).await;
    rejected(Case::ChangedOutputIncarnation, &["incarnation"]).await;
}

#[tokio::test]
async fn only_a_terminal_output_may_establish_a_release_operation() {
    for case in [
        Case::MissingTerminalRelease,
        Case::UnexpectedNonterminalRelease,
    ] {
        rejected(case, &["release operation"]).await;
    }
}

#[tokio::test]
async fn legacy_scalar_release_and_unsigned_output_fail_closed() {
    rejected(
        Case::ScalarRelease,
        &["unexpected inference event content type"],
    )
    .await;
    rejected(Case::LegacyOutput, &["submission_event_id"]).await;
}

#[tokio::test]
async fn a_known_unrelated_request_cannot_supply_the_receipt_correlation() {
    rejected(
        Case::ForeignKnownCorrelation,
        &["correlation is not one of its members"],
    )
    .await;
}

#[tokio::test]
async fn receipt_replay_does_not_relax_envelope_uniqueness_or_member_equality() {
    rejected(
        Case::DuplicateReceiptEnvelope,
        &["duplicate inference event identity"],
    )
    .await;
    rejected(Case::ChangedMemberReplay, &["approved terminal member"]).await;
}

#[tokio::test]
async fn a_future_wave_cannot_extend_the_overall_evidence_deadline_or_send_after_it() {
    let mut config = config(Case::Normal);
    config.waves = vec![
        ArrivalWave {
            after_ms: 0,
            count: 1,
        },
        ArrivalWave {
            after_ms: 1_000,
            count: 1,
        },
    ];
    config.timeout_ms = 20;
    config::validate(&config).unwrap();
    let (client, peer) = duplex(32 * 1024);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = wire::EventWire::new(reader, writer);
    let mut sender = Sender::new(OuterEndpoint {
        ingress_agent: Address::from_str(&config.ingress_agent).unwrap(),
        channel: config.channel.clone(),
        connection_generation: config.connection_generation,
    });
    let peer = async move {
        let (reader, writer) = tokio::io::split(peer);
        let mut wire = wire::EventWire::new(reader, writer);
        let first = wire
            .receive(Instant::now() + Duration::from_millis(200))
            .await
            .unwrap();
        assert_eq!(first.envelope.payload_content_type, PREFILL_CONTENT_TYPE);
        let second = wire
            .receive(Instant::now() + Duration::from_millis(100))
            .await;
        assert!(
            second.is_err(),
            "no second PREFILL may be sent after the overall deadline"
        );
    };
    let (result, ()) = tokio::join!(inference::drive(&config, &mut wire, &mut sender), peer);
    let error = result.unwrap_err().to_string();
    assert!(error.contains("overall deadline expired"), "{error}");
    assert_eq!(sender.sequence, 2, "exactly one submitted event was minted");
}

#[tokio::test]
async fn a_known_unrelated_submission_cannot_carry_another_requests_observation_or_span() {
    rejected(
        Case::ForeignObservationCarrier,
        &["carrier is not a recipient-owned member"],
    )
    .await;
    rejected(
        Case::ForeignSpanCarrier,
        &["carrier is not a recipient-owned member"],
    )
    .await;
}
