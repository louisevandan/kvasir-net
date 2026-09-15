//! Compare the timer interval separately from a genuine RELEASE/receipt publication.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Authority {
    next_event: u64,
    free_sequences: Vec<u32>,
    stable: String,
}

fn authority(state: &AdapterState) -> Authority {
    let requests: Vec<_> = state
        .requests
        .iter()
        .map(|(key, r)| {
            (
                key,
                r.prompt_cursor,
                r.prompt_issued,
                r.generated,
                r.outstanding,
                r.incarnation,
                r.sequence_id,
                r.ready
                    .as_ref()
                    .map(|v| (v.phase, v.position, v.tokens.clone())),
            )
        })
        .collect();
    Authority {
        next_event: state.next_event,
        free_sequences: state.free_sequences.iter().copied().collect(),
        stable: format!(
            "{:?}",
            (
                state.next_open_batch,
                state.next_incarnation,
                requests,
                &state.flights,
                &state.open_batches,
                state.request_budget.used()
            )
        ),
    }
}

#[derive(Default)]
struct Observation {
    before: Option<Authority>,
    issued: bool,
    peer_slot: Option<u32>,
    release_commits: usize,
}

pub(super) fn assert_timer_authority(interleave_release: bool) {
    let observation = Arc::new(Mutex::new(Observation::default()));
    let waiting = Arc::new(AtomicBool::new(false));
    let ack_queued = Arc::new(AtomicBool::new(false));
    struct ResumeOnDrop(Arc<AtomicBool>);
    impl Drop for ResumeOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let resume_on_drop = ResumeOnDrop(Arc::clone(&ack_queued));
    let captured = Arc::clone(&observation);
    let captured_waiting = Arc::clone(&waiting);
    let captured_ack = Arc::clone(&ack_queued);
    let observer: IssueObserver = Arc::new(move |point, state| {
        let mut seen = captured.lock().unwrap();
        if seen.issued {
            return;
        }
        if let Some(peer) = state
            .requests
            .values()
            .find(|r| r.command.request_id == "timer-1")
        {
            seen.peer_slot = peer.sequence_id;
        }
        if point == "after_release_committed" && seen.before.is_some() {
            assert!(interleave_release);
            assert_eq!(
                seen.release_commits, 0,
                "only the completed peer may release during this wait"
            );
            let mut expected = seen.before.clone().unwrap();
            expected.free_sequences.push(seen.peer_slot.unwrap());
            assert_eq!(
                authority(state),
                expected,
                "RELEASE only returns its proven peer slot"
            );
            // The committed release owes exactly one OUTER receipt before the next issue.
            expected.next_event += 1;
            seen.before = Some(expected);
            seen.release_commits += 1;
        } else if point == "decode_coalescing_wait" {
            assert!(state.prepared_issue.is_none());
            let now = authority(state);
            if let Some(before) = &seen.before {
                assert_eq!(before, &now);
            } else {
                seen.before = Some(now);
            }
            captured_waiting.store(true, Ordering::Release);
        } else if point == "before_native_issue" && seen.before.is_some() {
            assert_eq!(
                seen.before.as_ref().unwrap(),
                &authority(state),
                "timer wait must preserve request/flight/slot/input authority"
            );
            assert_eq!(seen.release_commits, usize::from(interleave_release));
            seen.issued = true;
        }
        drop(seen);
        if point == "decode_coalescing_wait" && interleave_release {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !captured_ack.load(Ordering::Acquire) {
                assert!(
                    Instant::now() < deadline,
                    "genuine release was not delivered"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });
    let commands: Vec<_> = (0..8)
        .map(|i| request(&format!("timer-{i}"), 2, if i == 0 { 7 } else { 6 }))
        .collect();
    let submissions: Vec<_> = commands
        .iter()
        .enumerate()
        .map(|(i, c)| submission_event(c, i as u64 + 1, default_route()))
        .collect();
    let mut h = Harness::observed_with_pacing(
        8,
        4,
        1,
        &submissions,
        0,
        None,
        Some(observer),
        None,
        OrdinaryLimits::default(),
        Some(crate::v2::scheduler::pipeline::PipelinePolicy {
            mixed_batch_rows: None,
            mixed_prefill_rows: 1,
        }),
        4,
    );
    // Drop the unblock guard before Harness joins workers on an assertion failure.
    let _resume_on_drop = resume_on_drop;
    h.hold_tail = true;
    h.hold_control = Some((RELEASED_CONTENT_TYPE.into(), 0));
    h.until("all initial prompt physical results", |h| {
        h.held_tail
            .iter()
            .map(|e| {
                CapsuleSet::decode(&e.payload)
                    .unwrap()
                    .0
                    .iter()
                    .map(|c| c.owners.len())
                    .sum::<usize>()
            })
            .sum::<usize>()
            == 16
    });
    h.pending.extend(h.held_tail.drain(..));
    h.until("four full generation cohorts", |h| h.held_tail.len() == 4);
    for _ in 0..6 {
        if observation.lock().unwrap().issued {
            break;
        }
        let index = h
            .held_tail
            .iter()
            .position(|e| {
                CapsuleSet::decode(&e.payload)
                    .unwrap()
                    .0
                    .iter()
                    .flat_map(|c| &c.owners)
                    .any(|r| r.request_id == "timer-0")
            })
            .unwrap();
        h.pending.push_back(h.held_tail.remove(index).unwrap());
        h.until(
            "returned cohort progresses or enters the controlled wait",
            |h| h.held_tail.len() == 4 || (interleave_release && waiting.load(Ordering::Acquire)),
        );
        if interleave_release && waiting.load(Ordering::Acquire) {
            h.until("genuine tail RELEASED is retained", |h| {
                !h.held_control.is_empty()
            });
            assert_eq!(h.held_control.len(), 1);
            let release: ReleaseCommand =
                serde_json::from_slice(&h.held_control[0].payload).unwrap();
            assert_eq!(release.sequences.len(), 1);
            assert_eq!(
                release.sequences[0].id,
                observation.lock().unwrap().peer_slot.unwrap()
            );
            h.pending.extend(h.held_control.drain(..));
            h.tick();
            assert!(
                h.pending.is_empty(),
                "release must enter the actual bounded worker input"
            );
            ack_queued.store(true, Ordering::Release);
            h.until("deadline resumes after the real release", |h| {
                h.held_tail.len() == 4
            });
        }
    }
    let seen = observation.lock().unwrap();
    assert!(
        seen.issued,
        "must observe refusal followed by the exact authorized state"
    );
    assert_eq!(seen.release_commits, usize::from(interleave_release));
    drop(seen);
    let calls;
    {
        let native = h.nodes[0].native.lock().unwrap();
        calls = native.logical_calls;
        let last = CapsuleSet::decode(
            native
                .issued_native
                .last()
                .unwrap()
                .result
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        let rows: Vec<_> = last.0.iter().flat_map(|c| &c.owners).collect();
        assert!(!rows.is_empty() && rows.len() < 4);
        assert!(
            rows.iter()
                .all(|r| r.phase == Phase::Decode && r.position >= 2)
        );
    }
    h.pump_for(Duration::from_millis(25));
    assert_eq!(
        h.nodes[0].native.lock().unwrap().logical_calls,
        calls,
        "deadline must not reissue an outstanding decode or bypass the full window"
    );
    h.hold_control = None;
    h.pending.extend(h.held_control.drain(..));
    h.resume_tail();
    h.finish(&commands);
    release_notifications::assert_complete(&h);
}
