// The one round trip to the stage server: phase derivation, the HOP
// request/response itself, and the release accounting that follows it.
//
// Split out of `hop.inc.rs` on line count alone -- `hop.inc.rs` still owns
// payload framing and the tombstone check that must run before any of this,
// `execute_hop` is called from `hop()` there, and nothing here changes
// meaning by moving files.

impl StagedAdapter {
    fn execute_hop(&self, hop: &p4_adapter::Hop) -> Result<HopSuccess, HopFailure> {
        let started = std::time::Instant::now();
        // Acquired before the tombstone check, not after: a redelivered hop
        // (the runner's watchdog gives up on a slow hop and starts another
        // while the first is still running on its own blocking thread --
        // `runner/mod.rs`'s "adapter cancellation remains cooperative"
        // comment) blocks here on the *first* hop's own lifecycle lock. By
        // the time it gets the lock, the first hop has already run
        // `reject_released_sequences`'s ledger update at the bottom of this
        // function and released. Checking the ledger before this lock, as
        // this function used to, reads it before that update lands: both the
        // original and the redelivered call can see the sequence as still
        // active and both reach the backend, which is a real duplicate
        // decode, not merely a rejected redelivery.
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| (None, "staged lifecycle lock is poisoned".to_owned()))?;
        self.reject_released_sequences(hop)?;
        let inputs = hop
            .sequences
            .iter()
            .map(|sequence| self.sequence_payload(sequence))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|detail| (None, detail))?;
        // Whether a sequence is beginning work here or continuing a decode
        // lap. The boundary carries no phase for this any more -- an
        // execution holding both is a fact about a backend, not about P4 --
        // so it is derived from what this adapter already holds, the same
        // way `served` derives it from its open sessions and the mock
        // derives it from its produced-token map.
        //
        // Neither wire field answers it in general. `prompt` is absent at a
        // middle or tail stage even on that sequence's true first hop there,
        // so it never generalises past the chain head. `SequencePayload::
        // position` looked like it would and does not: it is P4-relative
        // progress toward the request's bound, which only a sampled token
        // ever advances, and the transition hop out of the initial prefill
        // sweep -- the one that must be sent as Decode so the tail finally
        // samples -- is exactly the hop where nothing has sampled yet, so it
        // still reads zero. A real two-stage run traced this: stage 0's
        // second hop is the first Decode of the run, and its inbound
        // position was 0, indistinguishable from stage 0's first (Prefill)
        // hop by that field alone.
        //
        // What answers it correctly at every stage is this adapter's own
        // memory: `self.sequences`'s `active` set is exactly the sequences
        // that have already been through a hop at this node. A sequence not
        // in it is beginning work here right now, whichever node this is and
        // whichever lap of the chain this is. (A released sequence never
        // reaches this line -- `reject_released_sequences` above already
        // turned that case into an error.)
        let is_prefill_per_sequence: Vec<bool> = {
            let ledger = self.sequences.lock().expect("staged sequence ledger lock");
            hop.sequences
                .iter()
                .map(|sequence| !ledger.active.contains(&sequence.sequence))
                .collect()
        };
        let is_prefill = is_prefill_per_sequence.first().copied().unwrap_or(true);
        if is_prefill_per_sequence
            .iter()
            .any(|&sequence_is_prefill| sequence_is_prefill != is_prefill)
        {
            return Err((
                None,
                "hop mixes a sequence beginning work with one continuing a decode lap; \
                 this adapter does not yet support a mixed hop"
                    .to_owned(),
            ));
        }
        let phase = if is_prefill {
            HopPhase::Prefill
        } else {
            HopPhase::Decode
        };
        let inputs: Vec<SequencePayload> = inputs
            .into_iter()
            .zip(hop.sequences.iter())
            .map(|(mut input, sequence)| {
                // Only stage 0 tokenizes the user prompt, and only during
                // prefill. Intermediate stages must consume the inbound
                // hidden-state cut-set; forwarding the prompt to them makes
                // the native runtime tokenize it again and skip/compete with
                // the descriptor input on the first decode lap.
                input.prompt = if self.config.stage_begin == 0 && is_prefill {
                    sequence.prompt.clone()
                } else {
                    None
                };
                input.options = sequence.options.clone();
                input.n_tokens = if is_prefill { input.n_tokens } else { Some(1) };
                input.outcome = None;
                input
            })
            .collect();
        // How far each sequence had come when this hop began. It used to
        // arrive as a P4 field; it is the adapter's own now, so it is carried
        // here rather than read back off the boundary.
        let carried: Vec<u32> = inputs
            .iter()
            .map(|input| input.position.unwrap_or(0))
            .collect();
        let body = HopPayload {
            phase,
            sequences: inputs,
            legacy: false,
        }
        .encode(self.config.protocol_limits)
        .map_err(|error| (None, format!("cannot encode HOP: {error}")))?;
        let frame = Frame::new(Operation::Hop, body)
            .map_err(|error| (None, format!("cannot create HOP: {error}")))?;
        let response = lifecycle
            .request(frame)
            .map_err(|error| (None, format!("HOP request failed: {error:?}")))?;
        if response.header.operation == Operation::Error {
            return Err((
                None,
                format!(
                    "stage server rejected HOP: {}",
                    String::from_utf8_lossy(&response.body)
                ),
            ));
        }
        if response.header.operation != Operation::HopResult {
            return Err((
                None,
                format!(
                    "stage server returned {:?} for HOP",
                    response.header.operation
                ),
            ));
        }
        let results = HopPayload::decode(&response.body, self.config.protocol_limits)
            .map_err(|error| (None, format!("cannot decode HOP_RESULT: {error}")))?
            .sequences;
        if results.len() != hop.sequences.len() {
            return Err((
                None,
                format!(
                    "HOP_RESULT returned {} sequences for {} inputs",
                    results.len(),
                    hop.sequences.len()
                ),
            ));
        }
        self.telemetry.record(
            hop,
            phase,
            results.iter().map(|result| {
                (
                    result.sequence_id.clone(),
                    result.n_tokens.unwrap_or(if is_prefill { 0 } else { 1 }),
                )
            }),
            started.elapsed(),
        );
        let mut outcomes = Vec::with_capacity(results.len());
        let mut released = Vec::new();
        for (index, (sequence, result)) in hop.sequences.iter().zip(results).enumerate() {
            if result.sequence_id != sequence.sequence {
                return Err((
                    Some(sequence.sequence.clone()),
                    format!(
                        "HOP_RESULT sequence {} does not match {}",
                        result.sequence_id, sequence.sequence
                    ),
                ));
            }
            // `Sequence::remaining` is the request's total generation bound
            // on the P4 wire, not a decrementing counter.  The node emits the
            // The final-token wire reports the last token and its length
            // terminal atomically when the backend reaches the requested
            // position. Release the native KV slot on that same boundary;
            // waiting for a strictly greater position would leak the tail
            // slot after a valid max-length response.
            // Intermediate stages do not receive the tail's sampled outcome,
            // so they cannot use its resulting position.  They can release
            // after their final useful decode, while the tail keeps its slot
            // for the extra hop that produces the length terminal.
            let reached_length_terminal = !is_prefill
                && match result.outcome.as_ref() {
                    // Tail owns the sampled position and must keep its slot
                    // for the extra length-terminal hop.
                    Some(outcome) => outcome.position >= sequence.remaining,
                    None => intermediate_stage_should_release(carried[index], sequence.remaining),
                };
            let release_sequence = reached_length_terminal
                || result.outcome.as_ref().is_some_and(|outcome| outcome.stop.is_some());
            if std::env::var_os("P4_STAGED_TRACE_SEQUENCE_RELEASE").is_some() && !is_prefill {
                let position = result
                    .outcome
                    .as_ref()
                    .map(|outcome| outcome.position.to_string())
                    .unwrap_or_else(|| "none".to_owned());
                eprintln!(
                    "P4_STAGED_SEQUENCE_RELEASE_CHECK sequence={} input_position={} output_position={} limit={} release={}",
                    sequence.sequence,
                    carried[index],
                    position,
                    sequence.remaining,
                    release_sequence
                );
            }
            // The tail sampled a token and moved the position on. Both are
            // this adapter's facts, so they go into the state the next lap's
            // stage 0 will decode rather than onto the P4 boundary.
            let mut result = result;
            // A sampled tail result at the request bound is terminal even if
            // its text did not advance P4's visible-token tally.  Commit that
            // fact before constructing the adapter outcome: otherwise the
            // agent would schedule a fresh lap from stage 0 after this hop
            // has released its slot.
            if !is_prefill {
                mark_native_length_terminal(&mut result.outcome, sequence.remaining);
            }
            match result.outcome.as_ref() {
                Some(outcome) => {
                    let token = outcome.token;
                    let position = outcome.position;
                    // A stage that sampled is the end of the chain, and the
                    // next lap begins at stage 0 from the token rather than
                    // from anything hidden. Its cut-set describes the layers
                    // behind it and belongs to nobody ahead, so it does not
                    // travel: what leaves is the token and where the session
                    // has reached.
                    //
                    // P4 used to drop this on the caller's behalf, which is
                    // the sort of thing a boundary should not know how to do.
                    // It is the adapter's to decide and it decides here.
                    result.descriptors.clear();
                    result.payloads.clear();
                    result.initial_tokens = Some(vec![token]);
                    result.position = Some(position);
                }
                None => result.position = Some(carried[index]),
            }
            // A terminal result has no consumer for a continuation.  Not
            // encoding one keeps the ownership rule structural: terminal
            // state cannot be emitted and accidentally re-enqueued later.
            let forward = if result.outcome.as_ref().is_some_and(|outcome| outcome.stop.is_some()) {
                None
            } else {
                Some(
                    HopPayload {
                        phase,
                        sequences: vec![result.clone()],
                        legacy: false,
                    }
                    .encode(self.config.protocol_limits)
                    .map_err(|error| {
                        (
                            Some(sequence.sequence.clone()),
                            format!("cannot encode outbound state: {error}"),
                        )
                    })?,
                )
            };
            outcomes.push(outcome_from_result(sequence, forward, result.outcome));
            if release_sequence {
                let frame = Frame::new(Operation::Cancel, sequence.sequence.as_bytes().to_vec())
                    .map_err(|error| {
                        (
                            Some(sequence.sequence.clone()),
                            format!("cannot create sequence release frame: {error}"),
                        )
                    })?;
                let release = lifecycle.request(frame).map_err(|error| {
                    (
                        Some(sequence.sequence.clone()),
                        format!("sequence release request failed: {error:?}"),
                    )
                })?;
                if release.header.operation == Operation::Error {
                    // A terminal response may race the sequence cleanup
                    // request.  The server's no-active-HOP response is a
                    // successful no-op for this release path; accepting only
                    // this exact error keeps all other protocol failures
                    // visible.
                    if release.body.as_slice() == b"CANCEL rejected: no active HOP" {
                        released.push(sequence.sequence.clone());
                        continue;
                    }
                    return Err((
                        Some(sequence.sequence.clone()),
                        format!(
                            "stage server rejected sequence release: {}",
                            String::from_utf8_lossy(&release.body)
                        ),
                    ));
                }
                if release.header.operation != Operation::Cancel
                    || release.body.as_slice() != b"SEQUENCE_RELEASED"
                {
                    return Err((
                        Some(sequence.sequence.clone()),
                        "stage server returned an invalid sequence release response".into(),
                    ));
                }
                released.push(sequence.sequence.clone());
            }
        }
        // Every sequence in this hop has now been through a hop at this
        // node, whether it began here or continued: `active` is what the
        // next hop's derivation reads, so it has to gain every sequence this
        // one saw. Every sequence this one released moves to `released` in
        // the same critical section, so the two sets can never drift apart
        // -- see `SequenceLedger`.
        {
            let mut ledger = self.sequences.lock().expect("staged sequence ledger lock");
            for sequence in &hop.sequences {
                ledger.active.insert(sequence.sequence.clone());
            }
            for sequence in &released {
                if ledger.release(sequence) {
                    self.tombstone_evictions.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        Ok((outcomes, released, is_prefill))
    }
}
