//! Test-only producer/consumer contract. This file contains no scheduling or
//! settlement algorithm. The checked-in wire bytes came from actual Worker::run
//! ordinary and checkpoint Replay outputs, with a scripted native boundary.
//! v1 remains the legacy OUTPUT-v3 evidence. v2 preserves the complete original
//! PREFILLs alongside OUTPUT-v4 and exact release receipts; no approval fields
//! were added to captured bytes after production. v3 adds actual OUTPUT-v5,
//! owned observations and stage spans; both older files remain unchanged.
//!
//! Event ID, causation ID, and sequence values depend on the surrounding event
//! history. Compare every other envelope field and the entire outcome body.
//! Live uniqueness/sequence checks remain separate below. Causation is checked
//! for presence only; this helper does not prove the specific causal terminal ID.
#![allow(dead_code)] // Different test consumers exercise different entry points.

use p4_protocol::event::{Endpoint, Event, OuterEndpoint};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct FixtureCase {
    pub id: String,
    pub stage_count: usize,
    pub events: Vec<Event>,
    pub submissions: Vec<Event>,
    pub receipts: Vec<Event>,
    pub observations: Vec<Event>,
    pub spans: Vec<Event>,
}

#[derive(Deserialize)]
struct WireFile {
    format: u32,
    excluded_volatile_envelope_fields: Vec<String>,
    cases: Vec<WireCase>,
}

#[derive(Deserialize)]
struct WireCase {
    id: String,
    stage_count: usize,
    events: Vec<WireEvent>,
    #[serde(default)]
    submissions: Vec<WireEvent>,
    #[serde(default)]
    receipts: Vec<WireEvent>,
    #[serde(default)]
    observations: Vec<WireEvent>,
    #[serde(default)]
    spans: Vec<WireEvent>,
}

#[derive(Deserialize)]
struct WireEvent {
    wire_hex: String,
}

pub fn cases() -> Vec<FixtureCase> {
    decode_cases(include_str!("head-approved-output-v3.json"), 3)
}

pub fn output_v4_cases() -> Vec<FixtureCase> {
    decode_cases(include_str!("head-approved-output-v2.json"), 2)
}

pub fn legacy_cases() -> Vec<FixtureCase> {
    decode_cases(include_str!("head-approved-output-v1.json"), 1)
}

fn decode_event(value: WireEvent) -> Event {
    assert!(value.wire_hex.is_ascii());
    assert_eq!(value.wire_hex.len() % 2, 0);
    let bytes = value
        .wire_hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    let event = p4_protocol::event::decode(&bytes).unwrap();
    event.validate().unwrap();
    event
}

fn decode_cases(input: &str, version: u32) -> Vec<FixtureCase> {
    let file: WireFile = serde_json::from_str(input).unwrap();
    assert_eq!(file.format, version);
    assert_eq!(
        file.excluded_volatile_envelope_fields,
        ["event_id", "causation_id", "sequence"]
    );
    file.cases
        .into_iter()
        .map(|case| FixtureCase {
            id: case.id,
            stage_count: case.stage_count,
            events: case.events.into_iter().map(decode_event).collect(),
            submissions: case.submissions.into_iter().map(decode_event).collect(),
            receipts: case.receipts.into_iter().map(decode_event).collect(),
            observations: case.observations.into_iter().map(decode_event).collect(),
            spans: case.spans.into_iter().map(decode_event).collect(),
        })
        .collect()
}

fn outer(value: &OuterEndpoint) -> Value {
    json!({
        "ingress_agent": value.ingress_agent.to_string(),
        "channel": value.channel,
        "connection_generation": value.connection_generation,
    })
}

fn endpoint(value: &Endpoint) -> Value {
    match value {
        Endpoint::Agent(agent) => json!({"kind": "agent", "agent": agent.to_string()}),
        Endpoint::Node {
            agent,
            node,
            generation,
        } => {
            json!({"kind": "node", "agent": agent.to_string(), "node": node, "generation": generation})
        }
        Endpoint::Outer(route) => json!({"kind": "outer", "route": outer(route)}),
    }
}

