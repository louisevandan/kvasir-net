//! Tokio-backed Pipeline listener. Execute streams stay on asynchronous I/O.

use crate::application::adapter::handle_message;
use crate::application::execution::{batch as execution, stream::RuntimeStream};
use crate::domain::state::Config;
use p4_protocol::{Message, Phase, RoutedMessage, encode_routed_message};
use std::collections::HashMap;
use std::env;
use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};

type AsyncError = Box<dyn std::error::Error + Send + Sync>;

const DEFAULT_MAX_INFLIGHT: usize = 256;
const DEFAULT_PREFILL_CREDITS: usize = 16;
const DEFAULT_DECODE_CREDITS: usize = 4;
const DEFAULT_BATCH_COALESCE_MS: usize = 0;
const RESPONSE_QUEUE: usize = 4096;
mod wire;

use wire::{read_message, restore_request_id, write_message};

pub(crate) fn serve(
    listener: StdTcpListener,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let max_inflight = configured_limit("P4_ADAPTER_MAX_INFLIGHT", DEFAULT_MAX_INFLIGHT);
    let decode_credits = configured_limit(
        "P4_ADAPTER_DECODE_CREDITS",
        DEFAULT_DECODE_CREDITS.min(max_inflight),
    )
    .min(max_inflight);
    let batch_coalesce_ms =
        configured_nonnegative_limit("P4_ADAPTER_BATCH_COALESCE_MS", DEFAULT_BATCH_COALESCE_MS);
    listener.set_nonblocking(true)?;
    let runtime = execution_runtime()?;
    println!(
        "P4_ADAPTER_TRANSPORT max_queued={} prefill_gate=per-deployment prefill_fallback={} prefill_ceiling={} decode_credits={} batch_coalesce_ms={} batch_dispatch=opportunistic time_driver=true",
        max_inflight,
        config.capacity.fallback(),
        config.capacity.ceiling(),
        decode_credits,
        batch_coalesce_ms
    );
    runtime.block_on(run(
        listener,
        config,
        max_inflight,
        decode_credits,
        batch_coalesce_ms,
    ))
}

fn execution_runtime() -> Result<Runtime, std::io::Error> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(
            std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(1)
                .clamp(2, 32),
        )
        .max_blocking_threads(32)
        .enable_io()
        .enable_time()
        .build()
}

async fn run(
    listener: StdTcpListener,
    config: Config,
    max_inflight: usize,
    decode_credits: usize,
    batch_coalesce_ms: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::from_std(listener)?;
    let permits = Arc::new(Semaphore::new(max_inflight));
    let queued = Arc::new(Semaphore::new(max_inflight));
    loop {
        let (mut stream, _) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("P4_ADAPTER_ACCEPT_ERROR {error}");
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            reject_overload(&mut stream).await;
            continue;
        };
        let config = config.clone();
        let queued = Arc::clone(&queued);
        tokio::spawn(async move {
            let result = dispatch(
                stream,
                config,
                queued,
                decode_credits,
                batch_coalesce_ms,
            )
            .await;
            drop(permit);
            if let Err(error) = result {
                if !error
                    .downcast_ref::<p4_protocol::ProtocolError>()
                    .is_some_and(p4_protocol::ProtocolError::is_peer_closed)
                {
                    eprintln!("P4_ADAPTER_ERROR {error}");
                }
            }
        });
    }
}

