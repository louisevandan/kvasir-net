//! Tokio Agent listener layered on the generic P4 task queues.

use crate::Result;
use crate::application::dispatch::AgentTaskHandler;
use crate::domain::agent::AgentProcessor;
use crate::infrastructure::peer_mux::PeerMuxPool;
use crate::{AgentOptions, TaskQueue, TaskQueueConfig, TaskQueueError};
use p4_protocol::{
    Message, Participant, ParticipantRole, Phase, RoutedMessage, decode_routed_message,
    encode_routed_message,
};
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};

type AsyncError = Box<dyn std::error::Error + Send + Sync>;
const DEFAULT_MAX_CONNECTIONS: usize = 4096;
const RESPONSE_QUEUE: usize = 2048;
const HEADER_BYTES: usize = 16;
const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub fn serve_agent(options: AgentOptions) -> Result<()> {
    let physical_cores = num_cpus::get_physical().max(1);
    let worker_count = resolve_worker_count(options.workers, physical_cores);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_count)
        .max_blocking_threads(32)
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(run(options, physical_cores, worker_count))
}

async fn run(options: AgentOptions, physical_cores: usize, worker_count: usize) -> Result<()> {
    let listener = TcpListener::bind(&options.listen).await?;
    let processor = Arc::new(AgentProcessor::new());
    let agent_id = processor.id().to_owned();
    let handler = Arc::new(AgentTaskHandler::new(
        processor,
        Arc::new(PeerMuxPool::default()),
    ));
    let queue = TaskQueue::start(
        agent_id.clone(),
        TaskQueueConfig::for_parallelism(worker_count),
        handler.clone(),
    )?;
    let connections = Arc::new(Semaphore::new(configured_limit(
        "P4_AGENT_MAX_CONNECTIONS",
        DEFAULT_MAX_CONNECTIONS,
    )));
    let sequence = Arc::new(AtomicU64::new(1));
    println!(
        "P4_AGENT_READY listen={} agent_id={} physical_cores={} workers={} worker_source={} task_queue=enabled",
        listener.local_addr()?,
        agent_id,
        physical_cores,
        worker_count,
        if options.workers.is_some() {
            "explicit"
        } else {
            "physical_x2"
        }
    );
    loop {
        let (mut stream, peer) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("P4_AGENT_ACCEPT_ERROR {error}");
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }
        };
        let Ok(permit) = Arc::clone(&connections).try_acquire_owned() else {
            reject_overload(&mut stream).await;
            continue;
        };
        let connection_id = sequence.fetch_add(1, Ordering::Relaxed);
        let queue = queue.clone();
        let handler = Arc::clone(&handler);
        let agent_id = agent_id.clone();
        tokio::spawn(async move {
            let result = dispatch(
                stream,
                peer.to_string(),
                connection_id,
                &agent_id,
                queue,
                Arc::clone(&handler),
            )
            .await;
            drop(permit);
            if let Err(error) = result {
                eprintln!("P4_AGENT_ERROR {error}");
            }
        });
    }
}

async fn dispatch(
    stream: TcpStream,
    peer: String,
    connection_id: u64,
    agent_id: &str,
    queue: TaskQueue,
    handler: Arc<AgentTaskHandler>,
) -> std::result::Result<(), AsyncError> {
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();
    let (responses, mut receiver) = mpsc::channel::<RoutedMessage>(RESPONSE_QUEUE);
    let response_handler = Arc::clone(&handler);
    let writer_task = tokio::spawn(async move {
        while let Some(response) = receiver.recv().await {
            let terminal = response.message.is_terminal();
            let route_id = response.route_id.clone();
            if let Err(error) = write_message(&mut writer, &response).await {
                return Err(error);
            }
            if terminal {
                response_handler.unregister(&route_id);
            }
        }
        Ok::<(), AsyncError>(())
    });
    let mut owned_routes = HashSet::new();
    let read_result = loop {
        let routed = match read_frame(&mut reader).await {
            Ok(value) => value,
            Err(error)
                if error
                    .downcast_ref::<p4_protocol::ProtocolError>()
                    .is_some_and(p4_protocol::ProtocolError::is_peer_closed) =>
            {
                break Ok(());
            }
            Err(error) => break Err(error),
        };
        let (source, target) = match participants(agent_id, &peer, connection_id, &routed.message) {
            Ok(value) => value,
            Err(error) => break Err(error),
        };
        if let Err(error) =
            handler.register(routed.route_id.clone(), source.clone(), responses.clone())
        {
            break Err(error.into());
        }
        owned_routes.insert(routed.route_id.clone());
        let task = queue.routed_task(
            routed.route_id,
            routed.deadline_unix_ms,
            source,
            target,
            routed.message,
        )?;
        let rejection = if task.deadline_unix_ms > 0 && task.deadline_unix_ms <= unix_millis() {
            Some(TaskQueueError::Invalid(
                "route deadline expired before Agent dispatch".into(),
            ))
        } else {
            queue.submit(task.clone()).err()
        };
        if let Some(error) = rejection {
            enqueue_rejection(&queue, &task, error)?;
        }
    };
    for route_id in owned_routes {
        handler.unregister(&route_id);
    }
    drop(responses);
    match writer_task.await {
        Ok(Ok(())) => read_result,
        Ok(Err(error)) => Err(error),
        Err(error) => Err(error.into()),
    }
}