pub fn semantic_projection(event: &Event) -> Value {
    json!({
        "envelope": {
            "protocol_version": event.envelope.protocol_version,
            "correlation_id": event.envelope.correlation_id,
            "source": endpoint(&event.envelope.source),
            "target": endpoint(&event.envelope.target),
            "return_route": event.envelope.return_route.as_ref().map(outer),
            "class": format!("{:?}", event.envelope.class),
            "deadline_unix_ms": event.envelope.deadline_unix_ms,
            "adapter_kind": event.envelope.adapter_kind,
            "payload_content_type": event.envelope.payload_content_type,
        },
        // Do not select known OutcomePayload fields: a newly added field must
        // force explicit fixture review instead of silently disappearing.
        "outcome": serde_json::from_slice::<Value>(&event.payload).unwrap(),
    })
}

fn sorted_semantics(events: &[Event]) -> Vec<Value> {
    let mut values: Vec<_> = events.iter().map(semantic_projection).collect();
    values.sort_by_key(|value| {
        (
            value["outcome"]["request_id"].as_str().unwrap().to_owned(),
            value["outcome"]["position"].as_u64().unwrap(),
        )
    });
    values
}

pub fn assert_live_matches(case_id: &str, submissions: &[Event], live: &[Event], all: &[Event]) {
    let previous_v4 = output_v4_cases()
        .into_iter()
        .find(|case| case.id == case_id);
    let stage_count = previous_v4.as_ref().map_or_else(
        || {
            case_id
                .rsplit('-')
                .next()
                .unwrap()
                .parse::<usize>()
                .unwrap()
        },
        |case| case.stage_count,
    );
    let mut v4_view = sorted_semantics(live);
    for value in &mut v4_view {
        assert_eq!(
            value["envelope"]["payload_content_type"],
            "application/vnd.p4.llamacpp.output-v5+json"
        );
        value["envelope"]["payload_content_type"] =
            json!("application/vnd.p4.llamacpp.output-v4+json");
        let outcome = value["outcome"].as_object_mut().unwrap();
        if outcome["stop"].is_null() {
            assert!(
                !outcome.contains_key("issued_work"),
                "nonterminal must not claim final work"
            );
        } else {
            assert!(
                outcome.remove("issued_work").is_some(),
                "terminal omitted issued work"
            );
        }
    }
    if let Some(previous) = &previous_v4 {
        assert_eq!(
            v4_view,
            sorted_semantics(&previous.events),
            "migration changed the v4 approval/token/routing oracle"
        );
        assert_eq!(
            submissions, &previous.submissions,
            "original v4 PREFILL changed"
        );
    }
    // Explicit schema migration must preserve the previous token/text/position/
    // stop and routing oracle. Remove ONLY the three newly specified fields for
    // this extra legacy comparison; the new comparison below keeps every field.
    if let Some(previous) = legacy_cases().into_iter().find(|case| case.id == case_id) {
        let mut old_view = v4_view;
        for value in &mut old_view {
            assert_eq!(
                value["envelope"]["payload_content_type"],
                "application/vnd.p4.llamacpp.output-v4+json"
            );
            value["envelope"]["payload_content_type"] =
                json!("application/vnd.p4.llamacpp.output-v3+json");
            let outcome = value["outcome"].as_object_mut().unwrap();
            for field in ["submission_event_id", "incarnation", "release_operation_id"] {
                assert!(
                    outcome.remove(field).is_some(),
                    "new output omitted {field}"
                );
            }
        }
        assert_eq!(
            old_view,
            sorted_semantics(&previous.events),
            "migration changed the legacy output oracle"
        );
    }
    let receipts: Vec<_> = all
        .iter()
        .filter(|event| {
            event.envelope.payload_content_type
                == "application/vnd.p4.llamacpp.release-receipt-v1+json"
        })
        .cloned()
        .collect();
    let observations: Vec<_> = all
        .iter()
        .filter(|event| {
            event.envelope.payload_content_type
                == "application/vnd.p4.llamacpp.batch-observation-v4+json"
        })
        .cloned()
        .collect();
    let spans: Vec<_> = all
        .iter()
        .filter(|event| {
            event.envelope.payload_content_type == "application/vnd.p4.llamacpp.stage-span-v4+json"
        })
        .cloned()
        .collect();
    // Capture is observable output only; it never skips parity assertions. The
    // first migration run is intentionally RED until reviewed bytes are checked in.
    if std::env::var_os("P4_CAPTURE_APPROVED_OUTPUT").is_some() {
        let encoded = |events: &[Event]| {
            events.iter().map(|event| {
            let bytes = p4_protocol::event::encode(event).unwrap();
            json!({"wire_hex": bytes.iter().map(|byte| format!("{byte:02X}")).collect::<String>()})
        }).collect::<Vec<_>>()
        };
        eprintln!(
            "P4_APPROVED_OUTPUT_CAPTURE {}",
            json!({"id":case_id,"stage_count":stage_count,"submissions":encoded(submissions),"events":encoded(live),"receipts":encoded(&receipts),"observations":encoded(&observations),"spans":encoded(&spans)})
        );
    }
    let expected = cases()
        .into_iter()
        .find(|case| case.id == case_id)
        .unwrap_or_else(|| panic!("reviewed v3 fixture missing {case_id}"));
    assert_eq!(
        submissions, &expected.submissions,
        "original PREFILL bytes/identity changed"
    );
    for submission in submissions {
        let route = submission.envelope.return_route.as_ref().unwrap();
        assert_eq!(
            submission.envelope.event_id,
            format!(
                "outer:{}:{}:{}:{}",
                route.ingress_agent,
                route.channel,
                route.connection_generation,
                submission.envelope.sequence
            )
        );
    }
    let mut ids = HashSet::new();
    let mut sequences = HashMap::new();
    let mut request_positions = HashMap::new();
    for event in live {
        event.validate().unwrap();
        assert!(
            ids.insert(&event.envelope.event_id),
            "duplicate live output ID"
        );
        assert!(
            event
                .envelope
                .causation_id
                .as_ref()
                .is_some_and(|id| !id.is_empty()),
            "head output must name the causal terminal event"
        );
        if let Some(previous) = sequences.insert(&event.envelope.source, event.envelope.sequence) {
            assert!(
                event.envelope.sequence > previous,
                "live source sequence regressed"
            );
        }
        let body: Value = serde_json::from_slice(&event.payload).unwrap();
        let request = body["request_id"].as_str().unwrap().to_owned();
        let position = body["position"].as_u64().unwrap();
        if let Some(previous) = request_positions.insert(request, position) {
            assert_eq!(
                position,
                previous + 1,
                "live request output order regressed"
            );
        }
    }
    assert_eq!(
        sorted_semantics(live),
        sorted_semantics(&expected.events),
        "current actual producer no longer matches the shared OUTER contract ({case_id})"
    );
    let sorted_receipts = |values: &[Event]| {
        let mut values: Vec<_> = values.iter().map(semantic_projection).collect();
        values.sort_by_key(Value::to_string);
        values
    };
    assert_eq!(
        sorted_receipts(&receipts),
        sorted_receipts(&expected.receipts),
        "live release receipts changed"
    );
    let telemetry = |events: &[Event]| {
        let mut values: Vec<_> = events.iter().map(semantic_projection).collect();
        for value in &mut values {
            let body = value["outcome"].as_object_mut().unwrap();
            // Real timestamps/pacing remain in the raw capture, not in a
            // cross-run golden. Membership, counters and identities stay exact.
            for field in [
                "stage_ms",
                "idle_ms",
                "idle_gated",
                "ready_rows",
                "ready_sequences",
                // Scheduling diagnostics are checked against the independently
                // captured pre-native state by loop_tests/observation_contract.
                // The historical ownership golden remains unchanged.
                "scheduling",
                "ingress_unix_ms",
                "start_unix_ms",
                "end_unix_ms",
                "forward_unix_ms",
            ] {
                body.remove(field);
            }
        }
        values.sort_by_key(Value::to_string);
        values
    };
    assert_eq!(
        telemetry(&observations),
        telemetry(&expected.observations),
        "live owned observations changed"
    );
    assert_eq!(
        telemetry(&spans),
        telemetry(&expected.spans),
        "live owned stage spans changed"
    );
}

