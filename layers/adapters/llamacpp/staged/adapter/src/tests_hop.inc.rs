// Hop execution: the tombstone check, the ordering it depends on, and the
// FIFO cap's known limitation.
//
// Split out of `tests.inc.rs` on line count alone; a sibling test module
// rather than more content in the same one so each file stays under 400
// lines without inventing cross-file visibility for tiny fixtures --
// `adapter()`, `sequence()` and `hop_for()` below are the same shapes
// `tests.inc.rs` builds, duplicated rather than exported because moving
// three tiny functions is a smaller cost than a shared-fixture module for
// two call sites.
#[cfg(test)]
mod tests_hop {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<p4_adapter::Event>>);

    impl p4_adapter::EventSink for Recorder {
        fn raise(&self, event: p4_adapter::Event) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn adapter() -> StagedAdapter {
        StagedAdapter::new(
            StagedConfig::new("stage-server", "127.0.0.1:1".parse().unwrap()).with_kv_metadata(
                "model.gguf",
                8,
                16,
            ),
        )
    }

    fn sequence(id: &str, state: Option<Vec<u8>>) -> p4_adapter::Sequence {
        p4_adapter::Sequence {
            sequence: id.into(),
            session_epoch: 0,
            state,
            prompt: None,
            remaining: 2,
            options: "{}".into(),
        }
    }

    fn hop_for(sequences: Vec<p4_adapter::Sequence>) -> p4_adapter::Hop {
        p4_adapter::Hop {
            id: 1,
            deployment: "deployment".into(),
            sequences,
        }
    }

    // `execute_hop` on a never-loaded adapter fails at `lifecycle.request`
    // (`LifecycleError::InvalidState`, surfaced as "HOP request failed") --
    // no real stage server is needed to exercise everything ahead of that
    // call, which is what these tests are about.

    // Round 3 narrows this test's own contract: rejection here is scoped to
    // the exact session (`sequence`, `session_epoch`) pair that was
    // released, not to the sequence id alone. See `tests_session_epoch.
    // inc.rs` for the companion case -- a *different* epoch reusing this
    // same id -- and for why that used to be rejected too, wrongly.
    #[test]
    fn a_released_sequence_that_arrives_again_under_the_same_epoch_is_rejected() {
        let adapter = adapter();
        adapter.sequences.lock().unwrap().release("seq-released", 0);
        // `sequence()` builds `session_epoch: 0`, matching the release above
        // -- a genuine redelivery of the session that already ended here.
        let (returned_sequence, detail) = adapter
            .execute_hop(&hop_for(vec![sequence("seq-released", None)]))
            .unwrap_err();
        assert_eq!(returned_sequence, Some("seq-released".to_owned()));
        assert!(detail.contains("seq-released"));
        assert!(detail.contains("already released"));
        assert_eq!(
            adapter.tombstone_rejections.load(Ordering::Relaxed),
            1,
            "a tombstone rejection must be counted so it does not look like a random failure"
        );
    }

    #[test]
    fn a_sequence_never_released_still_classifies_normally() {
        // Not tombstoned, so this must fail later than the tombstone check --
        // at the (mocked-away) request to a stage server -- not be rejected
        // as already released. A regression here would mean the tombstone
        // check started swallowing ordinary new work.
        let adapter = adapter();
        let (returned_sequence, detail) = adapter
            .execute_hop(&hop_for(vec![sequence("seq-fresh", None)]))
            .unwrap_err();
        assert_eq!(returned_sequence, None);
        assert!(detail.contains("HOP request failed"));
        assert!(!detail.contains("already released"));
        assert_eq!(adapter.tombstone_rejections.load(Ordering::Relaxed), 0);
    }

    /// `reject_released_sequences` has to run before `execute_hop` derives
    /// residency (whether a sequence is beginning work or continuing a
    /// decode lap), not merely "somewhere before the backend request" --
    /// the doc comment on `reject_released_sequences` claims the stronger
    /// ordering, and until this test existed nothing checked it: moving the
    /// call to just after residency derivation still passed every other
    /// test in this crate, because with a single released sequence in the
    /// hop the two orderings produce the same externally visible failure
    /// (neither reaches the backend).
    ///
    /// A two-sequence hop tells them apart. `seq-released` derives as
    /// "beginning work" once it is out of `active` (which `release` always
    /// does), while `seq-active` -- still resident -- derives as
    /// "continuing a decode lap". Residency derivation disagreeing on a
    /// single hop is exactly what the mixed-hop guard right after it exists
    /// to catch. So: checked first, this hop fails as "already released".
    /// Checked after residency derivation, the mixed-hop guard fires first
    /// instead and the released sequence is never named as the problem.
    #[test]
    fn the_released_check_runs_before_residency_derivation_not_after() {
        let adapter = adapter();
        adapter
            .sequences
            .lock()
            .unwrap()
            .active
            .insert("seq-active".to_owned(), 0);
        adapter.sequences.lock().unwrap().release("seq-released", 0);

        let (returned_sequence, detail) = adapter
            .execute_hop(&hop_for(vec![
                sequence("seq-released", None),
                sequence("seq-active", None),
            ]))
            .unwrap_err();

        assert_eq!(
            returned_sequence,
            Some("seq-released".to_owned()),
            "the released check must name the released sequence, not fail generically: {detail}"
        );
        assert!(
            detail.contains("already released"),
            "the released check must fire before residency derivation gets a chance to call \
             this a mixed hop instead: {detail}"
        );
        assert!(
            !detail.contains("mixes"),
            "a mixed-hop failure here means the ordering moved: {detail}"
        );
    }

    #[test]
    fn the_tombstone_set_stays_bounded() {
        let mut ledger = SequenceLedger::new();
        for index in 0..RELEASED_TOMBSTONE_CAP + 10 {
            ledger.release(&format!("seq-{index}"), 0);
        }
        assert_eq!(ledger.released.len(), RELEASED_TOMBSTONE_CAP);
        assert_eq!(ledger.released_order.len(), RELEASED_TOMBSTONE_CAP);
        // The oldest entries were evicted first: the first ten sequences
        // released must be gone, and the most recent one must still be a
        // tombstone.
        for index in 0..10 {
            assert!(!ledger.released.contains(&(format!("seq-{index}"), 0)));
        }
        assert!(
            ledger
                .released
                .contains(&(format!("seq-{}", RELEASED_TOMBSTONE_CAP + 9), 0))
        );
    }

    #[test]
    fn release_reports_whether_it_evicted_the_oldest_tombstone() {
        // The return value is what lets a caller count evictions
        // (`execute_hop`'s ledger-update block does exactly that). Pinned on
        // its own because the FIFO-cap test above only checks the resulting
        // set membership, not what `release` itself reported along the way.
        let mut ledger = SequenceLedger::new();
        for index in 0..RELEASED_TOMBSTONE_CAP {
            assert!(
                !ledger.release(&format!("seq-{index}"), 0),
                "the cap is not reached until the {RELEASED_TOMBSTONE_CAP}th release"
            );
        }
        assert!(
            ledger.release("seq-over-cap", 0),
            "the release that pushes past the cap must report the eviction it caused"
        );
    }

    /// Once a released sequence's tombstone has fallen out of the
    /// FIFO-capped `released` set, a redelivered hop for it is no longer
    /// distinguishable from brand-new work. This is `RELEASED_TOMBSTONE_CAP`'s
    /// documented, accepted limitation (see its doc comment in
    /// `config.inc.rs`), not a bug this test is reporting: the tombstone is
    /// a detector with a bounded memory, not a guarantee, and past eviction
    /// this adapter knowingly returns to its pre-fix behaviour -- silent
    /// reclassification as a fresh Prefill -- rather than to something
    /// worse. This test pins that known limitation so a future change
    /// cannot silently turn "bounded memory" into "no memory" or, in the
    /// other direction, into an unbounded set.
    #[test]
    fn past_the_tombstone_cap_a_released_sequence_is_knowingly_reclassified_as_fresh() {
        let adapter = adapter();
        {
            let mut ledger = adapter.sequences.lock().unwrap();
            // Release the sequence under test first, then push it out of the
            // FIFO by releasing enough distinct sequences after it to fill
            // the cap.
            ledger.release("seq-evicted", 0);
            for index in 0..RELEASED_TOMBSTONE_CAP {
                ledger.release(&format!("seq-filler-{index}"), 0);
            }
        }
        assert!(
            !adapter
                .sequences
                .lock()
                .unwrap()
                .released
                .contains(&("seq-evicted".to_owned(), 0)),
            "the filler releases must have pushed the sequence under test out of the cap"
        );

        // A hop naming the evicted sequence must now reach the ordinary
        // "no real stage server" failure, not "already released": the
        // tombstone that used to catch it is gone, so it is read as new work.
        let (returned_sequence, detail) = adapter
            .execute_hop(&hop_for(vec![sequence("seq-evicted", None)]))
            .unwrap_err();
        assert_eq!(returned_sequence, None);
        assert!(
            detail.contains("HOP request failed"),
            "an evicted sequence must be classified as ordinary new work, not as: {detail}"
        );
        assert!(
            !detail.contains("already released"),
            "eviction must not still be caught by the tombstone: {detail}"
        );
        assert_eq!(
            adapter.tombstone_rejections.load(Ordering::Relaxed),
            0,
            "eviction means no tombstone rejection fires for this sequence any more"
        );
    }

    #[test]
    fn report_exposes_both_tombstone_counters() {
        // `report()` is the only place an operator (or the fleet driver)
        // reads either counter; a wiring mistake here means a real eviction
        // problem looks like silence.
        let adapter = adapter();
        adapter.tombstone_rejections.fetch_add(2, Ordering::Relaxed);
        adapter.tombstone_evictions.fetch_add(5, Ordering::Relaxed);
        let report = adapter.report();
        assert!(
            report.contains("P4_STAGED_TOMBSTONE_REJECTED_V1 count=2"),
            "{report}"
        );
        assert!(
            report.contains("P4_STAGED_TOMBSTONE_EVICTED_V1 count=5"),
            "{report}"
        );
    }

    #[test]
    fn clearing_the_sequence_ledger_forgets_both_active_and_released() {
        // Exercises the same `SequenceLedger::clear` the Unload path calls
        // (`adapter_trait.inc.rs`) so a genuinely new deployment does not
        // inherit either this node's residency or its tombstones. Driving it
        // through `Work::Unload` itself would need a real stage server
        // process, which is outside what this adapter's unit tests start.
        let mut ledger = SequenceLedger::new();
        ledger.active.insert("seq-active".to_owned(), 0);
        ledger.release("seq-released", 0);
        assert!(!ledger.active.is_empty());
        assert!(!ledger.released.is_empty());
        assert!(!ledger.released_order.is_empty());

        ledger.clear();

        assert!(ledger.active.is_empty());
        assert!(ledger.released.is_empty());
        assert!(ledger.released_order.is_empty());
    }

    /// A real four-way run produced `completed=1 failed=0 unanswered=3`: one
    /// tombstoned sequence in a hop aborted `execute_hop` for the whole hop,
    /// and `hop()` used to report only that one sequence as `Failed`,
    /// leaving its hop-mates with no outcome at all -- silently hung in the
    /// runner's `in_flight` map, since `events.rs`'s `Event::Failed` handler
    /// only stops holding the queue open once every in-flight sequence has
    /// been accounted for (see `agent/src/node/runner/events.rs`).
    ///
    /// This drives `hop()` itself (not just `execute_hop`) with a hop naming
    /// three sequences where only one is tombstoned, and checks that all
    /// three come out the other side as `Event::Failed` -- not just the
    /// tombstoned one -- and that no `Event::HopComplete` is raised for a
    /// hop that never reached the backend.
    #[test]
    fn every_sequence_in_a_failed_hop_gets_its_own_failed_event() {
        let adapter = adapter();
        adapter.sequences.lock().unwrap().release("seq-b-released", 0);

        let recorder = Recorder::default();
        adapter.hop(
            hop_for(vec![
                sequence("seq-a-active", None),
                sequence("seq-b-released", None),
                sequence("seq-c-active", None),
            ]),
            &recorder,
        );

        let events = recorder.0.into_inner().unwrap();
        assert_eq!(
            events.len(),
            3,
            "every named sequence must get its own outcome, not just the tombstoned one: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, p4_adapter::Event::HopComplete { .. })),
            "a hop that never reached the backend must not also claim to have completed: {events:?}"
        );
        for expected_sequence in ["seq-a-active", "seq-b-released", "seq-c-active"] {
            let failure = events.iter().find_map(|event| match event {
                p4_adapter::Event::Failed {
                    sequence: Some(sequence),
                    detail,
                    ..
                } if sequence == expected_sequence => Some(detail.clone()),
                _ => None,
            });
            let Some(detail) = failure else {
                panic!("{expected_sequence} never got a Failed event: {events:?}");
            };
            if expected_sequence == "seq-b-released" {
                assert!(
                    detail.contains("already released"),
                    "the culprit's own detail must say why: {detail}"
                );
            } else {
                assert!(
                    detail.contains("seq-b-released"),
                    "a sibling's detail must say which sequence actually caused the hop \
                     to fail, not repeat a generic message: {detail}"
                );
            }
        }
    }

    /// The single-sequence case must not regress to a worded-differently
    /// message: one sequence in, one `Failed` out, with the original detail
    /// verbatim (no "sibling" wrapping when there are no siblings).
    #[test]
    fn a_single_sequence_failed_hop_keeps_the_original_detail_unwrapped() {
        let adapter = adapter();
        adapter.sequences.lock().unwrap().release("seq-alone", 0);

        let recorder = Recorder::default();
        adapter.hop(hop_for(vec![sequence("seq-alone", None)]), &recorder);

        let events = recorder.0.into_inner().unwrap();
        assert_eq!(events.len(), 1, "{events:?}");
        let p4_adapter::Event::Failed {
            sequence: Some(sequence),
            detail,
            ..
        } = &events[0]
        else {
            panic!("expected a single Failed event: {events:?}");
        };
        assert_eq!(sequence, "seq-alone");
        assert!(detail.contains("already released"));
        assert!(
            !detail.contains("sibling") && !detail.contains("this hop failed because"),
            "a lone sequence must not get the multi-sequence wrapper message: {detail}"
        );
    }
}
