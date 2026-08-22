// The native side of `p4_adapter::Close`: releasing a sequence's slot at
// this node when a hop will never name it again, and the node's own runner
// only learns that from a message the tail sends -- this file is what that
// message does once it arrives. See `agent::node::outcome::close` for why it
// exists and `hop_execute.inc.rs`'s own release path for the request shape
// this reuses (deliberately not shared as one function: that path runs
// inside an already-open hop response and this runs alone, and the two are
// entitled to diverge exactly the way `served` and `staged` are).

impl StagedAdapter {
    /// Releases this node's reservation for `close.sequence`, if it is still
    /// holding one *under the session `close.session_epoch` names*, and
    /// always reports the close as done.
    ///
    /// A sequence this node never saw a hop for, has already released, or is
    /// now held under a *different* epoch than this close names, is treated
    /// identically: absent, for this close's purposes. The third case is the
    /// one that matters most -- it is what stops a belated close for an old
    /// session from ever native-cancelling a reservation a newer session
    /// (which reused the same sequence id, the way `tools/drive`'s
    /// `Admission::retry` does on purpose) now holds. In every one of these
    /// cases nothing here talks to the backend, and the no-op still raises
    /// `Event::Closed` so the node's single lifecycle slot is not left
    /// waiting on an event that will never come -- including, deliberately,
    /// when the reason was "a newer session owns this now": that session's
    /// own eventual close is what will actually release it, and this
    /// close's own sender is still owed its acknowledgement regardless.
    ///
    /// A native failure raises `Event::Failed` instead, which the runner's
    /// `Event::Failed` handling for `Work::Close` (see `close_fence::
    /// fail_session_close`) treats apart from an ordinary hop failure: it
    /// ends this lifecycle carrier but deliberately leaves the reservation
    /// in place, refuses a new admission on the same sequence id, and sends
    /// no acknowledgement, because the backend's own release is still
    /// unconfirmed. Visible over silent either way: the failure reaches
    /// OUTER's counters the same way any other adapter failure does.
    fn close(&self, close: Close, events: &dyn EventSink) {
        let held = {
            let ledger = self.sequences.lock().expect("staged sequence ledger lock");
            ledger.active.get(&close.sequence) == Some(&close.session_epoch)
        };
        if !held {
            events.raise(Event::Closed {
                deployment: close.deployment,
                sequence: close.sequence,
            });
            return;
        }
        let mut lifecycle = self.lifecycle.lock().expect("staged lifecycle lock");
        let released = self.release_native_sequence(&mut lifecycle, &close.sequence);
        drop(lifecycle);
        match released {
            Ok(()) => {
                let mut ledger = self.sequences.lock().expect("staged sequence ledger lock");
                if ledger.release(&close.sequence, close.session_epoch) {
                    self.tombstone_evictions.fetch_add(1, Ordering::Relaxed);
                }
                drop(ledger);
                events.raise(Event::Closed {
                    deployment: close.deployment,
                    sequence: close.sequence,
                });
            }
            Err(detail) => Self::failed(events, close.deployment, Some(close.sequence), None, detail),
        }
    }

    /// One CANCEL round trip against the stage server for `sequence`, and
    /// the same tolerance `execute_hop`'s own release path has for a
    /// terminal response racing this cleanup: the server's no-active-HOP
    /// answer means the sequence is already gone there too, which is success
    /// for this call, not a fault to report.
    fn release_native_sequence(
        &self,
        lifecycle: &mut LlamaLifecycle<ProcessServerControl>,
        sequence: &str,
    ) -> Result<(), String> {
        let frame = Frame::new(Operation::Cancel, sequence.as_bytes().to_vec())
            .map_err(|error| format!("cannot create sequence release frame: {error}"))?;
        let response = lifecycle
            .request(frame)
            .map_err(|error| format!("sequence release request failed: {error:?}"))?;
        if response.header.operation == Operation::Error {
            if response.body.as_slice() == b"CANCEL rejected: no active HOP" {
                return Ok(());
            }
            return Err(format!(
                "stage server rejected sequence release: {}",
                String::from_utf8_lossy(&response.body)
            ));
        }
        if response.header.operation != Operation::Cancel || response.body.as_slice() != b"SEQUENCE_RELEASED" {
            return Err("stage server returned an invalid sequence release response".into());
        }
        Ok(())
    }
}

