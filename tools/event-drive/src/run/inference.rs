use super::evidence_ledger::{EvidenceLedger, EvidenceStatus, SubmittedAuthority};
use super::inference_identity::InferenceIdentity;
use super::wire::EventWire;
use super::{ArrivalWave, RequestArtifact, RunConfig, Sender, node_endpoint};
use p4_llamacpp_staged_adapter::v2::{
    ApprovedOutputPayload, BATCH_OBSERVATION_CONTENT_TYPE, BatchObservation, ERROR_CONTENT_TYPE,
    InferenceCommand, OUTPUT_CONTENT_TYPE, PREFILL_CONTENT_TYPE, RELEASE_RECEIPT_CONTENT_TYPE,
    ReleaseReceipt, STAGE_SPAN_CONTENT_TYPE, StageSpan,
};
use p4_protocol::event::EventClass;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};

#[derive(Debug)]
pub struct InferenceResult {
    pub request_count: usize,
    pub completed_count: usize,
    pub released_count: usize,
    pub requests: Vec<RequestArtifact>,
    pub batch_observations: Vec<BatchObservation>,
    pub stage_spans: Vec<super::StageSpanArtifact>,
    pub elapsed_ms: u128,
    pub telemetry_complete_elapsed_ms: Option<u128>,
    pub error: Option<String>,
    /// What observation evidence was still outstanding when the run ended.
    /// `None` means every request and stage execution was accounted for.
    /// A failed run keeps this so a missing-evidence stop is not confused
    /// with a stop that had everything and refused something else.
    pub evidence_missing: Option<super::MissingEvidence>,
}