fn participants(
    agent_id: &str,
    peer: &str,
    connection_id: u64,
    message: &Message,
) -> std::result::Result<(Participant, Participant), AsyncError> {
    let remote_id = format!("tcp:{peer}:{connection_id}");
    let local = |role, instance_id: String| Participant {
        agent_id: agent_id.into(),
        role,
        instance_id,
    };
    let remote = |role, instance_id: String| Participant {
        agent_id: remote_id.clone(),
        role,
        instance_id: format!("{instance_id}@{connection_id}"),
    };
    let route = match message {
        Message::IngressSubmit { controller_id, .. }
        | Message::InventoryQuery { controller_id, .. } => (
            remote(
                ParticipantRole::External,
                format!("external-{connection_id}"),
            ),
            local(ParticipantRole::Controller, controller_id.clone()),
        ),
        Message::AdapterRegister { adapter_id, .. } => (
            remote(ParticipantRole::Adapter, adapter_id.clone()),
            local(ParticipantRole::Agent, agent_id.into()),
        ),
        Message::NodeCreate {
            controller_id,
            node_id,
            ..
        }
        | Message::ModelLoad {
            controller_id,
            node_id,
            ..
        }
        | Message::ModelUnload {
            controller_id,
            node_id,
            ..
        }
        | Message::HealthCheck {
            controller_id,
            node_id,
            ..
        } => (
            remote(ParticipantRole::Controller, controller_id.clone()),
            local(ParticipantRole::Node, node_id.clone()),
        ),
        Message::Execute(request) if request.phase == Phase::Prefill && request.position == 0 => (
            remote(ParticipantRole::Controller, request.controller_id.clone()),
            local(ParticipantRole::Node, request.node_id.clone()),
        ),
        Message::Execute(request) => (
            remote(
                ParticipantRole::Node,
                format!("upstream-{}", request.position),
            ),
            local(ParticipantRole::Node, request.node_id.clone()),
        ),
        Message::Cancel { .. } => (
            remote(
                ParticipantRole::External,
                format!("external-{connection_id}"),
            ),
            local(ParticipantRole::Controller, "controller".into()),
        ),
        response => {
            return Err(
                format!("Agent ingress accepts requests, got {:?}", response.kind()).into(),
            );
        }
    };
    Ok(route)
}

fn enqueue_rejection(
    queue: &TaskQueue,
    cause: &p4_protocol::TaskEnvelope,
    error: TaskQueueError,
) -> std::result::Result<(), AsyncError> {
    let response = queue.response(
        cause,
        Message::Error {
            request_id: cause.correlation_id.clone(),
            detail: error.to_string(),
        },
    )?;
    queue.submit(response)?;
    Ok(())
}

async fn read_frame(
    stream: &mut (impl AsyncReadExt + Unpin),
) -> std::result::Result<RoutedMessage, AsyncError> {
    let mut header = [0u8; HEADER_BYTES];
    if stream.read(&mut header[..1]).await? == 0 {
        return Err(p4_protocol::ProtocolError::peer_closed().into());
    }
    stream.read_exact(&mut header[1..]).await?;
    let length = u32::from_le_bytes(header[8..12].try_into()?) as usize;
    if length > MAX_FRAME_BYTES {
        return Err("invalid P4 frame length".into());
    }
    let mut frame = Vec::with_capacity(HEADER_BYTES + length);
    frame.extend_from_slice(&header);
    frame.resize(HEADER_BYTES + length, 0);
    stream.read_exact(&mut frame[HEADER_BYTES..]).await?;
    Ok(decode_routed_message(&frame)?)
}

async fn write_message(
    stream: &mut (impl AsyncWriteExt + Unpin),
    message: &RoutedMessage,
) -> std::result::Result<(), AsyncError> {
    stream.write_all(&encode_routed_message(message)?).await?;
    Ok(())
}

async fn reject_overload(stream: &mut TcpStream) {
    let routed = RoutedMessage::new(
        "agent-admission",
        0,
        Message::Error {
            request_id: "unknown".into(),
            detail: "agent connection admission is full".into(),
        },
    );
    if let Ok(routed) = routed {
        let _ = write_message(stream, &routed).await;
    }
    let _ = stream.shutdown().await;
}

fn unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn configured_limit(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=65_536).contains(value))
        .unwrap_or(default)
}

fn resolve_worker_count(configured: Option<usize>, physical_cores: usize) -> usize {
    configured.unwrap_or_else(|| physical_cores.max(1).saturating_mul(2))
}

#[cfg(test)]
mod tests;
