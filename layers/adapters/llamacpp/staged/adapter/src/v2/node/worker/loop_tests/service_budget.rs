//! Real workers, native Frame boundaries, feedback routing and settlement.
use super::*;
use crate::v2::{ServiceSample, ServiceVerdict};

#[test]
fn service_calibration_actual_loop_issues_a_small_probe_before_a_large_prompt_returns() {
    let commands: Vec<_> = (0..8)
        .map(|i| request(&format!("calibration-{i}"), if i < 2 { 2 } else { 96 }, 8))
        .collect();
    let submissions: Vec<_> = commands
        .iter()
        .enumerate()
        .map(|(i, c)| submission_event(c, i as u64 + 1, default_route()))
        .collect();
    let mut h = Harness::observed_with_service(
        8,
        4,
        1,
        &submissions,
        0,
        None,
        None,
        None,
        Default::default(),
        Some(crate::v2::scheduler::pipeline::PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 4,
        }),
        0,
        Some(1),
    );
    h.hold_tail = true;
    h.until(
        "a cold one-row calibration may follow the first full prompt before either returns",
        |h| h.held_tail.len() >= 2,
    );
    {
        let native = h.nodes[0].native.lock().unwrap();
        let first = CapsuleSet::decode(native.issued_native[0].result.as_ref().unwrap()).unwrap();
        assert_eq!(
            first.0.iter().flat_map(|c| &c.owners).count(),
            BATCH_CAPACITY
        );
        let second = CapsuleSet::decode(native.issued_native[1].result.as_ref().unwrap()).unwrap();
        let rows: Vec<_> = second.0.iter().flat_map(|c| &c.owners).collect();
        assert_eq!(
            rows.len(),
            1,
            "unknown service cannot admit another full prefill"
        );
        assert!(rows.iter().all(|r| r.phase == Phase::Prefill));
        assert!(
            native.logical_calls <= 4,
            "calibration cannot enlarge the flight window"
        );
    }
    assert!(
        h.outputs.is_empty(),
        "no tail result was returned to authorize generation"
    );
    h.resume_tail();
    h.finish(&commands);
}

#[test]
fn service_budget_actual_loop_learns_all_stages_defers_prefill_and_finishes_every_request() {
    let commands: Vec<_> = (0..8)
        .map(|i| request(&format!("service-{i}"), if i < 4 { 8 } else { 96 }, 48))
        .collect();
    let submissions: Vec<_> = commands
        .iter()
        .enumerate()
        .map(|(i, c)| submission_event(c, i as u64 + 1, default_route()))
        .collect();
    let mut h = Harness::observed_with_service(
        3,
        4,
        1,
        &submissions,
        0,
        None,
        None,
        None,
        Default::default(),
        Some(crate::v2::scheduler::pipeline::PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 4,
        }),
        0,
        Some(1),
    );
    h.finish(&commands);
    let mut stages = std::collections::BTreeSet::new();
    let mut samples = std::collections::BTreeMap::new();
    for event in h
        .stage_events
        .iter()
        .filter(|e| e.envelope.payload_content_type == SERVICE_SAMPLE_CONTENT_TYPE)
    {
        let sample: ServiceSample = serde_json::from_slice(&event.payload).unwrap();
        assert_eq!(event.envelope.target, endpoint(0));
        assert_eq!(event.envelope.source, endpoint(sample.stage_index));
        assert!(sample.stage_index > 0 && sample.stage_index < 3);
        let key = (sample.execution_ids.clone(), sample.stage_index);
        assert!(
            samples.insert(key, sample.clone()).is_none(),
            "replayed input is not fresh compute"
        );
        stages.insert(sample.stage_index);
    }
    assert_eq!(stages, std::collections::BTreeSet::from([1, 2]));
    assert!(
        samples
            .values()
            .any(|s| s.shape.prefill_rows == 0 && s.shape.decode_rows > 0),
        "actual downstream decode service must reach the predictor"
    );
    let observations: Vec<BatchObservation> = h
        .received
        .iter()
        .filter(|e| e.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE)
        .map(|e| serde_json::from_slice(&e.payload).unwrap())
        .collect();
    let mut deferred = 0;
    let mut probes = 0;
    let mut pure_full = 0;
    let mut ready_multi = 0;
    let mut rechunked = 0;
    for observation in &observations {
        let decision = observation
            .scheduling
            .as_ref()
            .unwrap()
            .service_budget
            .as_ref()
            .unwrap();
        let prefill: usize = observation
            .physical_batches
            .iter()
            .map(|b| b.prefill_rows)
            .sum();
        let decode: usize = observation
            .physical_batches
            .iter()
            .map(|b| b.decode_rows)
            .sum();
        let eligible = observation.scheduling.as_ref().unwrap().eligible_decode;
        assert_eq!(
            decision.selected_prefill_rows,
            Some(prefill),
            "chosen service plan must match the actual native batch"
        );
        if decision.examined_prefill_rows.len() > 1 {
            rechunked += 1;
            assert!(
                decision
                    .examined_prefill_rows
                    .windows(2)
                    .all(|p| p[1] < p[0])
            );
            assert!(
                prefill <= 1,
                "the deliberately infeasible budget must calibrate only one prompt row"
            );
        }
        if eligible > 1 {
            ready_multi += 1;
            let limit = observation
                .scheduling
                .as_ref()
                .unwrap()
                .pipeline
                .unwrap()
                .effective_limits
                .decode_members;
            assert_eq!(
                decode,
                eligible.min(limit).min(BATCH_CAPACITY - 1),
                "generation must fill its independent cohort before prefill"
            );
        }
        match decision.verdict {
            ServiceVerdict::DeferPrefill => {
                deferred += 1;
                assert_eq!(
                    prefill, 0,
                    "actual native work must omit over-budget prefill"
                );
                assert!(decode > 0, "service gate must keep ready decode runnable");
                assert_eq!(decision.known_stages, 3);
                assert!(decision.max_with_candidate_us > decision.budget_us);
            }
            ServiceVerdict::ProgressProbe => {
                probes += 1;
                assert_eq!(
                    prefill, 1,
                    "progress cannot re-admit the rejected original quantum"
                );
            }
            ServiceVerdict::PurePrefill => pure_full += usize::from(prefill == BATCH_CAPACITY),
            _ => {}
        }
    }
    assert!(
        deferred > 0 && probes > 0 && pure_full > 0,
        "must exercise actual bounded mixed admission and pure throughput: deferred={deferred}, probes={probes}, pure={pure_full}"
    );
    assert!(
        ready_multi > 0,
        "must exercise ready generation competing with prefill"
    );
    assert!(
        rechunked > 0,
        "must shrink a real prepared candidate, not only reject prefill"
    );
}
