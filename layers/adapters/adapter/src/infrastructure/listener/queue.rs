use crate::application::execution::{batch as execution, stream::RuntimeStream};
use crate::domain::state::Config;
use p4_protocol::{ExecutionRequest, Message, Phase};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

use super::AsyncError;

pub(super) struct BatchJob {
    pub(super) request: ExecutionRequest,
    pub(super) deadline_unix_ms: u64,
    pub(super) permit: OwnedSemaphorePermit,
}

pub(super) async fn run(
    jobs: mpsc::Receiver<BatchJob>,
    config: Config,
    responses: mpsc::Sender<Message>,
    decode_credits: usize,
    batch_coalesce_ms: usize,
) -> Result<(), AsyncError> {
    if batch_coalesce_ms == 0 {
        return independent_loop(jobs, config, responses, decode_credits).await;
    }
    batch_loop(jobs, config, responses, decode_credits, batch_coalesce_ms).await
}

async fn batch_loop(
    mut jobs: mpsc::Receiver<BatchJob>,
    config: Config,
    responses: mpsc::Sender<Message>,
    decode_credits: usize,
    batch_coalesce_ms: usize,
) -> Result<(), AsyncError> {
    while let Some(first) = jobs.recv().await {
        if expired(first.deadline_unix_ms) {
            deadline_error(&responses, &first.request.request_id).await?;
            continue;
        }
        let phase = first.request.phase.clone();
        let limit = if phase == Phase::Prefill {
            config.capacity.capacity(&first.request.deployment_id)
        } else {
            decode_credits
        };
        let mut batch = vec![first];
        let deadline = tokio::time::sleep(Duration::from_millis(batch_coalesce_ms as u64));
        tokio::pin!(deadline);
        while batch.len() < limit {
            tokio::select! {
                biased;
                Some(job) = jobs.recv() => {
                    if expired(job.deadline_unix_ms) {
                        deadline_error(&responses, &job.request.request_id).await?;
                    } else {
                        batch.push(job);
                    }
                }
                _ = &mut deadline => break,
                else => break,
            }
        }
        let request_ids = batch
            .iter()
            .map(|job| job.request.request_id.clone())
            .collect::<Vec<_>>();
        let requests = batch.iter().map(|job| job.request.clone()).collect();
        if let Err(error) = execution::execute_batch(&responses, requests, &config).await {
            for request_id in request_ids {
                let _ = responses
                    .send(Message::Error {
                        request_id,
                        detail: format!("Pipeline batch execution failed: {error}"),
                    })
                    .await;
            }
        }
    }
    Ok(())
}

