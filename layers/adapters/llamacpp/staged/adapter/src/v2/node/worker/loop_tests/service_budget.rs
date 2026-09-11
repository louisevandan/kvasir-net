//! Real workers, native Frame boundaries, feedback routing and settlement.
use super::*;
use crate::v2::{ServiceSample, ServiceVerdict};

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
            mixed_prefill_rows: 1,
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
        assert!(
            sample.shape.prefill_rows > 0,
            "decode-only calls need no prefill feedback event"
        );
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
    let observations: Vec<BatchObservation> = h
        .received
        .iter()
        .filter(|e| e.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE)
        .map(|e| serde_json::from_slice(&e.payload).unwrap())
        .collect();
    let mut deferred = 0;
    let mut probes = 0;
    let mut pure_full = 0;
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
                assert!(prefill > 0);
            }
            ServiceVerdict::PurePrefill => pure_full += usize::from(prefill == BATCH_CAPACITY),
            _ => {}
        }
    }
    assert!(
        deferred > 0 && probes > 0 && pure_full > 0,
        "must exercise actual bounded mixed admission and pure throughput: deferred={deferred}, probes={probes}, pure={pure_full}"
    );
}
