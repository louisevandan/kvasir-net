//! Actual Worker::run/Frame/settlement consumers; fake native, no GPU claim.
use super::*;
use crate::v2::scheduler::OrdinaryLimits;

#[test]
fn pipeline_policy_actual_loop_fills_independent_prefills_and_bounds_mixed_work() {
    let commands: Vec<_> = (0..8).map(|i| request(&format!("pipeline-{i}"),
        if i == 7 { 96 } else { 12 }, 12)).collect();
    let submissions: Vec<_> = commands.iter().enumerate()
        .map(|(i, c)| submission_event(c, i as u64 + 1, default_route())).collect();
    let mut h = Harness::observed_with_pipeline(8, 4, 32, &submissions, 0, None, None, None,
        OrdinaryLimits::default(), Some(crate::v2::scheduler::pipeline::PipelinePolicy {
            mixed_prefill_rows: 1,
        }));
    h.hold_tail = true;
    h.until("independent pipeline prefill before the first return", |h|
        h.nodes[0].native.lock().unwrap().logical_calls >= 4);
    {
        let native = h.nodes[0].native.lock().unwrap();
        assert_eq!(native.logical_calls, 4);
        let mut owners = std::collections::BTreeSet::new();
        for call in &native.issued_native {
            let capsules = CapsuleSet::decode(call.result.as_ref().unwrap()).unwrap();
            assert_eq!(capsules.0.iter().map(|c| c.owners.len()).sum::<usize>(), BATCH_CAPACITY);
            let group: std::collections::BTreeSet<_> = capsules.0.iter()
                .flat_map(|c| c.owners.iter().map(|r| r.sequence_id)).collect();
            assert_eq!(group.len(), 2);
            for id in group { assert!(owners.insert(id), "same request was issued twice before return"); }
        }
        assert_eq!(owners.len(), 8);
    }
    assert!(h.outputs.is_empty());
    h.resume_tail();
    h.finish(&commands);
    let mut mixed = 0;
    let mut pure = 0;
    for event in h.received.iter().filter(|e| e.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE) {
        let o: BatchObservation = serde_json::from_slice(&event.payload).unwrap();
        let policy = o.scheduling.unwrap().pipeline.expect("effective pipeline policy must be visible");
        let prefill: usize = o.physical_batches.iter().map(|p| p.prefill_rows).sum();
        if policy.decoding_active { assert!(prefill <= 1); mixed += usize::from(prefill > 0); }
        else if prefill == BATCH_CAPACITY { pure += 1; }
        assert!(policy.open < policy.window);
    }
    assert!(mixed > 0 && pure >= 4);
}

#[test]
fn request_storage_actual_loop_retires_after_completion_and_slot_reuse() {
    use crate::v2::node::request_budget::{RequestBudget, RequestCost};
    let observed = Arc::new(Mutex::new((None::<RequestBudget>, 0usize)));
    let copied = Arc::clone(&observed);
    let observer: IssueObserver = Arc::new(move |_, state| {
        let mut view = copied.lock().unwrap();
        view.0 = Some(state.request_budget.clone());
        view.1 = view.1.max(state.request_budget.used().requests);
    });
    let commands: Vec<_> = (0..24).map(|i| request(&format!("storage-{i}"), 12, 6)).collect();
    let submissions: Vec<_> = commands.iter().enumerate()
        .map(|(i, c)| submission_event(c, i as u64 + 1, default_route())).collect();
    let mut h = Harness::observed_events(3, 4, 1, &submissions, 0, None, Some(observer), None);
    h.finish(&commands);
    let view = observed.lock().unwrap();
    assert!(view.1 > SEQUENCE_CAPACITY as usize, "pending inputs must also be charged");
    assert_eq!(view.0.as_ref().unwrap().used(), RequestCost::default(),
        "real completion/release must retire inputs without ending the worker");
}

#[test]
fn bounded_strategy_actual_loop_keeps_independent_work_open_and_releases_every_slot() {
    let commands: Vec<_> = (0..8)
        .map(|i| request(&format!("bounded-{i}"), 12, 6))
        .collect();
    let submissions: Vec<_> = commands
        .iter()
        .enumerate()
        .map(|(i, c)| submission_event(c, i as u64 + 1, default_route()))
        .collect();
    let limits = OrdinaryLimits {
        decode_members: 1,
        prefill_members: 2,
        prefill_rows: 2,
        prefill_rows_per_request: 1,
    };
    let mut h = Harness::observed_with_limits(3, 4, 32, &submissions, 0, None, None, None, limits);
    h.hold_tail = true;
    h.until("four independent issues before any tail return", |h| {
        h.nodes[0].native.lock().unwrap().logical_calls >= 4
    });
    assert!(h.outputs.is_empty());
    h.resume_tail();
    h.finish(&commands);
    let observations: Vec<BatchObservation> = h
        .received
        .iter()
        .filter(|e| e.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE)
        .map(|e| serde_json::from_slice(&e.payload).unwrap())
        .collect();
    assert!(!observations.is_empty());
    let mut saw_decode = false;
    let mut saw_blocked = false;
    for o in observations {
        let s = o
            .scheduling
            .expect("actual producer must bind its pre-issue snapshot");
        assert_eq!(s.ordinary_limits, limits);
        assert!(s.ordinary_limits_applied);
        assert_eq!((s.max_open_batches, s.prefill_fragments), (4, 1));
        assert!(s.open_batches_before_issue < 4);
        saw_blocked |= s.blocked_outstanding > 0;
        let decode: usize = o.physical_batches.iter().map(|b| b.decode_rows).sum();
        let prefill: usize = o.physical_batches.iter().map(|b| b.prefill_rows).sum();
        assert!(decode <= 1 && prefill <= 2);
        saw_decode |= decode > 0;
        for b in o.physical_batches {
            assert!(b.owned_requests.iter().all(|r| r.prefill_rows <= 1));
        }
    }
    assert!(saw_decode && saw_blocked);
}
