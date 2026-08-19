type HopSuccess = (Vec<Outcome>, Vec<String>);
type HopFailure = (Option<String>, String);

impl StagedAdapter {

    fn sequence_payload(&self, sequence: &p4_adapter::Sequence) -> Result<SequencePayload, String> {
        match &sequence.inbound_cut_set {
            Some(bytes) => {
                let payload = SequencePayload::decode(bytes, self.config.protocol_limits).map_err(
                    |error| format!("invalid inbound cut-set for {}: {error}", sequence.sequence),
                )?;
                if payload.sequence_id != sequence.sequence {
                    return Err(format!(
                        "inbound cut-set sequence {} does not match {}",
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
                initial_tokens: sequence.initial_tokens.clone(),
                position: Some(sequence.position),
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
        let inputs = inputs
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
                input.position = Some(sequence.position);
                input.options = sequence.options.clone();
                input.n_tokens = match hop.phase {
                    p4_adapter::Phase::Prefill => input.n_tokens,
                    p4_adapter::Phase::Decode => Some(1),
                };
                input.outcome = None;
                input
            })
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
        for (sequence, result) in hop.sequences.iter().zip(results) {
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
                    None => sequence.position.saturating_add(1) >= sequence.remaining,
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
                    sequence.position,
                    position,
                    sequence.remaining,
                    release_sequence
                );
            }
            let outbound_cut_set = result
                .encode(self.config.protocol_limits)
                .map_err(|error| {
                    (
                        Some(sequence.sequence.clone()),
                        format!("cannot encode outbound cut-set: {error}"),
                    )
                })?;
            outcomes.push(outcome_from_result(
                sequence,
                Some(outbound_cut_set),
                result.outcome,
                hop.phase,
            ));
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
    outbound_cut_set: Option<Vec<u8>>,
    metadata: Option<OutcomeMetadata>,
    phase: p4_adapter::Phase,
) -> Outcome {
    // A decode hop is one logical token across the whole staged chain.  Only
    // the tail samples that token and reports the incremented position.  An
    // intermediate stage must preserve the input position; otherwise the
    // generic node payload rewrites the continuation after every stage and a
    // four-stage pipeline consumes four positions for one generated token.
    let minimum_position = match phase {
        p4_adapter::Phase::Prefill | p4_adapter::Phase::Decode => sequence.position,
    };
    let (text, token, position, stop) = metadata.map_or_else(
        || (String::new(), None, minimum_position, None),
        |metadata| (
            metadata.text,
            Some(metadata.token),
            metadata.position,
            metadata.stop,
        ),
    );
    Outcome {
        sequence: sequence.sequence.clone(),
        outbound_cut_set,
        text,
        token,
        position,
        stop,
    }
}