async fn dispatch(
    mut stream: TcpStream,
    config: Config,
    queued: Arc<Semaphore>,
    decode_credits: usize,
    batch_coalesce_ms: usize,
) -> Result<(), AsyncError> {
    stream.set_nodelay(true)?;
    let routed = read_message(&mut stream).await?;
    if matches!(&routed.message, Message::Execute(_)) {
        return serve_execution_connection(
            stream,
            routed,
            config,
            queued,
            decode_credits,
            batch_coalesce_ms,
        )
        .await;
    }
    let standard = stream.into_std()?;
    standard.set_nonblocking(false)?;
    let result = tokio::task::spawn_blocking(move || {
        handle_message(standard, routed.message, config).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Pipeline lifecycle relay task failed: {error}"))?;
    result.map_err(Into::into)
}

async fn serve_execution_connection(
    stream: TcpStream,
    first: RoutedMessage,
    config: Config,
    queued: Arc<Semaphore>,
    decode_credits: usize,
    batch_coalesce_ms: usize,
) -> Result<(), AsyncError> {
    let (mut reader, mut writer) = stream.into_split();
    let (responses, mut response_rx) = mpsc::channel::<Message>(RESPONSE_QUEUE);
    let routes = Arc::new(Mutex::new(HashMap::<String, RouteBinding>::new()));
    let writer_routes = Arc::clone(&routes);
    let writer_task = tokio::spawn(async move {
        while let Some(message) = response_rx.recv().await {
            let internal_id = message.correlation_id().to_owned();
            let binding = writer_routes.lock().await.get(&internal_id).cloned();
            let Some(binding) = binding else { continue };
            let terminal = message.is_terminal();
            let message = restore_request_id(message, &binding.request_id);
            writer
                .write_all(&encode_routed_message(&RoutedMessage {
                    route_id: internal_id.clone(),
                    deadline_unix_ms: binding.deadline_unix_ms,
                    message,
                })?)
                .await?;
            if terminal {
                writer_routes.lock().await.remove(&internal_id);
            }
        }
        Ok::<(), AsyncError>(())
    });
    let (jobs, job_rx) = mpsc::channel(DEFAULT_MAX_INFLIGHT);
    let batch_task = tokio::spawn(batch_loop(
        job_rx,
        config,
        responses.clone(),
        decode_credits,
        batch_coalesce_ms,
    ));
    schedule(first, &jobs, &queued, &routes).await?;
    loop {
        match read_message(&mut reader).await {
            Ok(routed) if matches!(&routed.message, Message::Execute(_)) => {
                schedule(routed, &jobs, &queued, &routes).await?;
            }
            Ok(RoutedMessage {
                route_id,
                message: Message::Cancel { .. },
                ..
            }) => {
                routes.lock().await.remove(&route_id);
            }
            Ok(other) => {
                eprintln!(
                    "P4_ADAPTER_ROUTE_ERROR route={} unsupported={:?}",
                    other.route_id,
                    other.message.kind()
                );
            }
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|value| value.kind() == std::io::ErrorKind::UnexpectedEof) =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    drop(jobs);
    batch_task.await??;
    drop(responses);
    writer_task.await??;
    Ok(())
}

async fn schedule(
    routed: RoutedMessage,
    jobs: &mpsc::Sender<BatchJob>,
    queued: &Arc<Semaphore>,
    routes: &Arc<Mutex<HashMap<String, RouteBinding>>>,
) -> Result<(), AsyncError> {
    let Message::Execute(mut request) = routed.message else {
        return Err("Pipeline scheduler requires Execute".into());
    };
    let request_id = std::mem::replace(&mut request.request_id, routed.route_id.clone());
    if routes
        .lock()
        .await
        .insert(
            routed.route_id,
            RouteBinding {
                request_id,
                deadline_unix_ms: routed.deadline_unix_ms,
            },
        )
        .is_some()
    {
        return Err("duplicate active Pipeline route_id".into());
    }
    let permit = Arc::clone(queued).acquire_owned().await?;
    jobs.send(BatchJob {
        request,
        _permit: permit,
    })
    .await
    .map_err(|_| "Pipeline batch dispatcher closed".into())
}

#[derive(Clone)]
struct RouteBinding {
    request_id: String,
    deadline_unix_ms: u64,
}

struct BatchJob {
    request: p4_protocol::ExecutionRequest,
    _permit: OwnedSemaphorePermit,
}

async fn batch_loop(
    mut jobs: mpsc::Receiver<BatchJob>,
    config: Config,
    responses: mpsc::Sender<Message>,
    decode_credits: usize,
    batch_coalesce_ms: usize,
) -> Result<(), AsyncError> {
    if batch_coalesce_ms == 0 {
        return independent_loop(jobs, config, responses, decode_credits).await;
    }
    while let Some(first) = jobs.recv().await {
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
                Some(job) = jobs.recv() => batch.push(job),
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
        // Prefill is gated per deployment at the capacity its controller
        // declared; decode keeps the adapter-wide credit.
        let phase_limit = if job.request.phase == Phase::Prefill {
            config.capacity.gate(&job.request.deployment_id)
        } else {
            Arc::clone(&decode)
        };
        let permit = phase_limit.acquire_owned().await?;
        let config = config.clone();
        let responses = responses.clone();
        let runtime = Arc::clone(&runtime);
        tasks.spawn(async move {
            let request_id = job.request.request_id.clone();
            let result = runtime.execute(job.request, &config, &responses).await;
            drop(permit);
            drop(job._permit);
            if let Err(error) = result {
                let _ = responses
                    .send(Message::Error {
                        request_id,
                        detail: format!("Pipeline execution failed: {error}"),
                    })
                    .await;
            }
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.map_err(|error| format!("Pipeline execution task failed: {error}"))?;
    }
    Ok(())
}

async fn reject_overload(stream: &mut TcpStream) {
    let _ = write_message(
        stream,
        &Message::Error {
            request_id: "unknown".into(),
            detail: "adapter admission is full; retry after an active stream completes".into(),
        },
    )
    .await;
    let _ = stream.shutdown().await;
}

fn configured_limit(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=4096).contains(value))
        .unwrap_or(default)
}

fn configured_nonnegative_limit(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value <= 4096)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests;
