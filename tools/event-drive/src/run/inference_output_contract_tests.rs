//! The fixture is captured from actual Worker::run mailbox outputs and checked
//! against the live producer by adapter default tests. This consumer reuses its
//! complete event wire, not hand-written outcomes or a test-only public API.
//! Native computation is scripted; neither side claims GPU/model validation.
use super::{config, node};
use crate::run::{Sender, inference, inference_identity::InferenceIdentity, wire::EventWire};
use p4_llamacpp_staged_adapter::v2::{
    ApprovedOutputPayload, BatchObservation, InferenceCommand, OutcomePayload, ReleaseReceipt,
    StageSpan,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event, EventClass, OuterEndpoint, encode};

#[path = "../../../../layers/adapters/llamacpp/staged/adapter/test-fixtures/head_approved_output.rs"]
mod captured;

fn outer() -> OuterEndpoint {
    OuterEndpoint {
        ingress_agent: Address::tcp("127.0.0.1", 42999),
        channel: "loop-output".into(),
        connection_generation: 1,
    }
}

fn endpoint(index: usize) -> Endpoint {
    Endpoint::node(Address::tcp("127.0.0.1", 42999), format!("loop-{index}"), 1)
}

fn fixture_config(case: &captured::FixtureCase, request: &str) -> crate::run::RunConfig {
    let (prefill_rows, max_tokens) = match (case.id.as_str(), request) {
        ("ordinary-2", "one") => (7, 5),
        ("checkpoint-2" | "checkpoint-4", "partial") => (3, 4),
        ("checkpoint-2" | "checkpoint-4", "fence-probe") => (1, 1),
        ("mixed-consumer-2" | "mixed-consumer-4", "release-owner-a" | "release-owner-b") => (1, 1),
        other => panic!("unknown independently specified fixture workload: {other:?}"),
    };
    let mut config = config();
    config.ingress_agent = "tcp://127.0.0.1:42999".into();
    config.channel = "loop-output".into();
    config.connection_generation = 1;
    config.load_generation = 1;
    config.session_id = "loop-session".into();
    config.request_id = request.into();
    config.max_tokens = max_tokens;
    config.acceptance.expected_prefill_rows = Some(prefill_rows);
    config.nodes = (0..case.stage_count)
        .map(|index| {
            let mut node = node(&format!("loop-{index}"));
            node.agent = config.ingress_agent.clone();
            node.generation = 1;
            node
        })
        .collect();
    config
}

fn request_events(case: &captured::FixtureCase, request: &str) -> Vec<Event> {
    // The shared artifact is a set of captured events, not a transport-order
    // log. The live producer parity separately checks each request's order.
    let mut events: Vec<_> = case
        .events
        .iter()
        .filter(|event| event.envelope.correlation_id == request)
        .cloned()
        .collect();
    events.sort_by_key(|event| outcome(event).position);
    events
}

fn outcome(event: &Event) -> OutcomePayload {
    serde_json::from_slice(&event.payload).expect("captured output payload")
}

fn change_outcome(event: &mut Event, change: impl FnOnce(&mut OutcomePayload)) {
    // Preserve the approval envelope while changing exactly the old oracle's
    // token/identity field; otherwise a missing signature masks the intended
    // position/load/after-stop refusal.
    let mut value: ApprovedOutputPayload = serde_json::from_slice(&event.payload).unwrap();
    change(&mut value.outcome);
    event.payload = serde_json::to_vec(&value).unwrap();
}

// OUTPUT, RELEASE_RECEIPT and the submission identity come from actual workers.
// Observations and spans are actual captured worker events too. Single-request
// fixtures may select whole events owned solely by that request; never rewrite
// a mixed-owner body to make it pass. This is not a whole execute/LOAD test.
async fn consume(
    case: &captured::FixtureCase,
    request: &str,
    outputs: &[Event],
) -> Result<inference::InferenceResult, String> {
    consume_changed(case, request, outputs, |_| {}).await
}

async fn consume_changed(
    case: &captured::FixtureCase,
    request: &str,
    outputs: &[Event],
    change: impl FnOnce(&mut Vec<Event>),
) -> Result<inference::InferenceResult, String> {
    consume_delayed(case, request, outputs, change, false).await
}

