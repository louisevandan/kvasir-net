type HopSuccess = (Vec<Outcome>, Vec<String>, bool);
type HopFailure = (Option<String>, String);

impl StagedAdapter {
    fn sequence_payload(&self, sequence: &p4_adapter::Sequence) -> Result<SequencePayload, String> {
        // `Sequence::state` is this adapter's own `SequencePayload`, handed
        // back byte for byte. Position, the token the tail sampled and the
        // cut-set's layout all live in it, so nothing above has to know that
        // any of them exist.
        match &sequence.state {
            Some(bytes) => {
                // One sequence in a HOP envelope. `SequencePayload::encode`
                // writes only the cut-set -- the position, the sampled token
                // and the options were never in it, because P4 used to carry
                // them alongside. A state that leaves anything out is not a
                // state, so this is the envelope form, which is the one that
                // writes every field, and it is what the stage server is
                // handed anyway.
                let payload = HopPayload::decode(bytes, self.config.protocol_limits)
                    .map_err(|error| {
                        format!("invalid inbound state for {}: {error}", sequence.sequence)
                    })?
                    .sequences
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("empty inbound state for {}", sequence.sequence))?;
                if payload.sequence_id != sequence.sequence {
                    return Err(format!(
                        "inbound state sequence {} does not match {}",
                        payload.sequence_id, sequence.sequence
                    ));
                }
                if std::env::var_os("P4_STAGED_TRACE_STATE").is_some() {
                    eprintln!(
                        "P4_STATE_IN stage={} seq={} bytes={} position={:?} tokens={:?} descriptors={}",
                        self.config.stage_begin,
                        sequence.sequence,
                        bytes.len(),
                        payload.position,
                        payload.initial_tokens,
                        payload.descriptors.len()
                    );
                }
                Ok(payload)
            }
            None => {
                if std::env::var_os("P4_STAGED_TRACE_STATE").is_some() {
                    eprintln!(
                        "P4_STATE_IN stage={} seq={} bytes=none",
                        self.config.stage_begin, sequence.sequence
                    );
                }
                Ok(SequencePayload {
                sequence_id: sequence.sequence.clone(),
                descriptors: Vec::new(),
                payloads: Vec::new(),
                n_tokens: None,
                prompt: None,
                initial_tokens: None,
                position: Some(0),
                options: sequence.options.clone(),
                outcome: None,
            })
            }
        }
    }

    /// Rejects a hop that names a `(sequence, session_epoch)` this node has
    /// already released -- a late arrival for a session that has already
    /// ended here, not merely a reused sequence id. The tombstone is keyed
    /// on the pair precisely so a *new* session reusing an old sequence id
    /// (`tools/drive`'s `Admission::retry` does this on purpose once a prior
    /// session has gone terminal) never collides with it: `released` only
    /// ever remembers the exact epoch that ended, so a hop naming a fresh
    /// epoch for that same id is not a match here at all and is read as
    /// ordinary new work by the residency derivation that follows.
    ///
    /// `release_sequence` in `hop_execute.inc.rs` is a *prediction* --
    /// remaining length and stage position, not an observation that the
    /// backend is actually done -- so a hop can legitimately arrive again
    /// for a session this node already let go. Checked ahead of the
    /// residency derivation (also in `hop_execute.inc.rs`) so a wrong
    /// prediction (or a redelivery) fails loudly here instead of being read
    /// as brand-new work and sent to the backend as a fresh Prefill, which
    /// is what produced `llama_decode failed with status -3` before this
    /// check existed. `the_released_check_runs_before_residency_derivation_not_after`
    /// in `tests_hop.inc.rs` pins that ordering rather than just describing it.
    ///
    /// Its caller (`execute_hop`) also has to hold the lifecycle lock before
    /// calling this, not merely call this before the backend request: a
    /// redelivered hop that arrives while the first one is still running
    /// blocks on that same lock, and only sees this ledger update after the
    /// first hop's own call has finished writing to it. Checked before the
    /// lock instead, both calls can read the ledger while the sequence still
    /// looks active and both would reach the backend -- a real duplicate
    /// decode, not a rejected redelivery. A real run surfaced this: a
    /// single-request session with no concurrency at all still tombstoned
    /// once the stage server was slow enough (GPU contention from another
    /// process) for the runner's hop watchdog to redeliver.
    fn reject_released_sequences(&self, hop: &p4_adapter::Hop) -> Result<(), HopFailure> {
        let ledger = self.sequences.lock().expect("staged sequence ledger lock");
        for sequence in &hop.sequences {
            if ledger
                .released
                .contains(&(sequence.sequence.clone(), sequence.session_epoch))
            {
                self.tombstone_rejections.fetch_add(1, Ordering::Relaxed);
                return Err((
                    Some(sequence.sequence.clone()),
                    format!(
                        "sequence {} was already released at this node and cannot be resumed",
                        sequence.sequence
                    ),
                ));
            }
        }
        Ok(())
    }

    fn hop(&self, hop: p4_adapter::Hop, events: &dyn EventSink) {
        let deployment = hop.deployment.clone();
        let expected = hop
            .sequences
            .iter()
            .map(|sequence| sequence.sequence.clone())
            .collect::<Vec<_>>();
        match self.execute_hop(&hop) {
            Ok((outcomes, released, is_prefill)) => {
                if is_prefill {
                    for sequence in &expected {
                        events.raise(Event::SequenceAcquired {
                            deployment: deployment.clone(),
                            sequence: sequence.clone(),
                        });
                    }
                }
                for sequence in released {
                    events.raise(Event::SequenceReleased {
                        deployment: deployment.clone(),
                        sequence,
                    });
                }
                events.raise(Event::HopComplete {
                    hop_id: hop.id,
                    deployment,
                    expected,
                    outcomes,
                });
            }
            Err((culprit, detail)) => {
                // `execute_hop` is one round trip for every sequence in this
                // hop: a lock, one HOP request, one HOP_RESULT. An `Err` from
                // any point in it means none of `hop.sequences` produced an
                // outcome, even though only one sequence's name may be
                // attached as the cause (the tombstoned one, the one whose
                // outbound state failed to encode, ...). Reporting only that
                // one leaves its hop-mates parked in the runner's in-flight
                // map forever: `events.rs`'s `Event::Failed` handler only
                // stops holding the queue open once every in-flight sequence
                // has an outcome, and a hop that silently drops some of them
                // is exactly the silent hang a real four-way run produced --
                // `completed=1 failed=0 unanswered=3` -- before this loop
                // existed. So every sequence this hop named gets its own
                // `Failed`, worded to say which one actually caused it when
                // that differs from itself.
                if expected.is_empty() {
                    Self::failed(events, deployment, culprit, Some(hop.id), detail);
                } else {
                    for sequence in &expected {
                        let is_culprit = culprit.as_deref() == Some(sequence.as_str());
                        let message = if is_culprit || culprit.is_none() {
                            detail.clone()
                        } else {
                            format!(
                                "this hop failed because sequence {} failed: {detail}",
                                culprit.as_deref().unwrap_or("<unknown>")
                            )
                        };
                        Self::failed(
                            events,
                            deployment.clone(),
                            Some(sequence.clone()),
                            Some(hop.id),
                            message,
                        );
                    }
                }
            }
        }
    }
}

fn outcome_from_result(
    sequence: &p4_adapter::Sequence,
    forward: Option<Vec<u8>>,
    metadata: Option<OutcomeMetadata>,
) -> Outcome {
    // Only the tail samples, so only the tail has anything to say to whoever
    // asked. Every other stage returns state and silence. The position that
    // used to be reconciled here is inside `forward` now, which is why a
    // four-stage chain can no longer spend four positions on one token.
    let (text, stop, terminal_generated) = metadata.map_or_else(
        || (String::new(), None, None),
        |metadata| {
            // `position` stays inside the staged payload during ordinary
            // decoding.  At this one proven terminal boundary, however, it
            // establishes the request-level count despite UTF-8 coalescing.
            let terminal_generated = (metadata.stop.as_deref() == Some("length")
                && metadata.position >= sequence.remaining)
                .then_some(sequence.remaining);
            (metadata.text, metadata.stop, terminal_generated)
        },
    );
    Outcome {
        sequence: sequence.sequence.clone(),
        forward,
        text,
        stop,
        terminal_generated,
    }
}
