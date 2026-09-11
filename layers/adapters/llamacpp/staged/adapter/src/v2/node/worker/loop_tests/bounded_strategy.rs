//! Actual Worker::run/Frame/settlement consumers; fake native, no GPU claim.
use super::*;
use crate::v2::scheduler::OrdinaryLimits;

fn pacing_harness(prompt_rows: usize, observer: Option<IssueObserver>) -> (Harness, Vec<InferenceCommand>) {
    let commands: Vec<_> = (0..8).map(|i| request(&format!("pacing-{i}"), prompt_rows, 6)).collect();
    let submissions: Vec<_> = commands.iter().enumerate()
        .map(|(i, c)| submission_event(c, i as u64 + 1, default_route())).collect();
    let mut h = Harness::observed_with_pacing(8, 4, 1, &submissions, 0, None, observer, None,
        OrdinaryLimits::default(), Some(crate::v2::scheduler::pipeline::PipelinePolicy {
            mixed_prefill_rows: 1,
        }), 4);
    h.hold_tail = true;
    (h, commands)
}

#[test]
fn phase_pacing_actual_loop_issues_the_last_full_prefill_without_four_requests() {
    let (mut h, commands) = pacing_harness(12, None);
    h.until("all four full-width prefills before any tail return", |h|
        h.held_tail.len() == 4);
    h.pump_for(Duration::from_millis(25));
    {
        let native = h.nodes[0].native.lock().unwrap();
        assert_eq!(native.logical_calls, 4, "pacing must not bypass the open-batch window");
        let mut members = std::collections::BTreeSet::new();
        for call in &native.issued_native {
            let result = CapsuleSet::decode(call.result.as_ref().unwrap()).unwrap();
            let rows: Vec<_> = result.0.iter().flat_map(|c| &c.owners).collect();
            assert_eq!(rows.len(), BATCH_CAPACITY);
            assert!(rows.iter().all(|r| r.phase == Phase::Prefill));
            let selected: std::collections::BTreeSet<_> = rows.iter().map(|r| r.sequence_id).collect();
            assert_eq!(selected.len(), 2);
            for id in selected { assert!(members.insert(id)); }
        }
        assert_eq!(members.len(), 8);
    }
    assert!(h.outputs.is_empty());
    h.resume_tail();
    h.finish(&commands);
}

#[test]
fn phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input() {
    let refusal = Arc::new(Mutex::new((None::<String>, false)));
    let captured = Arc::clone(&refusal);
    let observer: IssueObserver = Arc::new(move |point, state| {
        if state.next_open_batch != 5 || !matches!(point, "decode_coalescing_wait" | "before_native_issue") {
            return;
        }
        let requests: Vec<_> = state.requests.iter().map(|(key, r)|
            (key, r.prompt_cursor, r.prompt_issued, r.generated, r.outstanding,
                r.incarnation, r.sequence_id, r.ready.as_ref().map(|v|
                    (v.phase, v.position, v.tokens.clone())))).collect();
        let authority = format!("{:?}", (state.next_event, state.next_incarnation,
            requests, &state.flights, &state.open_batches, &state.free_sequences,
            state.request_budget.used()));
        let mut captured = captured.lock().unwrap();
        if point == "decode_coalescing_wait" {
            assert!(state.prepared_issue.is_none(), "waiting must precede native preparation");
            if let Some(before) = &captured.0 { assert_eq!(before, &authority); }
            else { captured.0 = Some(authority); }
        } else if let Some(before) = &captured.0 {
            assert_eq!(before, &authority, "timer wait must preserve request/flight/slot/input authority");
            captured.1 = true;
        }
    });
    let (mut h, commands) = pacing_harness(2, Some(observer));
    h.until("four prompt results retained at the tail", |h| h.held_tail.len() == 4);
    // Exactly two requests become ready; the other six stay in flight. No
    // further input is sent to the head until its timer issues this decode.
    let first = h.held_tail.pop_front().unwrap();
    h.pending.push_back(first);
    h.until("decode deadline must wake the actual blocking worker", |h|
        h.nodes[0].native.lock().unwrap().logical_calls == 5);
    assert!(refusal.lock().unwrap().1, "must observe refusal followed by unchanged authority");
    {
        let native = h.nodes[0].native.lock().unwrap();
        let last = CapsuleSet::decode(native.issued_native.last().unwrap().result.as_ref().unwrap()).unwrap();
        let rows: Vec<_> = last.0.iter().flat_map(|c| &c.owners).collect();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.phase == Phase::Decode && r.position == 2));
    }
    h.pump_for(Duration::from_millis(25));
    assert_eq!(h.nodes[0].native.lock().unwrap().logical_calls, 5,
        "deadline must not reissue an outstanding decode or bypass the full window");
    h.resume_tail();
    h.finish(&commands);
}

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
        // logical_calls counts call entry; the fourth result can still be
        // computing after that counter becomes four. Inspect completed data.
        h.nodes[0].native.lock().unwrap().issued_native.len() >= 4);
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