/// Independent input-workload declarations, not derived from OUTPUT positions
/// or from the producer's observation accounting implementation.
fn prefill_workload(case_id: &str) -> &'static [(&'static str, usize)] {
    match case_id {
        "ordinary-2" => &[("one", 7)],
        "checkpoint-2" | "checkpoint-4" => &[("partial", 3), ("fence-probe", 1)],
        "mixed-owner-2" | "mixed-owner-4" | "mixed-consumer-2" | "mixed-consumer-4" => {
            &[("release-owner-a", 1), ("release-owner-b", 1)]
        }
        other => panic!("no independent prefill workload for {other}"),
    }
}

pub fn expected_prefill_rows(case_id: &str, request: &str) -> usize {
    prefill_workload(case_id)
        .iter()
        .find_map(|(id, rows)| (*id == request).then_some(*rows))
        .unwrap_or_else(|| panic!("unexpected request {request} in {case_id}"))
}

/// Consume all events received by the actual worker pump. A retransmitted exact
/// observation is harmless, but a physical execution cannot earn rows again in
/// a differently named observation. This checks the live producer, not merely a
/// synthetic observation supplied to the OUTER consumer's fixture replay.
pub fn assert_live_prefill_counts(case_id: &str, live: &[Event]) {
    const OBSERVATION: &str = "application/vnd.p4.llamacpp.batch-observation-v4+json";
    let expected = cases().into_iter().find(|case| case.id == case_id).unwrap();
    let head = &expected.events[0].envelope.source;
    let mut observations = HashMap::<String, Value>::new();
    let mut physical_ids = HashSet::new();
    let mut totals = HashMap::<String, usize>::new();
    for event in live
        .iter()
        .filter(|event| event.envelope.payload_content_type == OBSERVATION)
    {
        event.validate().unwrap();
        assert_eq!(&event.envelope.source, head, "observation is not from head");
        let body: Value = serde_json::from_slice(&event.payload).unwrap();
        assert_eq!(body["load_generation"].as_u64(), Some(1));
        assert_eq!(body["session_id"].as_str(), Some("loop-session"));
        let observation_id = body["observation_id"].as_str().unwrap();
        assert!(!observation_id.is_empty(), "empty live observation ID");
        let observation_key = format!("{:?}:{observation_id}", event.envelope.target);
        if let Some(previous) = observations.get(&observation_key) {
            assert_eq!(previous, &body, "live observation ID changed its payload");
            continue;
        }
        for physical in body["physical_batches"].as_array().unwrap() {
            let execution = physical["execution_id"].as_u64().unwrap();
            assert!(
                physical_ids.insert((format!("{:?}", event.envelope.target), execution)),
                "physical execution {execution} appeared in different observations"
            );
            let mut members = HashSet::new();
            let mut physical_prefill = 0usize;
            for member in physical["owned_requests"].as_array().unwrap() {
                let request = member["request_id"].as_str().unwrap();
                assert!(
                    members.insert(request),
                    "physical request membership repeats"
                );
                // Reject unknown work even when it reports zero prefill rows.
                expected_prefill_rows(case_id, request);
                let rows = usize::try_from(member["prefill_rows"].as_u64().unwrap()).unwrap();
                physical_prefill = physical_prefill.checked_add(rows).unwrap();
                let total = totals.entry(request.to_owned()).or_default();
                *total = total.checked_add(rows).unwrap();
            }
            assert!(
                physical_prefill as u64 <= physical["prefill_rows"].as_u64().unwrap(),
                "owned prefill rows exceed global physical accounting"
            );
        }
        observations.insert(observation_key, body);
    }
    for &(request, rows) in prefill_workload(case_id) {
        assert_eq!(
            totals.get(request).copied(),
            Some(rows),
            "actual producer prefill rows differ from independent workload ({case_id}/{request})"
        );
    }
}
