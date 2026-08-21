use super::model::tps;
use super::{TelemetryCollector, TelemetryEvidence};
use p4_service::status::{
    ActiveHopLane, ActiveHopSnapshot, LaneSnapshot, NodeSnapshot, StatusSnapshot, TrafficSnapshot,
};

fn snapshot(backend: &str, lane: Option<ActiveHopLane>, depth: usize) -> StatusSnapshot {
    StatusSnapshot {
        schema: 6,
        snapshot_seq: 4,
        generated_at_unix_ms: 100,
        address: "tcp://agent:1".into(),
        traffic: TrafficSnapshot {
            forwarded: 0,
            consumed: 0,
            to_nodes: 0,
            unrouted: 0,
            refused: 0,
            emergency_lost: 0,
        },
        lanes: LaneSnapshot {
            control: 0,
            prefill: 0,
            decode: 0,
            response: 0,
        },
        peers: 0,
        continuations: 0,
        subscription_pending: 0,
        subscription_unacked: 0,
        subscription_dropped: 0,
        subscription_ack_rejected: 0,
        nodes: vec![NodeSnapshot {
            node: "n0".into(),
            depth,
            running: 1,
            outbox_lost: 0,
            waiting: vec![],
            backend: backend.into(),
            waiting_requests: vec![],
            active_hop: lane.map(|lane| ActiveHopSnapshot {
                id: 8,
                lane,
                timed_out: false,
                requests: vec![],
            }),
        }],
    }
}

#[test]
fn parses_samples_uses_typed_status_and_deduplicates_retained_reports() {
    let collector = TelemetryCollector::default();
    let report = "P4_RUNTIME_EVIDENCE_V1 retained=2 dropped=0\nP4_RUNTIME_SAMPLE_V1 hop_id=8 phase=prefill sequence_hex=7330 tokens=10 elapsed_us=2000\nP4_RUNTIME_SAMPLE_V1 hop_id=9 phase=generation sequence_hex=7330 tokens=4 elapsed_us=4000";
    collector.observe(&snapshot(report, Some(ActiveHopLane::Prefill), 7));
    collector.observe(&snapshot(report, Some(ActiveHopLane::Decode), 11));
    let evidence = collector.evidence(std::time::Duration::from_millis(20));
    assert_eq!(evidence.samples.len(), 2);
    assert_eq!(evidence.sessions[0].prefill.tps, Some(5_000.0));
    assert_eq!(evidence.sessions[0].generation.tps, Some(1_000.0));
    assert_eq!(evidence.nodes[0].depth, 7);
    assert_eq!(
        evidence.nodes[1].active_phase,
        Some(super::RuntimePhase::Generation)
    );
    let json = evidence.to_json();
    assert!(json.contains("P4_RUNTIME_SAMPLE_V1"));
    assert!(json.contains("\"depth\":11"));
    assert!(json.contains("\"generation\":{\"tokens\":4"));
}

#[test]
fn totals_survive_retained_sample_eviction_for_long_generation() {
    let collector = TelemetryCollector::default();
    let report = "P4_RUNTIME_SAMPLE_V1 hop_id=8 phase=generation sequence_hex=7330 position=511 tokens=1 elapsed_us=1000\nP4_RUNTIME_TOTAL_V1 phase=prefill sequence_hex=7330 tokens=5000 elapsed_us=1000000\nP4_RUNTIME_TOTAL_V1 phase=generation sequence_hex=7330 tokens=5000 elapsed_us=5000000";
    collector.observe(&snapshot(report, None, 0));
    let evidence = collector.evidence(std::time::Duration::from_secs(5));
    assert_eq!(evidence.aggregate.logical_prefill_tokens, 5000);
    assert_eq!(evidence.aggregate.logical_generation_tokens, 5000);
    assert_eq!(
        evidence.sessions[0].logical_generation_compute_tps,
        Some(1000.0)
    );
}

#[test]
fn cumulative_total_updates_replace_older_status_snapshots() {
    let collector = TelemetryCollector::default();
    collector.observe(&snapshot(
        "P4_RUNTIME_TOTAL_V1 phase=generation sequence_hex=7330 tokens=2 elapsed_us=2000",
        None,
        0,
    ));
    collector.observe(&snapshot(
        "P4_RUNTIME_TOTAL_V1 phase=generation sequence_hex=7330 tokens=8 elapsed_us=8000",
        None,
        0,
    ));
    let evidence = collector.evidence(std::time::Duration::from_secs(1));
    assert_eq!(evidence.observed_total_lines, 1);
    assert_eq!(evidence.aggregate.logical_generation_tokens, 8);
    assert_eq!(evidence.aggregate.generation.elapsed_us, 8000);
}

