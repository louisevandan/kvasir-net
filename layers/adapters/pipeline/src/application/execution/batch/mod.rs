use crate::application::execution::options;
use crate::domain::state::Config;
use crate::infrastructure::http::asynchronous as async_http;
use p4_protocol::{ExecutionDone, ExecutionRequest, ExecutionToken, Message};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;

type AsyncError = Box<dyn std::error::Error + Send + Sync>;

struct StreamState {
    request: ExecutionRequest,
    streamed_chunks: u32,
    generated_tokens: Option<u32>,
    reason: String,
    timings: Option<Value>,
}

pub(crate) async fn execute_batch(
    responses: &mpsc::Sender<Message>,
    requests: Vec<ExecutionRequest>,
    config: &Config,
) -> Result<(), AsyncError> {
    let mut groups = HashMap::<String, Vec<ExecutionRequest>>::new();
    for request in requests {
        if let Err(detail) = validate(&request, config) {
            write_error(responses, &request.request_id, &detail).await?;
            continue;
        }
        groups
            .entry(request.deployment_id.clone())
            .or_default()
            .push(request);
    }
    for (deployment, group) in groups {
        if let Err(error) = execute_group(responses, &deployment, group, config).await {
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) fn validate(request: &ExecutionRequest, config: &Config) -> Result<(), String> {
    let key = (request.node_id.clone(), request.binding_id.clone());
    let ready = matches!(
        config.bindings.read().map_err(|_| "binding registry lock poisoned")?.get(&key),
        Some(binding) if binding.deployment_id == request.deployment_id
            && binding.generation == request.runtime_generation
    );
    ready
        .then_some(())
        .ok_or_else(|| "Pipeline binding is not ready for this runtime generation".into())
}

async fn execute_group(
    responses: &mpsc::Sender<Message>,
    deployment: &str,
    requests: Vec<ExecutionRequest>,
    config: &Config,
) -> Result<(), AsyncError> {
    let started = Instant::now();
    let mut states = HashMap::new();
    let mut entries = Vec::with_capacity(requests.len());
    for request in requests {
        match options::request_body(&request) {
            Ok(body) => {
                entries.push(json!({"request_id": request.request_id, "request": body}));
                states.insert(
                    request.request_id.clone(),
                    StreamState {
                        request,
                        streamed_chunks: 0,
                        generated_tokens: None,
                        reason: "stop".into(),
                        timings: None,
                    },
                );
            }
            Err(detail) => {
                write_error(
                    responses,
                    &request.request_id,
                    &format!("invalid Pipeline execution options: {detail}"),
                )
                .await?;
            }
        }
    }
    if states.is_empty() {
        return Ok(());
    }
    let path = format!("/api/runtime-groups/{deployment}/v1/chat/completions/batch");
    let body = json!({"requests": entries});
    let mut response = async_http::open_sse(&config.host, &path, &body).await?;
    while let Some(data) = response.next_data().await? {
        if data == "[DONE]" {
            break;
        }
        let envelope: Value = serde_json::from_str(&data)?;
        let request_id = envelope
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or("batch event has no request_id")?;
        let event = envelope.get("data").ok_or("batch event has no data")?;
        if request_id == "*" {
            let detail = event
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Pipeline batch inference failed");
            for id in states.keys() {
                write_error(responses, id, detail).await?;
            }
            return Ok(());
        }
        let state = states
            .get_mut(request_id)
            .ok_or("unknown batch request_id")?;
        if let Some(error) = event.get("error") {
            write_error(
                responses,
                request_id,
                &format!("pipeline inference error: {error}"),
            )
            .await?;
            states.remove(request_id);
            continue;
        }
        if let Some(text) = event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
        {
            if !text.is_empty() {
                write(
                    responses,
                    &Message::Token(ExecutionToken {
                        controller_id: state.request.controller_id.clone(),
                        node_id: state.request.node_id.clone(),
                        request_id: state.request.request_id.clone(),
                        session_id: state.request.session_id.clone(),
                        phase: state.request.phase.clone(),
                        position: state.request.position,
                        index: state.streamed_chunks,
                        text: text.into(),
                    }),
                )
                .await?;
                state.streamed_chunks += 1;
            }
        }
        if let Some(tokens) = event
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_u64)
        {
            state.generated_tokens = u32::try_from(tokens).ok();
        }
        if let Some(reason) = event
            .pointer("/choices/0/finish_reason")
            .and_then(Value::as_str)
        {
            state.reason = reason.into();
        }
        if let Some(timings) = event.get("timings") {
            state.timings = Some(timings.clone());
        }
    }
    let completed = states.len();
    for (_, state) in states {
        eprintln!(
            "P4_PIPELINE_BATCH_RESULT {}",
            json!({
                "request_id": &state.request.request_id,
                "prompt_n": state.timings.as_ref().and_then(|value| value.get("prompt_n")),
                "prompt_ms": state.timings.as_ref().and_then(|value| value.get("prompt_ms")),
                "predicted_n": state.timings.as_ref().and_then(|value| value.get("predicted_n")),
                "predicted_ms": state.timings.as_ref().and_then(|value| value.get("predicted_ms")),
                "request_duration_ms": state.timings.as_ref().and_then(|value| value.get("request_duration_ms")),
            })
        );
        write(
            responses,
            &Message::Done(ExecutionDone {
                controller_id: state.request.controller_id,
                node_id: state.request.node_id,
                request_id: state.request.request_id,
                session_id: state.request.session_id,
                reason: state.reason,
                generated_tokens: state.generated_tokens.unwrap_or(state.streamed_chunks),
            }),
        )
        .await?;
    }
    eprintln!(
        "P4_PIPELINE_BATCH_COMPLETE deployment={} requests={} elapsed_ms={}",
        deployment,
        completed,
        started.elapsed().as_millis()
    );
    Ok(())
}

async fn write(responses: &mpsc::Sender<Message>, message: &Message) -> Result<(), AsyncError> {
    responses
        .send(message.clone())
        .await
        .map_err(|_| "P4 response channel closed".into())
}

async fn write_error(
    responses: &mpsc::Sender<Message>,
    request_id: &str,
    detail: &str,
) -> Result<(), AsyncError> {
    write(
        responses,
        &Message::Error {
            request_id: request_id.into(),
            detail: detail.into(),
        },
    )
    .await
}