async fn consume_delayed(
    case: &captured::FixtureCase,
    request: &str,
    outputs: &[Event],
    change: impl FnOnce(&mut Vec<Event>),
    delay_telemetry: bool,
) -> Result<inference::InferenceResult, String> {
    let submission = case
        .submissions
        .iter()
        .find(|event| {
            let command: InferenceCommand = serde_json::from_slice(&event.payload).unwrap();
            command.request_id == request
        })
        .expect("actual captured submission")
        .clone();
    let submitted: InferenceCommand = serde_json::from_slice(&submission.payload).unwrap();
    let mut config = fixture_config(case, request);
    config.options = submitted.options.clone();
    let Endpoint::Outer(submitted_outer) = &submission.envelope.source else {
        panic!("captured request has an OUTER source");
    };
    config.ingress_agent = submitted_outer.ingress_agent.to_string();
    config.channel = submitted_outer.channel.clone();
    config.connection_generation = submitted_outer.connection_generation;
    let released: Vec<Event> = case
        .receipts
        .iter()
        .filter(|event| {
            let receipt: ReleaseReceipt = serde_json::from_slice(&event.payload).unwrap();
            receipt
                .members
                .iter()
                .any(|member| member.request_id == request)
        })
        .cloned()
        .collect();
    assert!(!released.is_empty(), "actual captured release receipt");
    for event in &released {
        let receipt: ReleaseReceipt = serde_json::from_slice(&event.payload).unwrap();
        assert!(
            receipt
                .members
                .iter()
                .all(|member| member.request_id == request),
            "a grouped live receipt requires a whole-group consumer fixture, not rewritten bytes"
        );
    }
    let observations = case.observations.iter().filter(|event| {
        let observation: BatchObservation = serde_json::from_slice(&event.payload).unwrap();
        let owners: Vec<_> = observation
            .physical_batches
            .iter()
            .flat_map(|batch| &batch.owned_requests)
            .collect();
        !owners.is_empty() && owners.iter().all(|owner| owner.request_id == request)
    });
    let spans = case.spans.iter().filter(|event| {
        let span: StageSpan = serde_json::from_slice(&event.payload).unwrap();
        let owners: Vec<_> = span
            .executions
            .iter()
            .flat_map(|batch| &batch.owned_requests)
            .collect();
        !owners.is_empty() && owners.iter().all(|owner| owner.request_id == request)
    });
    let mut response_events: Vec<Event> = outputs
        .iter()
        .chain(released.iter())
        .chain(observations)
        .chain(spans)
        .cloned()
        .collect();
    change(&mut response_events);
    for event in &response_events {
        encode(event).expect("negative case must remain codec-valid");
    }
    let (client, peer) = tokio::io::duplex(128 * 1024);
    let (reader, writer) = tokio::io::split(client);
    let mut wire = EventWire::new(reader, writer);
    let mut sender = Sender::new(submitted_outer.clone());
    // Match the independently captured submission sequence. Do not derive it
    // from OUTPUT or alter any captured OUTPUT/receipt bytes. This fixture uses
    // explicit native tokens; drive emits a prompt, so only the submission's
    // routing/attempt and command identity are shared, not tokenization proof.
    sender.sequence = submission.envelope.sequence;
    let peer = async move {
        let (reader, writer) = tokio::io::split(peer);
        let mut wire = EventWire::new(reader, writer);
        let actual = wire
            .receive(std::time::Instant::now() + std::time::Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(actual.envelope, submission.envelope);
        let command: InferenceCommand = serde_json::from_slice(&actual.payload).unwrap();
        command.validate().unwrap();
        assert_eq!(command.load_generation, submitted.load_generation);
        assert_eq!(command.session_id, submitted.session_id);
        assert_eq!(command.request_id, submitted.request_id);
        assert_eq!(command.max_tokens, submitted.max_tokens);
        assert_eq!(command.options, submitted.options);
        let mut delayed = false;
        for event in response_events {
            if delay_telemetry
                && !delayed
                && event.envelope.payload_content_type
                    == p4_llamacpp_staged_adapter::v2::BATCH_OBSERVATION_CONTENT_TYPE
            {
                delayed = true;
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            }
            wire.send(event).await.unwrap();
        }
    };
    let (result, ()) = tokio::join!(inference::drive(&config, &mut wire, &mut sender), peer);
    // A refusal after the first submission ends the run and is reported on
    // the result rather than discarding it. These cases judge what is
    // refused, so they read the refusal from there; what a refused run keeps
    // is fixed by `partial_result_preservation_tests`.
    let run = result.map_err(|error| error.to_string())?;
    match run.error {
        Some(error) => Err(error),
        None => Ok(run),
    }
}

#[tokio::test]
async fn actual_head_output_captures_pass_identity_and_the_real_inference_consumer() {
    let cases: Vec<_> = captured::cases()
        .into_iter()
        .filter(|case| {
            matches!(
                case.id.as_str(),
                "ordinary-2" | "checkpoint-2" | "checkpoint-4"
            )
        })
        .collect();
    assert_eq!(cases.len(), 3);
    let mut checked = 0;
    for case in cases {
        let requests: &[&str] = if case.id == "ordinary-2" {
            &["one"]
        } else {
            &["partial", "fence-probe"]
        };
        for request in requests {
            let config = fixture_config(&case, request);
            let events = request_events(&case, request);
            assert_eq!(events.len(), config.max_tokens as usize);
            let identity = InferenceIdentity::new(&config, &outer()).unwrap();
            let mut previous = None;
            for event in &events {
                assert_eq!(event.envelope.source, endpoint(0));
                let value = outcome(event);
                identity.output(event, &value, previous.as_ref()).unwrap();
                previous = Some(value);
                checked += 1;
            }
            let result = consume(&case, request, &events).await.unwrap();
            assert_eq!(
                (
                    result.request_count,
                    result.completed_count,
                    result.released_count
                ),
                (1, 1, 1)
            );
            assert!(result.error.is_none());
            assert_eq!(result.requests.len(), 1);
            assert_eq!(result.requests[0].request_id, *request);
            let expected: Vec<_> = events.iter().map(outcome).collect();
            assert_eq!(
                serde_json::to_value(&result.requests[0].outcomes).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(
                result.requests[0].response,
                expected
                    .iter()
                    .map(|value| value.text.as_str())
                    .collect::<String>()
            );
        }
    }
    assert_eq!(checked, 15);
}

#[tokio::test]
async fn captured_outputs_reject_tail_middle_other_head_generation_and_route_changes() {
    let case = captured::cases()
        .into_iter()
        .find(|case| case.id == "checkpoint-4")
        .unwrap();
    let original = request_events(&case, "partial");
    for (name, source) in [
        ("tail", endpoint(3)),
        ("middle", endpoint(1)),
        (
            "other head",
            Endpoint::node(Address::tcp("127.0.0.1", 42999), "other-head", 1),
        ),
        (
            "head generation",
            Endpoint::node(Address::tcp("127.0.0.1", 42999), "loop-0", 2),
        ),
        (
            "head host",
            Endpoint::node(Address::tcp("127.0.0.2", 42999), "loop-0", 1),
        ),
    ] {
        let mut changed = original.clone();
        changed[0].envelope.source = source;
        assert_eq!(
            consume(&case, "partial", &changed).await.err().as_deref(),
            Some("inference event source or correlation is incorrect"),
            "{name}"
        );
    }
    for mutation in 0..8 {
        let mut changed = original.clone();
        let envelope = &mut changed[0].envelope;
        match mutation {
            0 => envelope.target = Endpoint::outer(Address::tcp("127.0.0.1", 42999), "wrong", 1),
            1 => envelope.return_route.as_mut().unwrap().channel = "wrong".into(),
            2 => {
                envelope
                    .return_route
                    .as_mut()
                    .unwrap()
                    .connection_generation += 1
            }
            3 => envelope.return_route = None,
            4 => envelope.causation_id = None,
            5 => envelope.class = EventClass::Telemetry,
            6 => envelope.adapter_kind = Some("mock".into()),
            7 => {
                envelope.target =
                    Endpoint::outer(Address::tcp("127.0.0.1", 42999), "loop-output", 2)
            }
            _ => unreachable!(),
        }
        assert_eq!(
            consume(&case, "partial", &changed).await.err().as_deref(),
            Some("inference event route is not self-consistent"),
            "route mutation {mutation}"
        );
    }
}

#[tokio::test]
async fn captured_outputs_keep_load_session_request_position_and_sequence_fences() {
    let case = captured::cases()
        .into_iter()
        .find(|case| case.id == "ordinary-2")
        .unwrap();
    let original = request_events(&case, "one");
    for mutation in 0..2 {
        let mut changed = original.clone();
        change_outcome(&mut changed[0], |value| {
            if mutation == 0 {
                value.load_generation += 1;
            } else {
                value.session_id = "other-session".into();
            }
        });
        assert_eq!(
            consume(&case, "one", &changed).await.err().as_deref(),
            Some("output load or session identity is stale")
        );
    }
    let mut changed = original.clone();
    change_outcome(&mut changed[0], |value| {
        value.request_id = "never-submitted".into()
    });
    assert_eq!(
        consume(&case, "one", &changed).await.err().as_deref(),
        Some("output references a request that was not submitted")
    );
    let mut changed = original.clone();
    changed[0].envelope.correlation_id = "other-request".into();
    assert_eq!(
        consume(&case, "one", &changed).await.err().as_deref(),
        Some("inference event correlation is not a submitted request")
    );
    let mut changed = original.clone();
    change_outcome(&mut changed[0], |value| value.position -= 1);
    assert_eq!(
        consume(&case, "one", &changed).await.err().as_deref(),
        Some("first output position does not follow the exact prefill boundary")
    );
    for position in [6, 7, 9] {
        let mut changed = original.clone();
        change_outcome(&mut changed[1], |value| value.position = position);
        let error = consume(&case, "one", &changed).await.err().unwrap();
        assert!(
            error.starts_with("output token positions are not contiguous:"),
            "{error}"
        );
    }
    let mut changed = original.clone();
    change_outcome(&mut changed[1], |value| value.sequence_id += 1);
    assert_eq!(
        consume(&case, "one", &changed).await.err().as_deref(),
        Some("output changed sequence identity within one request")
    );
}

#[tokio::test]
async fn actual_inference_consumer_rejects_duplicate_events_and_outputs_after_stop() {
    let case = captured::cases()
        .into_iter()
        .find(|case| case.id == "ordinary-2")
        .unwrap();
    let original = request_events(&case, "one");
    let mut duplicated = original.clone();
    duplicated.insert(1, original[0].clone());
    assert_eq!(
        consume(&case, "one", &duplicated).await.err().as_deref(),
        Some("duplicate inference event identity")
    );
    let mut after_stop = original.clone();
    let mut extra = original.last().unwrap().clone();
    extra.envelope.event_id.push_str("-after-stop");
    change_outcome(&mut extra, |value| {
        value.position += 1;
        value.stop = None;
    });
    after_stop.push(extra);
    assert_eq!(
        consume(&case, "one", &after_stop).await.err().as_deref(),
        Some("output arrived after a terminal outcome")
    );
}

#[tokio::test]
async fn legacy_actual_output_captures_cannot_bypass_submission_approval() {
    let current = captured::cases()
        .into_iter()
        .find(|case| case.id == "ordinary-2")
        .unwrap();
    let legacy = captured::legacy_cases()
        .into_iter()
        .find(|case| case.id == "ordinary-2")
        .unwrap();
    let error = consume(&current, "one", &request_events(&legacy, "one"))
        .await
        .err()
        .unwrap();
    assert_eq!(
        error,
        "unexpected inference event content type: application/vnd.p4.llamacpp.output-v3+json"
    );
}

fn ordinary_case() -> captured::FixtureCase {
    captured::cases()
        .into_iter()
        .find(|case| case.id == "ordinary-2")
        .unwrap()
}

fn edit_observation(event: &mut Event, change: impl FnOnce(&mut BatchObservation)) {
    let mut value: BatchObservation = serde_json::from_slice(&event.payload).unwrap();
    change(&mut value);
    event.payload = serde_json::to_vec(&value).unwrap();
}

fn edit_span(event: &mut Event, change: impl FnOnce(&mut StageSpan)) {
    let mut value: StageSpan = serde_json::from_slice(&event.payload).unwrap();
    change(&mut value);
    event.payload = serde_json::to_vec(&value).unwrap();
}

#[tokio::test]
async fn actual_terminal_proof_refuses_missing_count_ordinal_authority_and_digest() {
    let case = ordinary_case();
    let original = request_events(&case, "one");
    for mutation in 0..6 {
        let mut outputs = original.clone();
        let event = outputs.last_mut().unwrap();
        let mut output: ApprovedOutputPayload = serde_json::from_slice(&event.payload).unwrap();
        let proof = output.issued_work.as_mut().unwrap();
        match mutation {
            0 => proof.revision += 1,
            1 => proof.issue_count += 1,
            2 => proof.last_ordinal += 1,
            3 => proof.authority_digest[0] ^= 1,
            4 => proof.digest[0] ^= 1,
            5 => output.issued_work = None,
            _ => unreachable!(),
        }
        event.payload = serde_json::to_vec(&output).unwrap();
        let error = consume(&case, "one", &outputs).await.unwrap_err();
        assert!(
            error.contains("issued-work") || (mutation == 1 && error.contains("Missing")),
            "mutation {mutation}: {error}"
        );
    }
}

#[tokio::test]
async fn removing_an_entire_late_decode_observation_cannot_self_certify_completeness() {
    use p4_llamacpp_staged_adapter::v2::BATCH_OBSERVATION_CONTENT_TYPE;
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    let error = consume_changed(&case, "one", &outputs, |events| {
        let at = events
            .iter()
            .rposition(|event| {
                event.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE
            })
            .unwrap();
        let value: BatchObservation = serde_json::from_slice(&events[at].payload).unwrap();
        assert!(
            value
                .physical_batches
                .iter()
                .all(|batch| batch.prefill_rows == 0)
        );
        events.remove(at);
    })
    .await
    .unwrap_err();
    assert!(error.contains("Missing"), "{error}");
}

#[tokio::test]
async fn deleting_a_middle_issue_and_every_corresponding_span_still_leaves_the_terminal_chain_incomplete()
 {
    use p4_llamacpp_staged_adapter::v2::{BATCH_OBSERVATION_CONTENT_TYPE, STAGE_SPAN_CONTENT_TYPE};
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    for remove_spans in [false, true] {
        let error = consume_changed(&case, "one", &outputs, |events| {
            let at = events
                .iter()
                .position(|event| {
                    event.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE
                        && serde_json::from_slice::<BatchObservation>(&event.payload)
                            .unwrap()
                            .logical_ordinal
                            == 4
                })
                .unwrap();
            let removed: BatchObservation = serde_json::from_slice(&events[at].payload).unwrap();
            assert_eq!(
                removed
                    .physical_batches
                    .iter()
                    .map(|batch| batch.execution_id)
                    .collect::<Vec<_>>(),
                [6]
            );
            assert_eq!(removed.physical_batches[0].decode_rows, 1);
            events.remove(at);
            if remove_spans {
                let mut removed_stages = Vec::new();
                events.retain(|event| {
                    if event.envelope.payload_content_type != STAGE_SPAN_CONTENT_TYPE {
                        return true;
                    }
                    let span: StageSpan = serde_json::from_slice(&event.payload).unwrap();
                    if span.execution_ids.contains(&6) {
                        assert_eq!(
                            span.execution_ids,
                            [6],
                            "delete whole span events, never rewrite a partial group"
                        );
                        removed_stages.push(event.envelope.source.clone());
                        false
                    } else {
                        true
                    }
                });
                assert_eq!(removed_stages, [endpoint(0), endpoint(1)]);
            }
        })
        .await
        .unwrap_err();
        assert!(error.contains("Missing { requests: 1"), "{error}");
        if remove_spans {
            assert!(
                error.contains("stage_executions: 0"),
                "only the terminal's independent expected chain may detect this deletion: {error}"
            );
        }
    }
}

#[tokio::test]
async fn reordered_captured_issues_and_spans_complete_after_release_without_recounting() {
    use p4_llamacpp_staged_adapter::v2::{BATCH_OBSERVATION_CONTENT_TYPE, STAGE_SPAN_CONTENT_TYPE};
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    let run = consume_changed(&case, "one", &outputs, |events| {
        let at = events
            .iter()
            .position(|event| event.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE)
            .unwrap();
        events[at..].reverse(); // Spans before head observations; issues descending.
        let mut span = events
            .iter()
            .find(|event| event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE)
            .unwrap()
            .clone();
        span.envelope.event_id.push_str("-exact-replay");
        edit_span(&mut span, |value| {
            value.execution_ids.reverse();
            value.executions.reverse();
            for execution in &mut value.executions {
                execution.owned_requests.reverse();
            }
        });
        events.insert(at, span);
    })
    .await
    .unwrap();
    assert_eq!(run.batch_observations.len(), case.observations.len());
    assert_eq!(run.stage_spans.len(), case.spans.len());
    assert_eq!(run.requests[0].prefill_rows, 7);
    assert_eq!(run.requests[0].outcomes.len(), 5);
    assert!(run.requests[0].submission_authority.is_some());
    assert_eq!(run.requests[0].issued_work.unwrap().issue_count, 6);
    assert!(run.telemetry_complete_elapsed_ms.unwrap() >= run.elapsed_ms);
}

#[tokio::test]
async fn changed_owned_rows_attempt_or_issue_index_fail_against_captured_terminal_proof() {
    use p4_llamacpp_staged_adapter::v2::BATCH_OBSERVATION_CONTENT_TYPE;
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    for mutation in 0..5 {
        let error = consume_changed(&case, "one", &outputs, |events| {
            let event = events
                .iter_mut()
                .find(|event| event.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE)
                .unwrap();
            edit_observation(event, |value| {
                let owner = &mut value.physical_batches[0].owned_requests[0];
                match mutation {
                    0 => owner.rows[0].position += 100,
                    1 => owner.submission_event_id.push_str("-stale"),
                    2 => owner.incarnation += 1,
                    3 => owner.request_issue_index = 0,
                    4 => value.logical_ordinal += 100,
                    _ => unreachable!(),
                }
            });
        })
        .await
        .unwrap_err();
        assert!(
            error.contains("issued-work")
                || error.contains("submission")
                || error.contains("incarnation")
                || error.contains("issue identity"),
            "mutation {mutation}: {error}"
        );
    }
}

#[tokio::test]
async fn every_declared_stage_must_cover_each_owned_execution() {
    use p4_llamacpp_staged_adapter::v2::STAGE_SPAN_CONTENT_TYPE;
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    for node in 0..2 {
        let error = consume_changed(&case, "one", &outputs, |events| {
            let at = events
                .iter()
                .position(|event| {
                    event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE
                        && event.envelope.source == endpoint(node)
                })
                .unwrap();
            let removed: StageSpan = serde_json::from_slice(&events[at].payload).unwrap();
            assert_eq!(
                removed.execution_ids.len(),
                2,
                "first native prefill was physically split in two"
            );
            events.remove(at);
        })
        .await
        .unwrap_err();
        assert!(
            error.contains("Missing") && error.contains("stage_executions: 2"),
            "{error}"
        );
    }
}

#[tokio::test]
async fn captured_stage_width_owner_and_fresh_id_conflicts_are_invalid() {
    use p4_llamacpp_staged_adapter::v2::STAGE_SPAN_CONTENT_TYPE;
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    for mutation in 0..3 {
        let error = consume_changed(&case, "one", &outputs, |events| {
            let at = events
                .iter()
                .position(|event| event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE)
                .unwrap();
            let mut changed = events[at].clone();
            changed.envelope.event_id.push_str("-changed");
            edit_span(&mut changed, |span| match mutation {
                0 => span.rows += 1,
                1 => span.executions[0].owned_requests[0].incarnation += 1,
                2 => {
                    span.end_unix_ms += 1;
                    span.forward_unix_ms += 1;
                }
                _ => unreachable!(),
            });
            if mutation == 2 {
                events.insert(at + 1, changed);
            } else {
                events[at] = changed;
            }
        })
        .await
        .unwrap_err();
        assert!(
            error.contains("physical rows")
                || error.contains("incarnation")
                || error.contains("duplicate stage span"),
            "mutation {mutation}: {error}"
        );
    }
}

#[tokio::test]
async fn legacy_v4_terminal_without_an_issued_witness_is_not_v5_evidence() {
    let current = ordinary_case();
    let old = captured::output_v4_cases()
        .into_iter()
        .find(|case| case.id == "ordinary-2")
        .unwrap();
    let error = consume(&current, "one", &request_events(&old, "one"))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        "unexpected inference event content type: application/vnd.p4.llamacpp.output-v4+json"
    );
}

#[tokio::test]
async fn actual_mixed_outer_projections_keep_physical_totals_but_only_own_request_rows() {
    let cases: Vec<_> = captured::cases()
        .into_iter()
        .filter(|case| case.id.starts_with("mixed-consumer-"))
        .collect();
    assert_eq!(
        cases.len(),
        2,
        "actual two- and four-stage owner projections"
    );
    for case in cases {
        for request in ["release-owner-a", "release-owner-b"] {
            let outputs = request_events(&case, request);
            assert_eq!(outputs.len(), 1);
            let run = consume(&case, request, &outputs).await.unwrap();
            assert_eq!(
                (run.request_count, run.completed_count, run.released_count),
                (1, 1, 1)
            );
            assert_eq!(run.requests[0].prefill_rows, 1);
            assert_eq!(run.requests[0].issued_work.unwrap().issue_count, 1);
            assert_eq!(run.batch_observations.len(), 1);
            let physical = &run.batch_observations[0].physical_batches[0];
            assert_eq!(physical.rows, 2);
            assert_eq!(physical.request_count, 2);
            assert_eq!(physical.owned_requests.len(), 1);
            assert_eq!(physical.owned_requests[0].request_id, request);
            assert_eq!(run.stage_spans.len(), case.stage_count);
            assert!(
                run.stage_spans
                    .iter()
                    .all(|artifact| artifact.span.rows == 2)
            );
        }
    }
}

#[tokio::test]
async fn late_telemetry_wait_has_its_own_clock_and_does_not_inflate_release_elapsed() {
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    let run = consume_delayed(&case, "one", &outputs, |_| {}, true)
        .await
        .unwrap();
    let telemetry = run.telemetry_complete_elapsed_ms.unwrap();
    assert!(
        telemetry >= run.elapsed_ms + 30,
        "40ms post-release telemetry delay was folded into release time: release={}, telemetry={telemetry}",
        run.elapsed_ms
    );
    assert_eq!(run.requests[0].outcomes.len(), 5);
}

#[tokio::test]
async fn a_received_foreign_only_execution_still_requires_head_physical_dimensions() {
    use p4_llamacpp_staged_adapter::v2::{STAGE_SPAN_CONTENT_TYPE, StageExecutionObservation};
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    let error = consume_changed(&case, "one", &outputs, |events| {
        let event = events
            .iter_mut()
            .find(|event| event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE)
            .unwrap();
        edit_span(event, |span| {
            assert!(!span.execution_ids.contains(&999));
            span.execution_ids.push(999);
            span.executions.push(StageExecutionObservation {
                execution_id: 999,
                owned_requests: Vec::new(),
            });
            span.rows += 1;
        });
    })
    .await
    .unwrap_err();
    assert!(
        error.contains("Missing") && error.contains("stage_executions: 1"),
        "{error}"
    );
}

#[tokio::test]
async fn a_received_empty_projection_can_resolve_when_its_head_dimensions_arrive_later() {
    use p4_llamacpp_staged_adapter::v2::{
        BATCH_OBSERVATION_CONTENT_TYPE, PhysicalBatchObservation, STAGE_SPAN_CONTENT_TYPE,
        StageExecutionObservation,
    };
    let case = ordinary_case();
    let outputs = request_events(&case, "one");
    // Metamorphic wire control: a foreign execution is introduced consistently
    // on head + each stage, not claimed to be in the unmodified native capture.
    // Its rows must not enter this request's chain or phase counters.
    let run = consume_changed(&case, "one", &outputs, |events| {
        for event in events.iter_mut() {
            if event.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE {
                edit_observation(event, |observation| {
                    if observation.logical_ordinal == 1 {
                        observation.logical_rows += 1;
                        observation.physical_batches.push(PhysicalBatchObservation {
                            execution_id: 999,
                            rows: 1,
                            prefill_rows: 1,
                            decode_rows: 0,
                            verify_rows: 0,
                            replay_rows: 0,
                            request_count: 1,
                            sequence_count: 1,
                            owned_requests: Vec::new(),
                        });
                    }
                });
            } else if event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE {
                edit_span(event, |span| {
                    if span.execution_ids == [1, 2] {
                        span.execution_ids.push(999);
                        span.executions.push(StageExecutionObservation {
                            execution_id: 999,
                            owned_requests: Vec::new(),
                        });
                        span.rows += 1;
                    }
                });
            }
        }
        let at = events
            .iter()
            .position(|event| event.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE)
            .unwrap();
        events[at..].reverse();
    })
    .await
    .unwrap();
    assert_eq!(run.requests[0].prefill_rows, 7);
    assert_eq!(run.requests[0].decode_rows, 4);
    assert_eq!(run.requests[0].issued_work.unwrap().issue_count, 6);
    assert_eq!(run.stage_spans[0].span.rows, 5);
}
