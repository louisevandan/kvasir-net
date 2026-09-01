use super::inference_identity::{InferenceIdentity, insert_observation};
use super::wire::EventWire;
use super::{ArrivalWave, RequestArtifact, RunConfig, Sender, node_endpoint};
use p4_llamacpp_staged_adapter::v2::{
    BATCH_OBSERVATION_CONTENT_TYPE, BatchObservation, ERROR_CONTENT_TYPE, InferenceCommand,
    OUTPUT_CONTENT_TYPE, OutcomePayload, PREFILL_CONTENT_TYPE, RELEASED_CONTENT_TYPE,
    ReleasedPayload,
};
use p4_protocol::event::EventClass;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};

pub struct InferenceResult {
    pub request_count: usize,
    pub completed_count: usize,
    pub released_count: usize,
    pub requests: Vec<RequestArtifact>,
    pub batch_observations: Vec<BatchObservation>,
    pub elapsed_ms: u128,
    pub error: Option<String>,
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
    let mut observations = BTreeMap::new();
    let mut known_requests = BTreeSet::new();
    let mut seen_event_ids = BTreeSet::new();
    let identity = InferenceIdentity::new(config, &sender.outer)?;

    loop {
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
                wire,
                sender,
                started,
            )
            .await?;
            next_wave += 1;
        }
        if completed == total && released == total {
            break;
        }
        let read_until = if next_wave < config.waves.len() {
            overall.min(started + Duration::from_millis(config.waves[next_wave].after_ms))
        } else {
            overall
        };
        match wire.receive(read_until).await {
            Ok(event) => {
                if !seen_event_ids.insert(event.envelope.event_id.clone()) {
                    return Err("duplicate inference event identity".into());
                }
                match event.envelope.payload_content_type.as_str() {
                    OUTPUT_CONTENT_TYPE => {
                        let outcome: OutcomePayload = serde_json::from_slice(&event.payload)?;
                        let request = requests
                            .get_mut(&outcome.request_id)
                            .ok_or("output references a request that was not submitted")?;
                        if request.completed_ms.is_some() {
                            return Err("output arrived after a terminal outcome".into());
                        }
                        identity.output(&event, &outcome, request.outcomes.last())?;
                        let observed_ms = started.elapsed().as_millis();
                        if request.first_output_ms.is_none() {
                            request.first_output_ms = Some(observed_ms);
                        }
                        request.response.push_str(&outcome.text);
                        if outcome.stop.is_some() {
                            request.completed_ms = Some(observed_ms);
                            completed += 1;
                        }
                        request.outcomes.push(outcome);
                    }
                    RELEASED_CONTENT_TYPE => {
                        let value: ReleasedPayload = serde_json::from_slice(&event.payload)?;
                        identity.released(&event, &value, &known_requests)?;
                        released = released
                            .checked_add(value.released)
                            .ok_or("released request count overflow")?;
                        if released > total {
                            return Err("too many requests were released".into());
                        }
                    }
                    BATCH_OBSERVATION_CONTENT_TYPE => {
                        let observation: BatchObservation = serde_json::from_slice(&event.payload)?;
                        identity.observation(&event, &observation, &known_requests)?;
                        insert_observation(&mut observations, observation)?;
                    }
                    ERROR_CONTENT_TYPE => {
                        identity.error(&event, &known_requests)?;
                        failure = Some(String::from_utf8_lossy(&event.payload).into_owned());
                        break;
                    }
                    other => {
                        return Err(
                            format!("unexpected inference event content type: {other}").into()
                        );
                    }
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::TimedOut && next_wave < config.waves.len() =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(InferenceResult {
        request_count: total,
        completed_count: completed,
        released_count: released,
        requests: requests.into_values().collect(),
        batch_observations: observations.into_values().collect(),
        elapsed_ms: started.elapsed().as_millis(),
        error: failure,
    })
}

async fn send_wave<R, W>(
    config: &RunConfig,
    wave: &ArrivalWave,
    total: usize,
    next_index: &mut usize,
    requests: &mut BTreeMap<String, RequestArtifact>,
    known_requests: &mut BTreeSet<String>,
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
        wire.send(sender.event(
            node_endpoint(&config.nodes[0])?,
            EventClass::Data,
            PREFILL_CONTENT_TYPE,
            serde_json::to_vec(&command)?,
            &request_id,
        ))
        .await?;
        if !known_requests.insert(request_id.clone()) {
            return Err("duplicate submitted request identity".into());
        }
        requests.insert(
            request_id.clone(),
            RequestArtifact {
                request_id,
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
    }
    Ok(())
}
