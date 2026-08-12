use crate::application::execution::{batch as execution, options};
use crate::domain::state::Config;
use p4_protocol::{ExecutionDone, ExecutionRequest, ExecutionToken, Message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};

type AsyncError = Box<dyn std::error::Error + Send + Sync>;
type EventSender = mpsc::Sender<StreamEvent>;
const PROTOCOL: &str = "linker-pipeline-inference-stream-v1";
const PATH: &str = "/api/pipeline-inference-stream";
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct RuntimeStream {
    outbound: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<String, EventSender>>>,
}

#[derive(Serialize)]
struct InferCommand<'a> {
    protocol: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    request_id: &'a str,
    deployment_id: &'a str,
    request: Value,
}

#[derive(Deserialize)]
struct StreamEvent {
    protocol: String,
    #[serde(rename = "type")]
    kind: String,
    request_id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    generated_tokens: u32,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    detail: String,
}

impl RuntimeStream {
    pub(crate) async fn connect(host: &str) -> Result<Self, AsyncError> {
        let authority = host
            .strip_prefix("http://")
            .ok_or("Pipeline inference stream requires an http:// host")?
            .trim_end_matches('/');
        let mut stream = TcpStream::connect(authority).await?;
        stream.set_nodelay(true)?;
        stream
            .write_all(
                format!(
                    "GET {PATH} HTTP/1.1\r\nHost: {authority}\r\nConnection: Upgrade\r\nUpgrade: {PROTOCOL}\r\n\r\n"
                )
                .as_bytes(),
            )
            .await?;
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            if response.len() >= 16 * 1024 {
                return Err("Pipeline inference upgrade response is too large".into());
            }
            let mut byte = [0u8; 1];
            stream.read_exact(&mut byte).await?;
            response.push(byte[0]);
        }
        let header = String::from_utf8(response)?;
        if !header.starts_with("HTTP/1.1 101 ") {
            return Err(format!("Pipeline inference upgrade rejected: {}", header.trim()).into());
        }
        let (reader, mut writer) = stream.into_split();
        let (outbound, mut outbound_rx) = mpsc::channel::<String>(4096);
        let pending = Arc::new(Mutex::new(HashMap::<String, EventSender>::new()));
        let writer_pending = Arc::clone(&pending);
        tokio::spawn(async move {
            while let Some(line) = outbound_rx.recv().await {
                if writer.write_all(line.as_bytes()).await.is_err() {
                    fail_all(&writer_pending, "Pipeline inference stream writer closed").await;
                    return;
                }
            }
        });
        let reader_pending = Arc::clone(&pending);
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) if line.len() <= MAX_LINE_BYTES => {
                        let Ok(event) = serde_json::from_str::<StreamEvent>(&line) else {
                            fail_all(&reader_pending, "invalid Pipeline inference stream event").await;
                            return;
                        };
                        if event.protocol != PROTOCOL {
                            fail_all(
                                &reader_pending,
                                "unsupported Pipeline inference stream protocol",
                            )
                            .await;
                            return;
                        }
                        let terminal = event.kind == "done" || event.kind == "error";
                        let request_id = event.request_id.clone();
                        if let Some(sender) = reader_pending.lock().await.get(&request_id).cloned()
                        {
                            let _ = sender.send(event).await;
                        }
                        if terminal {
                            reader_pending.lock().await.remove(&request_id);
                        }
                    }
                    Ok(Some(_)) => {
                        fail_all(&reader_pending, "Pipeline inference stream event exceeds 4 MiB")
                            .await;
                        return;
                    }
                    Ok(None) => {
                        fail_all(&reader_pending, "Pipeline inference stream closed").await;
                        return;
                    }
                    Err(error) => {
                        fail_all(
                            &reader_pending,
                            &format!("Pipeline inference stream read failed: {error}"),
                        )
                        .await;
                        return;
                    }
                }
            }
        });
        Ok(Self { outbound, pending })
    }

    pub(crate) async fn execute(
        &self,
        request: ExecutionRequest,
        config: &Config,
        responses: &mpsc::Sender<Message>,
    ) -> Result<(), AsyncError> {
        if let Err(detail) = execution::validate(&request, config) {
            return write_error(responses, &request.request_id, detail).await;
        }
        let body = options::request_body(&request)
            .map_err(|detail| format!("invalid Pipeline execution options: {detail}"))?;
        let command = serde_json::to_string(&InferCommand {
            protocol: PROTOCOL,
            kind: "infer",
            request_id: &request.request_id,
            deployment_id: &request.deployment_id,
            request: body,
        })? + "\n";
        let (events, mut event_rx) = mpsc::channel(1024);
        self.pending
            .lock()
            .await
            .insert(request.request_id.clone(), events);
        if self.outbound.send(command).await.is_err() {
            self.pending.lock().await.remove(&request.request_id);
            return Err("Pipeline inference stream writer is closed".into());
        }
        let mut index = 0;
        while let Some(event) = event_rx.recv().await {
            match event.kind.as_str() {
                "token" => {
                    responses
                        .send(Message::Token(ExecutionToken {
                            controller_id: request.controller_id.clone(),
                            node_id: request.node_id.clone(),
                            request_id: request.request_id.clone(),
                            session_id: request.session_id.clone(),
                            phase: request.phase.clone(),
                            position: request.position,
                            index,
                            text: event.text,
                        }))
                        .await?;
                    index += 1;
                }
                "done" => {
                    responses
                        .send(Message::Done(ExecutionDone {
                            controller_id: request.controller_id,
                            node_id: request.node_id,
                            request_id: request.request_id,
                            session_id: request.session_id,
                            reason: event.reason,
                            generated_tokens: event.generated_tokens,
                        }))
                        .await?;
                    return Ok(());
                }
                "error" => return write_error(responses, &request.request_id, event.detail).await,
                _ => return Err("unknown Pipeline inference stream event".into()),
            }
        }
        Err("Pipeline inference stream ended before a terminal event".into())
    }
}

async fn write_error(
    responses: &mpsc::Sender<Message>,
    request_id: &str,
    detail: String,
) -> Result<(), AsyncError> {
    responses
        .send(Message::Error {
            request_id: request_id.into(),
            detail,
        })
        .await?;
    Ok(())
}

async fn fail_all(pending: &Mutex<HashMap<String, EventSender>>, detail: &str) {
    let requests = std::mem::take(&mut *pending.lock().await);
    for (request_id, sender) in requests {
        let _ = sender
            .send(StreamEvent {
                protocol: PROTOCOL.into(),
                kind: "error".into(),
                request_id,
                text: String::new(),
                generated_tokens: 0,
                reason: String::new(),
                detail: detail.into(),
            })
            .await;
    }
}

#[cfg(test)]
mod tests;