pub async fn drive<R, W>(
    config: &RunConfig,
    wire: &mut EventWire<R, W>,
    sender: &mut Sender,
) -> Result<InferenceResult, Box<dyn std::error::Error>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let total: usize = config.waves.iter().map(|wave| wave.count).sum();
    if total == 0 || total > 100_000 {
        return Err("request count must be between 1 and 100000".into());
    }
    let started = Instant::now();
    let overall = started + Duration::from_millis(config.timeout_ms);
    let mut requests = BTreeMap::new();
    let mut next_index = 0usize;
    let mut next_wave = 0usize;
    let mut completed = 0usize;
    let mut released = 0usize;
    let mut failure = None;
    let mut evidence = EvidenceLedger::new(config.nodes.len());
    let mut release_elapsed_ms = None;
    let mut telemetry_complete_elapsed_ms = None;
    let mut known_requests = BTreeSet::new();
    let mut seen_event_ids = BTreeSet::new();
    let mut submissions = super::release_ledger::SubmissionLedger::default();
    let identity = InferenceIdentity::new(config, &sender.outer)?;

    // Everything from the first submission on is accumulated, so a refusal
    // inside this loop must end the run rather than discard it. Wrapping the
    // loop keeps every `?` and `return Err` written where the check belongs
    // while the error becomes this run's first failure, and the artifact is
    // built from whatever was approved before it.
    let outcome: Result<(), Box<dyn std::error::Error>> = async {
        loop {
            if Instant::now() >= overall {
                return Err(format!(
                    "inference overall deadline expired with observation evidence {:?}",
                    evidence.status()
                )
                .into());
            }
            while next_wave < config.waves.len()
                && started.elapsed() >= Duration::from_millis(config.waves[next_wave].after_ms)
            {
                send_wave(
                    config,
                    &config.waves[next_wave],
                    total,
                    &mut next_index,
                    &mut requests,
                    &mut known_requests,
                    &mut submissions,
                    &mut evidence,
                    wire,
                    sender,
                    started,
                )
                .await?;
                next_wave += 1;
            }
            if completed == total && released == total {
                // Never include a late-telemetry wait in the established throughput
                // denominator. Missing evidence waits only to the original deadline.
                release_elapsed_ms.get_or_insert_with(|| started.elapsed().as_millis());
                if evidence.status() == EvidenceStatus::Complete {
                    // The counts are installed once, after the loop, for both
                    // outcomes. This branch owns only the timestamp.
                    telemetry_complete_elapsed_ms = Some(started.elapsed().as_millis());
                    break;
                }
            }
            let read_until = if next_wave < config.waves.len() {
                overall.min(started + Duration::from_millis(config.waves[next_wave].after_ms))
            } else {
                overall
            };
            match wire.receive(read_until).await {
                Ok(event) => {
                    // Capture receipt before parsing/validation work; retain it
                    // only if the output is approved below.
                    let received_ms = started.elapsed().as_millis();
                    if !seen_event_ids.insert(event.envelope.event_id.clone()) {
                        return Err("duplicate inference event identity".into());
                    }
                    match event.envelope.payload_content_type.as_str() {
                        OUTPUT_CONTENT_TYPE => {
                            let approved: ApprovedOutputPayload =
                                serde_json::from_slice(&event.payload)?;
                            let outcome = &approved.outcome;
                            let request = requests
                                .get_mut(&outcome.request_id)
                                .ok_or("output references a request that was not submitted")?;
                            if request.completed_ms.is_some() {
                                return Err("output arrived after a terminal outcome".into());
                            }
                            identity.output(&event, outcome, request.outcomes.last())?;
                            super::output_budget::validate_output(
                                config.max_tokens,
                                request.outcomes.len(),
                                outcome,
                            )?;
                            let release_approval = submissions.prepare_output(&approved)?;
                            evidence.output(&event, &approved)?;
                            let expected_release = release_approval.expected.clone();
                            submissions.commit_output(release_approval);
                            let observed_ms = received_ms;
                            if request.first_output_ms.is_none() {
                                request.first_output_ms = Some(observed_ms);
                            }
                            request.response.push_str(&outcome.text);
                            if outcome.stop.is_some() {
                                request.completed_ms = Some(observed_ms);
                                request.release_member = expected_release;
                                request.issued_work = approved.issued_work;
                                completed += 1;
                            }
                            request.output_received_ms.push(observed_ms);
                            request.outcomes.push(approved.outcome);
                        }
                        RELEASE_RECEIPT_CONTENT_TYPE => {
                            let value: ReleaseReceipt = serde_json::from_slice(&event.payload)?;
                            identity.released(&event, &value, &known_requests)?;
                            let newly_released = submissions.apply_receipt(&value)?;
                            // Count only what this event is allowed to release.
                            // A refusal now keeps the artifact, so a rejected
                            // receipt must not leave its arithmetic behind.
                            let next_released = released
                                .checked_add(newly_released.len())
                                .ok_or("released request count overflow")?;
                            if next_released > total {
                                return Err("too many requests were released".into());
                            }
                            released = next_released;
                            for member in newly_released {
                                requests
                                    .get_mut(&member.request_id)
                                    .expect("registered receipt member")
                                    .released = true;
                            }
                            if completed == total && released == total {
                                release_elapsed_ms
                                    .get_or_insert_with(|| started.elapsed().as_millis());
                            }
                        }
                        BATCH_OBSERVATION_CONTENT_TYPE => {
                            let observation: BatchObservation =
                                serde_json::from_slice(&event.payload)?;
                            identity.observation(&event, &observation, &known_requests)?;
                            evidence.observation(&event, observation)?;
                        }
                        STAGE_SPAN_CONTENT_TYPE => {
                            let span: StageSpan = serde_json::from_slice(&event.payload)?;
                            let node = identity.span(&event, &span, &known_requests)?;
                            evidence.span(&event, node, span)?;
                        }
                        ERROR_CONTENT_TYPE => {
                            identity.error(&event, &known_requests)?;
                            failure = Some(String::from_utf8_lossy(&event.payload).into_owned());
                            break;
                        }
                        other => {
                            return Err(format!(
                                "unexpected inference event content type: {other}"
                            )
                            .into());
                        }
                    }
                }
                Err(error)
                    if error.kind() == io::ErrorKind::TimedOut
                        && next_wave < config.waves.len()
                        && Instant::now() < overall =>
                {
                    continue;
                }
                Err(error)
                    if error.kind() == io::ErrorKind::TimedOut && Instant::now() >= overall =>
                {
                    return Err(format!(
                        "inference overall deadline expired with observation evidence {:?}",
                        evidence.status()
                    )
                    .into());
                }
                Err(error) => {
                    return Err(format!(
                        "inference observation evidence {:?}; receive failed: {error}",
                        evidence.status()
                    )
                    .into());
                }
            }
        }
        Ok(())
    }
    .await;
    // A refusal that arrives after the node already reported one does not
    // replace it: the first error is the one that explains the run.
    if let Err(error) = outcome {
        failure.get_or_insert_with(|| error.to_string());
    }
    let confirmed: BTreeSet<String> = wire.take_confirmed().into_iter().collect();
    for request in requests.values_mut() {
        if confirmed.contains(&request.submission_event_id) {
            request.submission = super::SubmissionState::Delivered;
        }
    }
    // Attribution follows the evidence, not the verdict. Evidence that is
    // complete proves every request's rows whether or not the run went on to
    // fail, be refused, or leave sequences unreleased - and a failed run that
    // reported zero rows for requests its own observations account for was
    // reporting a number nobody measured.
    //
    // `evidence_missing` is therefore the reader's contract: `None` means
    // these row counts are attributed and final; `Some` means they are
    // unattributed, and their zeros are the absence of evidence rather than
    // the absence of work.
    let evidence_missing = match evidence.status() {
        EvidenceStatus::Complete => {
            evidence.apply_counts(&mut requests);
            None
        }
        EvidenceStatus::Missing {
            requests,
            stage_executions,
        } => Some(super::MissingEvidence {
            requests,
            stage_executions,
        }),
    };
    let (batch_observations, stage_spans) = evidence.into_artifacts();
    Ok(InferenceResult {
        request_count: total,
        completed_count: completed,
        released_count: released,
        requests: requests.into_values().collect(),
        batch_observations,
        stage_spans,
        elapsed_ms: release_elapsed_ms.unwrap_or_else(|| started.elapsed().as_millis()),
        telemetry_complete_elapsed_ms,
        error: failure,
        evidence_missing,
    })
}