// `adapter()`, `Recorder` and the endpoint that refuses every connection are
// the same fixture shape `tests_hop.inc.rs` builds, duplicated for the
// reason its own header gives: three tiny functions are cheaper to repeat
// than a shared-fixture module for two call sites.
#[cfg(test)]
mod close_tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Recorder(StdMutex<Vec<p4_adapter::Event>>);

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

    fn close(sequence: &str, epoch: u64) -> Close {
        Close {
            deployment: "deployment".into(),
            generation: 1,
            sequence: sequence.into(),
            session_epoch: epoch,
        }
    }

    /// The no-op path this exists to make safe: a sequence this node never
    /// held -- absence, not a released tombstone -- must complete without
    /// ever touching the (here, deliberately unreachable) backend, and must
    /// still tell the runner the close is done rather than leaving its
    /// single lifecycle slot waiting.
    #[test]
    fn closing_a_sequence_this_node_never_held_is_a_no_op_success() {
        let adapter = adapter();
        let recorder = Recorder::default();

        adapter.close(close("never-seen", 0), &recorder);

        let events = recorder.0.lock().unwrap();
        assert_eq!(
            *events,
            vec![p4_adapter::Event::Closed {
                deployment: "deployment".into(),
                sequence: "never-seen".into(),
            }],
            "no active reservation means no backend call and one Closed"
        );
    }

    /// The same no-op path, reached through `released` instead of through
    /// never having been seen: a redelivered close for a sequence this node
    /// already let go must not attempt a second native release.
    #[test]
    fn closing_an_already_released_sequence_is_also_a_no_op_success() {
        let adapter = adapter();
        adapter.sequences.lock().unwrap().active.insert("s0".into(), 0);
        adapter.sequences.lock().unwrap().release("s0", 0);
        let recorder = Recorder::default();

        adapter.close(close("s0", 0), &recorder);

        let events = recorder.0.lock().unwrap();
        assert_eq!(
            *events,
            vec![p4_adapter::Event::Closed {
                deployment: "deployment".into(),
                sequence: "s0".into(),
            }]
        );
    }

    /// A sequence this node believes it still holds forces an attempt to
    /// tell the backend, and this adapter was never loaded (its endpoint
    /// refuses every connection), so that attempt fails. This must surface
    /// as `Event::Failed`, not as a silent success and not as a panic -- the
    /// runner's `Event::Failed` handling for `Work::Close` ends this
    /// lifecycle carrier but deliberately *keeps* the reservation and
    /// refuses a new admission on the same sequence id
    /// (`close_fence::fail_session_close`), because the backend's own
    /// release is still unconfirmed -- an undeliverable close must not let a
    /// second session race onto a slot that may still be held natively.
    #[test]
    fn a_native_failure_on_a_genuinely_held_sequence_is_reported_not_swallowed() {
        let adapter = adapter();
        adapter.sequences.lock().unwrap().active.insert("s0".into(), 0);
        let recorder = Recorder::default();

        adapter.close(close("s0", 0), &recorder);

        let events = recorder.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            p4_adapter::Event::Failed {
                deployment,
                sequence,
                hop_id,
                detail,
            } => {
                assert_eq!(deployment, "deployment");
                assert_eq!(sequence.as_deref(), Some("s0"));
                assert_eq!(*hop_id, None);
                assert!(!detail.is_empty());
            }
            other => panic!("expected Event::Failed, got {other:?}"),
        }
        // A failed close must not have moved the sequence to `released`: the
        // backend never confirmed it, so a later hop for it must still be
        // classified as an ordinary decode lap rather than as new work.
        assert_eq!(
            adapter.sequences.lock().unwrap().active.get("s0"),
            Some(&0)
        );
    }
}