async fn independent_loop(
    mut jobs: mpsc::Receiver<BatchJob>,
    config: Config,
    responses: mpsc::Sender<Message>,
    decode_credits: usize,
) -> Result<(), AsyncError> {
    let decode = Arc::new(Semaphore::new(decode_credits));
    let runtime = Arc::new(RuntimeStream::connect(&config.host).await?);
    let mut tasks = tokio::task::JoinSet::new();
    while let Some(job) = jobs.recv().await {
        let phase_limit = if job.request.phase == Phase::Prefill {
            config.capacity.gate(&job.request.deployment_id)
        } else {
            Arc::clone(&decode)
        };
        let config = config.clone();
        let responses = responses.clone();
        let runtime = Arc::clone(&runtime);
        tasks.spawn(async move {
            let request_id = job.request.request_id.clone();
            let permit = match acquire_before_deadline(phase_limit, job.deadline_unix_ms).await? {
                Some(permit) => permit,
                None => {
                    deadline_error(&responses, &request_id).await?;
                    return Ok::<(), AsyncError>(());
                }
            };
            let result = runtime.execute(job.request, &config, &responses).await;
            drop(permit);
            drop(job.permit);
            if let Err(error) = result {
                let _ = responses
                    .send(Message::Error {
                        request_id,
                        detail: format!("Pipeline execution failed: {error}"),
                    })
                    .await;
            }
            Ok(())
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.map_err(|error| format!("Pipeline execution task failed: {error}"))??;
    }
    Ok(())
}

async fn acquire_before_deadline(
    gate: Arc<Semaphore>,
    deadline_unix_ms: u64,
) -> Result<Option<OwnedSemaphorePermit>, AsyncError> {
    let Some(wait) = remaining_deadline(deadline_unix_ms) else {
        return Ok(None);
    };
    if let Some(wait) = wait {
        return match tokio::time::timeout(wait, gate.acquire_owned()).await {
            Ok(permit) => Ok(Some(permit?)),
            Err(_) => Ok(None),
        };
    }
    Ok(Some(gate.acquire_owned().await?))
}

fn remaining_deadline(deadline_unix_ms: u64) -> Option<Option<Duration>> {
    if deadline_unix_ms == 0 {
        return Some(None);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    deadline_unix_ms
        .checked_sub(now)
        .map(Duration::from_millis)
        .map(Some)
}

fn expired(deadline_unix_ms: u64) -> bool {
    remaining_deadline(deadline_unix_ms).is_none()
}

async fn deadline_error(
    responses: &mpsc::Sender<Message>,
    request_id: &str,
) -> Result<(), AsyncError> {
    responses
        .send(Message::Error {
            request_id: request_id.into(),
            detail: "Pipeline execution deadline elapsed while waiting for adapter capacity".into(),
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{BatchJob, expired, remaining_deadline, run};
    use crate::domain::state::{Binding, Config};
    use p4_protocol::{ExecutionRequest, Message, Phase};
    use serde_json::Value;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, RwLock};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::{Semaphore, mpsc, oneshot};

    #[test]
    fn zero_deadline_never_expires() {
        assert_eq!(remaining_deadline(0), Some(None));
        assert!(!expired(0));
    }

    #[test]
    fn elapsed_deadline_expires_before_backend_dispatch() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(expired(now.saturating_sub(1)));
    }

    #[test]
    fn one_deployments_full_gate_does_not_block_another_deployment() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = listener.local_addr().unwrap();
            let (first_request, first_request_rx) = oneshot::channel();
            let server = tokio::spawn(mock_runtime(listener, first_request));
            let config = config(&endpoint.to_string());
            config.capacity.declare("deployment-a", &plan(1));
            config.capacity.declare("deployment-b", &plan(1));
            let held = config
                .capacity
                .gate("deployment-a")
                .acquire_owned()
                .await
                .unwrap();
            let (jobs, job_rx) = mpsc::channel(2);
            let (responses, mut received) = mpsc::channel(2);
            let queue_permits = Arc::new(Semaphore::new(2));
            let queue = tokio::spawn(run(job_rx, config, responses, 1, 0));
            jobs.send(job(request("a"), Arc::clone(&queue_permits)))
                .await
                .unwrap();
            jobs.send(job(request("b"), queue_permits)).await.unwrap();

            assert_eq!(first_request_rx.await.unwrap(), "request-b");
            drop(held);
            drop(jobs);
            let done = [received.recv().await, received.recv().await]
                .into_iter()
                .map(|message| match message {
                    Some(Message::Done(done)) => done.request_id,
                    other => panic!("unexpected response {other:?}"),
                })
                .collect::<Vec<_>>();
            assert!(done.contains(&"request-a".into()));
            assert!(done.contains(&"request-b".into()));
            queue.await.unwrap().unwrap();
            server.await.unwrap();
        });
    }

    fn plan(max_sequences: u64) -> String {
        format!(r#"{{"load_options":{{"batching":{{"max_sequences":{max_sequences}}}}}}}"#)
    }

    fn config(endpoint: &str) -> Config {
        let bindings = HashMap::from([
            (
                ("node-a".into(), "binding-a".into()),
                Binding {
                    deployment_id: "deployment-a".into(),
                    generation: 1,
                },
            ),
            (
                ("node-b".into(), "binding-b".into()),
                Binding {
                    deployment_id: "deployment-b".into(),
                    generation: 1,
                },
            ),
        ]);
        Config {
            host: format!("http://{endpoint}"),
            agent_endpoint: "unused".into(),
            adapter_id: "pipeline".into(),
            nodes: Arc::new(RwLock::new(HashSet::new())),
            bindings: Arc::new(RwLock::new(bindings)),
            capacity: Arc::new(crate::domain::capacity::CapacityRegistry::new(1, 8)),
        }
    }

    fn request(suffix: &str) -> ExecutionRequest {
        ExecutionRequest {
            controller_id: "controller".into(),
            node_id: format!("node-{suffix}"),
            deployment_id: format!("deployment-{suffix}"),
            binding_id: format!("binding-{suffix}"),
            runtime_generation: 1,
            request_id: format!("request-{suffix}"),
            session_id: format!("session-{suffix}"),
            phase: Phase::Prefill,
            position: 0,
            max_tokens: 1,
            temperature: 0.0,
            prompt: "test".into(),
            options: "{}".into(),
        }
    }

    fn job(request: ExecutionRequest, permits: Arc<Semaphore>) -> BatchJob {
        BatchJob {
            request,
            deadline_unix_ms: 0,
            permit: permits.try_acquire_owned().unwrap(),
        }
    }

    async fn mock_runtime(listener: TcpListener, first_request: oneshot::Sender<String>) {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0_u8; 1];
            socket.read_exact(&mut byte).await.unwrap();
            header.push(byte[0]);
        }
        socket.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: linker-pipeline-inference-stream-v1\r\nConnection: Upgrade\r\n\r\n").await.unwrap();
        let (reader, mut writer) = socket.into_split();
        let mut lines = BufReader::new(reader).lines();
        let mut first_request = Some(first_request);
        for index in 0..2 {
            let command: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            let request_id = command["request_id"].as_str().unwrap().to_owned();
            if index == 0 {
                first_request
                    .take()
                    .unwrap()
                    .send(request_id.clone())
                    .unwrap();
            }
            writer.write_all(format!("{{\"protocol\":\"linker-pipeline-inference-stream-v1\",\"type\":\"done\",\"request_id\":\"{request_id}\",\"generated_tokens\":1,\"reason\":\"stop\"}}\n").as_bytes()).await.unwrap();
        }
    }
}