#[test]
fn ignores_mtp_speculative_and_malformed_sample_lines() {
    let collector = TelemetryCollector::default();
    let report = "P4_RUNTIME_SAMPLE_V1 hop_id=1 phase=mtp sequence_hex=7330 tokens=2 elapsed_us=1\nP4_RUNTIME_SAMPLE_V1 hop_id=2 phase=speculative sequence_hex=7330 tokens=2 elapsed_us=1\nP4_RUNTIME_SAMPLE_V1 hop_id=nope phase=prefill sequence_hex=7330 tokens=2 elapsed_us=1";
    collector.observe(&snapshot(report, None, 0));
    assert!(
        collector
            .evidence(std::time::Duration::ZERO)
            .samples
            .is_empty()
    );
}

#[test]
fn zero_elapsed_has_no_tps_instead_of_infinity() {
    assert_eq!(tps(10, 0), None);
}

#[test]
fn logical_metrics_deduplicate_stage_copies_by_sequence_phase_and_position() {
    let state = super::model::State {
        samples: vec![
            super::model::RuntimeSample {
                node: "stage-0".into(),
                hop_id: 1,
                phase: super::RuntimePhase::Prefill,
                sequence: "s0".into(),
                position: 0,
                tokens: 10,
                elapsed_us: 2_000,
            },
            super::model::RuntimeSample {
                node: "stage-1".into(),
                hop_id: 1,
                phase: super::RuntimePhase::Prefill,
                sequence: "s0".into(),
                position: 0,
                tokens: 10,
                elapsed_us: 2_500,
            },
            super::model::RuntimeSample {
                node: "stage-0".into(),
                hop_id: 2,
                phase: super::RuntimePhase::Generation,
                sequence: "s0".into(),
                position: 1,
                tokens: 1,
                elapsed_us: 3_000,
            },
            super::model::RuntimeSample {
                node: "stage-1".into(),
                hop_id: 2,
                phase: super::RuntimePhase::Generation,
                sequence: "s0".into(),
                position: 1,
                tokens: 1,
                elapsed_us: 5_000,
            },
        ],
        ..Default::default()
    };
    let evidence = TelemetryEvidence::from_state(&state, std::time::Duration::from_millis(10));
    let session = &evidence.sessions[0];

    assert_eq!(evidence.aggregate.prefill.tokens, 20);
    assert_eq!(evidence.aggregate.logical_prefill_tokens, 10);
    assert_eq!(evidence.aggregate.logical_generation_tokens, 1);
    assert_eq!(session.logical_prefill_tokens, 10);
    assert_eq!(session.logical_generation_tokens, 1);
    assert_eq!(session.logical_prefill_elapsed_us, 2_500);
    assert_eq!(session.logical_generation_elapsed_us, 5_000);
    assert_eq!(session.logical_prefill_compute_tps, Some(4_000.0));
    assert_eq!(session.logical_generation_compute_tps, Some(200.0));
    assert_eq!(
        evidence.aggregate.logical_prefill_tps_over_run,
        Some(1_000.0)
    );
    assert_eq!(
        evidence.aggregate.logical_generation_tps_over_run,
        Some(100.0)
    );

    let json = evidence.to_json();
    assert!(json.contains("\"logical_prefill_tokens\":10"));
    assert!(json.contains("\"logical_generation_tokens\":1"));
}

#[test]
fn logical_generation_ignores_stage_zero_kv_priming_hop() {
    let state = super::model::State {
        samples: vec![
            super::model::RuntimeSample {
                node: "stage-0".into(),
                hop_id: 1,
                phase: super::RuntimePhase::Generation,
                sequence: "s0".into(),
                position: 0,
                tokens: 1,
                elapsed_us: 100,
            },
            super::model::RuntimeSample {
                node: "stage-0".into(),
                hop_id: 2,
                phase: super::RuntimePhase::Generation,
                sequence: "s0".into(),
                position: 1,
                tokens: 1,
                elapsed_us: 100,
            },
            super::model::RuntimeSample {
                node: "tail-1".into(),
                hop_id: 1,
                phase: super::RuntimePhase::Generation,
                sequence: "s0".into(),
                position: 1,
                tokens: 1,
                elapsed_us: 200,
            },
        ],
        ..Default::default()
    };
    let evidence = TelemetryEvidence::from_state(&state, std::time::Duration::from_millis(10));
    assert_eq!(evidence.aggregate.logical_generation_tokens, 1);
    assert_eq!(evidence.sessions[0].logical_generation_tokens, 1);
}
