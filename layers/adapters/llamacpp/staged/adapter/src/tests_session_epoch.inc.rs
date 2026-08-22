// The staged adapter's own `session_epoch` fence: whether this node's
// residency and tombstone bookkeeping can tell a reused sequence id apart
// from a redelivery of the session that used to hold it.
//
// Split out of `tests_hop.inc.rs` on line count alone -- `adapter()`,
// `sequence()` and `hop_for()` below are the same shapes `tests_hop.inc.rs`
// and `hop_close.inc.rs`'s own test module build, duplicated for the reason
// their own headers give: three tiny functions are cheaper to repeat than a
// shared-fixture module for a handful of call sites.
#[cfg(test)]
mod tests_session_epoch {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<p4_adapter::Event>>);

    impl p4_adapter::EventSink for Recorder {
        fn raise(&self, event: p4_adapter::Event) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn adapter() -> StagedAdapter {
        StagedAdapter::new(StagedConfig::new(
            "stage-server",
            "127.0.0.1:1".parse().unwrap(),
        ))
    }

    fn sequence(id: &str, epoch: u64) -> p4_adapter::Sequence {
        p4_adapter::Sequence {
            sequence: id.into(),
            session_epoch: epoch,
            state: None,
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

    fn close(sequence: &str, epoch: u64) -> Close {
        Close {
            deployment: "deployment".into(),
            generation: 1,
            sequence: sequence.into(),
            session_epoch: epoch,
        }
    }

    /// The defect this pins (§7.5/§8.1 of the working brief): before
    /// `session_epoch` reached this adapter's own ledger, `released`
    /// tombstoned a bare sequence id, so a completely unrelated *later*
    /// session that legitimately reused the id -- exactly what
    /// `tools/drive`'s `Admission::retry` does once a prior request has gone
    /// terminal -- was refused with "already released at this node and
    /// cannot be resumed". That contradicts the very contract
    /// `session_epoch` exists to support. On the unfixed ledger (a bare
    /// `HashSet<String>`, ignoring this field even once it existed on the
    /// wire and on `Sequence`) this test fails exactly the way
    /// `tests_hop.inc.rs`'s `a_released_sequence_that_arrives_again_under_
    /// the_same_epoch_is_rejected` passes: same "already released" detail,
    /// same tombstone counter, because the old ledger cannot tell the two
    /// sessions apart -- it never looked at the epoch at all.
    #[test]
    fn a_released_sequence_reused_under_a_new_epoch_is_admitted_as_a_fresh_session() {
        let adapter = adapter();
        // Epoch 1's session released this sequence id and is gone.
        adapter.sequences.lock().unwrap().release("seq-reused", 1);

        // Epoch 2 is a different, later session that happens to reuse the
        // same id.
        let (returned_sequence, detail) = adapter
            .execute_hop(&hop_for(vec![sequence("seq-reused", 2)]))
            .unwrap_err();

        // Epoch 1's tombstone must not fire for epoch 2: this hop has to
        // reach the same "no real stage server" failure a never-seen
        // sequence reaches, not "already released".
        assert_eq!(returned_sequence, None, "{detail}");
        assert!(detail.contains("HOP request failed"), "{detail}");
        assert!(
            !detail.contains("already released"),
            "a new epoch for a reused sequence id must not be caught by the old \
             epoch's tombstone: {detail}"
        );
        assert_eq!(
            adapter.tombstone_rejections.load(Ordering::Relaxed),
            0,
            "a fresh epoch is not a tombstone rejection"
        );
    }

    /// The residency half of the same fence: a sequence id still `active`
    /// under an old epoch must not make a new epoch's first hop look like a
    /// continuing decode lap. There is no real backend here to observe a
    /// phase field on, so this borrows `tests_hop.inc.rs`'s own technique
    /// (`the_released_check_runs_before_residency_derivation_not_after`):
    /// pair the sequence under test with a second, genuinely fresh one in
    /// the same hop. If the epoch mismatch were *not* read as a fresh
    /// admission, `seq-carried` would derive as "continuing" while
    /// `seq-fresh` derives as "beginning", and the mixed-hop guard would
    /// fire before either ever reached the (mocked-away) backend.
    #[test]
    fn a_sequence_still_active_under_an_old_epoch_derives_as_prefill_for_a_new_one() {
        let adapter = adapter();
        adapter
            .sequences
            .lock()
            .unwrap()
            .active
            .insert("seq-carried".to_owned(), 1);

        let (returned_sequence, detail) = adapter
            .execute_hop(&hop_for(vec![
                sequence("seq-carried", 2),
                sequence("seq-fresh", 0),
            ]))
            .unwrap_err();

        assert_eq!(returned_sequence, None, "{detail}");
        assert!(detail.contains("HOP request failed"), "{detail}");
        assert!(
            !detail.contains("mixes"),
            "a new epoch for a sequence still active under an old one must derive as \
             Prefill, the same as the genuinely fresh sequence beside it, not as a \
             continuing decode lap: {detail}"
        );
    }

    /// A late hop for an epoch that has already ended -- the same
    /// `(sequence, epoch)` this node tombstoned -- is refused, same as
    /// before `session_epoch` existed. Pinned here so the new keying does
    /// not accidentally widen the tombstone into forgiving every epoch for
    /// an id once any one of them has released.
    #[test]
    fn a_late_hop_for_an_ended_epoch_is_refused() {
        let adapter = adapter();
        adapter.sequences.lock().unwrap().release("seq-ended", 5);

        let (returned_sequence, detail) = adapter
            .execute_hop(&hop_for(vec![sequence("seq-ended", 5)]))
            .unwrap_err();

        assert_eq!(returned_sequence, Some("seq-ended".to_owned()));
        assert!(detail.contains("already released"), "{detail}");
        assert_eq!(adapter.tombstone_rejections.load(Ordering::Relaxed), 1);
    }

    /// A close naming an epoch this node no longer holds -- because a newer
    /// epoch has since reserved the same sequence id -- must be a no-op:
    /// answered as done, but never touching the newer reservation. This is
    /// the native-cancel guard from `hop_close.inc.rs`'s own `close`, proven
    /// here against a ledger that already has a newer epoch in `active`
    /// rather than against an empty one.
    #[test]
    fn a_close_from_an_older_epoch_never_cancels_a_newer_epochs_reservation() {
        let adapter = adapter();
        adapter
            .sequences
            .lock()
            .unwrap()
            .active
            .insert("seq-live".to_owned(), 2);
        let recorder = Recorder::default();

        // Epoch 1's belated close arrives after epoch 2 already holds the
        // reservation.
        adapter.close(close("seq-live", 1), &recorder);

        let events = recorder.0.lock().unwrap();
        assert_eq!(
            *events,
            vec![p4_adapter::Event::Closed {
                deployment: "deployment".into(),
                sequence: "seq-live".into(),
            }],
            "the belated close must still be acknowledged as done"
        );
        // The endpoint refuses every connection, so if this close had
        // treated epoch 2's reservation as its own to release, the native
        // CANCEL attempt against it would have failed loudly as
        // `Event::Failed` instead of the silent no-op above.
        assert_eq!(
            adapter.sequences.lock().unwrap().active.get("seq-live"),
            Some(&2),
            "epoch 2's reservation must be exactly as it was"
        );
    }

    /// The same guard, the other way around: closing the epoch that *is*
    /// currently held still releases it, so the fence above is scoped to a
    /// mismatch and does not simply stop every close from working.
    #[test]
    fn a_close_for_the_epoch_currently_held_still_releases_it() {
        let adapter = adapter();
        adapter
            .sequences
            .lock()
            .unwrap()
            .active
            .insert("seq-live".to_owned(), 2);
        let recorder = Recorder::default();

        adapter.close(close("seq-live", 2), &recorder);

        // The endpoint refuses every connection, so a genuinely held
        // reservation's native release attempt fails -- `Event::Failed`, not
        // `Event::Closed` -- which is exactly what tells the same-epoch path
        // apart from the mismatched-epoch no-op above.
        let events = recorder.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(
            matches!(events[0], p4_adapter::Event::Failed { .. }),
            "{events:?}"
        );
    }
}