async fn send_wave<R, W>(
    config: &RunConfig,
    wave: &ArrivalWave,
    total: usize,
    next_index: &mut usize,
    requests: &mut BTreeMap<String, RequestArtifact>,
    known_requests: &mut BTreeSet<String>,
    submissions: &mut super::release_ledger::SubmissionLedger,
    evidence: &mut EvidenceLedger,
    wire: &mut EventWire<R, W>,
    sender: &mut Sender,
    started: Instant,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    for _ in 0..wave.count {
        let request_index = *next_index + 1;
        let request_id = if total == 1 {
            config.request_id.clone()
        } else {
            format!("{}-{:03}", config.request_id, *next_index + 1)
        };
        let prompt_source = config.prompts.get(*next_index).unwrap_or(&config.prompt);
        let prompt = prompt_source
            .replace("{{request_index}}", &request_index.to_string())
            .replace("{{request_id}}", &request_id);
        let options = config
            .options
            .replace("{{request_index}}", &request_index.to_string())
            .replace("{{request_id}}", &request_id);
        // Minting the conversation identity is OUTER work: only OUTER knows
        // which turns belong to the same conversation, and the adapter only
        // enforces the grammar and refuses a request identity that changes
        // conversations.
        let session_key = (!config.session_key_template.is_empty()).then(|| {
            config
                .session_key_template
                .replace("{{request_index}}", &request_index.to_string())
                .replace("{{request_id}}", &request_id)
        });
        *next_index += 1;
        let command = InferenceCommand {
            load_generation: config.load_generation,
            session_id: config.session_id.clone(),
            request_id: request_id.clone(),
            tokens: Vec::new(),
            prompt: Some(prompt.clone()),
            options,
            session_key,
            max_tokens: config.max_tokens,
        };
        let event = sender.event(
            node_endpoint(&config.nodes[0])?,
            EventClass::Data,
            PREFILL_CONTENT_TYPE,
            serde_json::to_vec(&command)?,
            &request_id,
        );
        if known_requests.contains(&request_id) || requests.contains_key(&request_id) {
            return Err("duplicate submitted request identity".into());
        }
        let authority = SubmittedAuthority::from_event(&event, &command)?;
        submissions.register(&request_id, &event.envelope.event_id)?;
        evidence.register(authority.clone());
        known_requests.insert(request_id.clone());
        let key = request_id.clone();
        requests.insert(
            key.clone(),
            RequestArtifact {
                output_received_ms: Vec::new(),
                request_id,
                submission_event_id: event.envelope.event_id.clone(),
                submission_authority: Some(authority),
                // Corrected below once the write either succeeds or fails.
                submission: super::SubmissionState::Uncertain,
                issued_work: None,
                release_member: None,
                released: false,
                prompt,
                arrival_ms: started.elapsed().as_millis(),
                first_output_ms: None,
                completed_ms: None,
                prefill_rows: 0,
                decode_rows: 0,
                verify_rows: 0,
                replay_rows: 0,
                prefill_elapsed_ms: None,
                generation_elapsed_ms: None,
                logical_prefill_tps: None,
                logical_generation_tps: None,
                response: String::new(),
                outcomes: Vec::new(),
            },
        );
        // A failed send may already have reached the peer, so the request is
        // neither delivered nor unsent: its identity is spent either way.
        // Record which of the two this was before propagating, because the
        // artifact is now built even when this run aborts.
        let sent = wire.send(event).await;
        let acknowledged = wire.acknowledged_mode();
        requests
            .get_mut(&key)
            .expect("registered submission")
            .submission = if sent.is_ok() && !acknowledged {
            super::SubmissionState::Delivered
        } else {
            super::SubmissionState::Uncertain
        };
        sent?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use p4_protocol::{Address, event::OuterEndpoint};
    use std::pin::Pin;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Poll};

    struct FailedWriter(Arc<AtomicUsize>);

    impl AsyncWrite for FailedWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "scripted send failure",
            )))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_failed_send_retains_the_registered_attempt_and_never_resends_a_duplicate_request() {
        let mut node = serde_json::json!({
            "agent": "tcp://127.0.0.1:53000", "node": "head", "generation": 1,
            "binary": "unused", "endpoint": "tcp://127.0.0.1:53001", "plan": "unused",
            "n_batch": 8, "n_ubatch": 8, "context_size": 8,
            "total_context_size": 8, "sequence_capacity": 1,
        });
        node["resource_profile"] =
            serde_json::to_value(super::super::config::test_resource_profile()).unwrap();
        let config: RunConfig = serde_json::from_value(serde_json::json!({
            "ingress_agent": "tcp://127.0.0.1:53000", "channel": "send-test", "connection_generation": 7,
            "load_generation": 1, "session_id": "s", "request_id": "r", "nodes": [node],
            "prompt": "Normal prompt.", "max_tokens": 1,
        })).unwrap();
        let mut sender = Sender::new(OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 53000),
            channel: "send-test".into(),
            connection_generation: 7,
        });
        let mut requests = BTreeMap::new();
        let mut known = BTreeSet::new();
        let mut ledger = super::super::release_ledger::SubmissionLedger::default();
        let mut evidence = EvidenceLedger::new(config.nodes.len());
        let writes = Arc::new(AtomicUsize::new(0));
        let mut wire = EventWire::new(&[][..], FailedWriter(Arc::clone(&writes)));
        let wave = ArrivalWave {
            after_ms: 0,
            count: 1,
        };
        let mut index = 0;
        let error = send_wave(
            &config,
            &wave,
            1,
            &mut index,
            &mut requests,
            &mut known,
            &mut ledger,
            &mut evidence,
            &mut wire,
            &mut sender,
            Instant::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "scripted send failure");
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(
            requests["r"].submission_event_id,
            "outer:tcp://127.0.0.1:53000:send-test:7:1"
        );
        assert_eq!(sender.sequence, 2);
        assert!(known.contains("r"));
        assert!(!requests["r"].released && requests["r"].release_member.is_none());
        assert!(ledger.register("r", "new-attempt").is_err());
        let before = serde_json::to_value(&requests).unwrap();
        let error = send_wave(
            &config,
            &wave,
            1,
            &mut index,
            &mut requests,
            &mut known,
            &mut ledger,
            &mut evidence,
            &mut wire,
            &mut sender,
            Instant::now(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "duplicate submitted request identity");
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(serde_json::to_value(&requests).unwrap(), before);
    }

    /// Writes that succeed a fixed number of times and fail after that.
    struct WriterFailingAfter {
        remaining: usize,
    }

    impl AsyncWrite for WriterFailingAfter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            if self.remaining == 0 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "scripted send failure",
                )));
            }
            self.remaining -= 1;
            Poll::Ready(Ok(buffer.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    /// A write that fails may still have arrived, so the request it carried is
    /// neither delivered nor unsent. The wave it was in stops, so the waves
    /// behind it were never attempted at all. Those are three different
    /// states and the artifact has to tell them apart - a run that reports
    /// "two requests, both missing" cannot say whether the peer saw them.
    #[tokio::test]
    async fn a_wave_that_fails_mid_write_separates_delivered_uncertain_and_unsubmitted() {
        let mut node = serde_json::json!({
            "agent": "tcp://127.0.0.1:53100", "node": "head", "generation": 1,
            "binary": "unused", "endpoint": "tcp://127.0.0.1:53101", "plan": "unused",
            "n_batch": 8, "n_ubatch": 8, "context_size": 8,
            "total_context_size": 8, "sequence_capacity": 4,
        });
        node["resource_profile"] =
            serde_json::to_value(super::super::config::test_resource_profile()).unwrap();
        let config: RunConfig = serde_json::from_value(serde_json::json!({
            "ingress_agent": "tcp://127.0.0.1:53100", "channel": "partial-wave",
            "connection_generation": 7, "load_generation": 1, "session_id": "s",
            "request_id": "r",
            "nodes": [node.clone(), node],
            "prompt": "Normal prompt.", "max_tokens": 1,
            "waves": [{"after_ms": 0, "count": 2}, {"after_ms": 0, "count": 2}],
            "timeout_ms": 1_000,
        }))
        .unwrap();
        let mut sender = Sender::new(OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 53100),
            channel: "partial-wave".into(),
            connection_generation: 7,
        });
        // Each event is one length write plus one body write, so the second
        // request's body is what fails.
        let mut wire = EventWire::new(&[][..], WriterFailingAfter { remaining: 3 });

        let run = drive(&config, &mut wire, &mut sender)
            .await
            .expect("a send failure ends the run, it does not erase it");
        assert_eq!(
            run.error.as_deref(),
            Some("scripted send failure"),
            "the write failure is the run's first error"
        );
        assert_eq!(run.request_count, 4);
        assert_eq!(run.requests.len(), 2, "only the first wave was attempted");

        let artifact = super::super::assemble(config, Default::default(), run, None);
        assert_eq!(
            artifact.submissions,
            super::super::SubmissionSummary {
                configured: 4,
                delivered: 1,
                uncertain: 1,
                unsubmitted: 2,
                incomplete: 2,
                unreleased: 2,
            },
            "one write landed, one is unknown, and the second wave never ran"
        );
        assert!(!artifact.passed);
    }
}
