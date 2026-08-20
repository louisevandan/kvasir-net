type HopSuccess = (Vec<Outcome>, Vec<String>);
type HopFailure = (Option<String>, String);

impl StagedAdapter {

    fn sequence_payload(&self, sequence: &p4_adapter::Sequence) -> Result<SequencePayload, String> {
        // `Sequence::state` is this adapter's own `SequencePayload`, handed
        // back byte for byte. Position, the token the tail sampled and the
        // cut-set's layout all live in it, so nothing above has to know that
        // any of them exist.
        match &sequence.state {
            Some(bytes) => {
                let payload = SequencePayload::decode(bytes, self.config.protocol_limits).map_err(
                    |error| format!("invalid inbound state for {}: {error}", sequence.sequence),
                )?;
                if payload.sequence_id != sequence.sequence {
                    return Err(format!(
                        "inbound state sequence {} does not match {}",
                        payload.sequence_id, sequence.sequence
                    ));
                }
                Ok(payload)
            }
            None => Ok(SequencePayload {
                sequence_id: sequence.sequence.clone(),
                descriptors: Vec::new(),
                payloads: Vec::new(),
                n_tokens: None,
                prompt: None,
                initial_tokens: None,
                position: Some(0),
                options: sequence.options.clone(),
                outcome: None,
            }),
        }
    }

    fn execute_hop(
        &self,
        hop: &p4_adapter::Hop,
    ) -> Result<HopSuccess, HopFailure> {
        let started = std::time::Instant::now();
        let mut lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| (None, "staged lifecycle lock is poisoned".to_owned()))?;
        let inputs = hop
            .sequences
            .iter()
            .map(|sequence| self.sequence_payload(sequence))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|detail| (None, detail))?;
        let phase = match hop.phase {
            p4_adapter::Phase::Prefill => HopPhase::Prefill,
            p4_adapter::Phase::Decode => HopPhase::Decode,
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
                input.prompt = if self.config.stage_begin == 0
                    && hop.phase == p4_adapter::Phase::Prefill
                {
                    sequence.prompt.clone()
                } else {
                    None
                };
                input.options = sequence.options.clone();
                input.n_tokens = match hop.phase {
                    p4_adapter::Phase::Prefill => input.n_tokens,
                    p4_adapter::Phase::Decode => Some(1),
                };
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
            results.iter().map(|result| {
                (
                    result.sequence_id.clone(),
                    result.n_tokens.unwrap_or_else(|| {
                        if hop.phase == p4_adapter::Phase::Decode { 1 } else { 0 }
                    }),
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
            let reached_length_terminal = hop.phase == p4_adapter::Phase::Decode
                && match result.outcome.as_ref() {
                    // Tail owns the sampled position and must keep its slot
                    // for the extra length-terminal hop.
                    Some(outcome) => outcome.position >= sequence.remaining,
                    // Intermediate stages finish their final useful decode
                    // one hop before the tail emits that terminal result.
                    None => carried[index].saturating_add(1) >= sequence.remaining,
                };
            let release_sequence = reached_length_terminal
                || result.outcome.as_ref().is_some_and(|outcome| outcome.stop.is_some());
            if std::env::var_os("P4_STAGED_TRACE_SEQUENCE_RELEASE").is_some()
                && hop.phase == p4_adapter::Phase::Decode
            {
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
            match result.outcome.as_ref() {
                Some(outcome) => {
                    result.initial_tokens = Some(vec![outcome.token]);
                    result.position = Some(outcome.position);
                }
                None => result.position = Some(carried[index]),
            }
            let forward = result
                .encode(self.config.protocol_limits)
                .map_err(|error| {
                    (
                        Some(sequence.sequence.clone()),
                        format!("cannot encode outbound state: {error}"),
                    )
                })?;
            outcomes.push(outcome_from_result(sequence, Some(forward), result.outcome));
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
        Ok((outcomes, released))
    }

    fn hop(&self, hop: p4_adapter::Hop, events: &dyn EventSink) {
        let deployment = hop.deployment.clone();
        let expected = hop
            .sequences
            .iter()
            .map(|sequence| sequence.sequence.clone())
            .collect::<Vec<_>>();
        match self.execute_hop(&hop) {
            Ok((outcomes, released)) => {
                if hop.phase == p4_adapter::Phase::Prefill {
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
            Err((sequence, detail)) => {
                Self::failed(events, deployment, sequence, Some(hop.id), detail)
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
    let (text, stop) = metadata.map_or_else(
        || (String::new(), None),
        |metadata| (metadata.text, metadata.stop),
    );
    Outcome {
        sequence: sequence.sequence.clone(),
        forward,
        text,
        stop,
    }
}
