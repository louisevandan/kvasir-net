//! Adapter startup, bounded scheduling, and persistent routed dispatch.

use crate::application::inference;
use crate::application::model_load_options;
use crate::application::response::RouteResponder;
use crate::application::scheduler::{FixedLinger, Job, Scheduler};
use crate::domain::config::AdapterConfig;
use p4_protocol::{
    Message, RoutedMessage, read_message, read_routed_message, write_message, write_routed_message,
};
use std::collections::{HashMap, HashSet};
use std::env;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;

const DEFAULT_MAX_INFLIGHT: usize = 256;
const DEFAULT_MAX_QUEUED: usize = 1024;
const DEFAULT_MAX_BATCH: usize = 256;
const DEFAULT_BATCH_LINGER_MS: usize = 0;
const RESPONSE_QUEUE: usize = 4096;

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("usage: p4-llamacpp LISTEN_ENDPOINT AGENT_ENDPOINT ADAPTER_ID LLAMA_SERVER_ENDPOINT MODEL_NAME".into());
    }
    let max_inflight = configured_limit("P4_LLAMACPP_MAX_INFLIGHT", DEFAULT_MAX_INFLIGHT);
    let config = AdapterConfig {
        agent_endpoint: args[1].clone(),
        adapter_id: args[2].clone(),
        endpoint: args[3].clone(),
        model: args[4].clone(),
        nodes: Arc::new(RwLock::new(HashSet::new())),
        bindings: Arc::new(RwLock::new(HashMap::new())),
        active_upstreams: Arc::new(Mutex::new(HashMap::new())),
        max_inflight,
        max_queued: configured_limit("P4_LLAMACPP_MAX_QUEUED", DEFAULT_MAX_QUEUED),
        max_batch: configured_limit("P4_LLAMACPP_BATCH_MAX", DEFAULT_MAX_BATCH).min(max_inflight),
        batch_linger_ms: configured_nonnegative_limit(
            "P4_LLAMACPP_BATCH_LINGER_MS",
            DEFAULT_BATCH_LINGER_MS,
        ),
    };
    let listener = TcpListener::bind(&args[0])?;
    register_adapter(&config, &args[0])?;
    let worker_config = config.clone();
    let scheduler = Arc::new(Scheduler::start(
        config.max_inflight,
        config.max_queued,
        config.max_batch,
        Arc::new(FixedLinger(std::time::Duration::from_millis(
            config.batch_linger_ms as u64,
        ))),
        Arc::new(move |job| handle_job(job, &worker_config)),
    )?);
    println!(
        "P4_LLAMACPP_READY listen={} upstream={} model={} max_inflight={} max_queued={} max_batch={} partial_linger_ms={} cycle_hint=http_completion batching=llama_server_continuous",
        args[0],
        config.endpoint,
        config.model,
        config.max_inflight,
        config.max_queued,
        config.max_batch,
        config.batch_linger_ms
    );
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let config = config.clone();
                let scheduler = Arc::clone(&scheduler);
                thread::spawn(move || {
                    if let Err(error) = serve_connection(stream, config, scheduler)
                        && !error
                            .downcast_ref::<p4_protocol::ProtocolError>()
                            .is_some_and(p4_protocol::ProtocolError::is_peer_closed)
                    {
                        eprintln!("P4_LLAMACPP_ERROR {error}");
                    }
                });
            }
            Err(error) => eprintln!("P4_LLAMACPP_ACCEPT_ERROR {error}"),
        }
    }
    Ok(())
}

fn serve_connection(
    mut stream: TcpStream,
    config: AdapterConfig,
    scheduler: Arc<Scheduler>,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_nodelay(true)?;
    let mut writer = stream.try_clone()?;
    let (responses, response_rx) = sync_channel::<RoutedMessage>(RESPONSE_QUEUE);
    thread::spawn(move || {
        while let Ok(response) = response_rx.recv() {
            if write_routed_message(&mut writer, &response).is_err() {
                break;
            }
        }
    });
    loop {
        let routed = read_routed_message(&mut stream)?;
        if matches!(routed.message, Message::Cancel { .. }) {
            config.cancel(&routed.route_id);
            continue;
        }
        let responder = RouteResponder::new(
            routed.route_id.clone(),
            routed.deadline_unix_ms,
            responses.clone(),
        );
        if expired(routed.deadline_unix_ms) {
            responder.emit(Message::Error {
                request_id: routed.message.correlation_id().into(),
                detail: "llama.cpp adapter route deadline expired before scheduling".into(),
            })?;
            continue;
        }
        if let Err(job) = scheduler.submit(Job {
            routed,
            responses: responses.clone(),
        }) {
            responder.emit(Message::Error {
                request_id: job.routed.message.correlation_id().into(),
                detail: format!(
                    "llama.cpp adapter queue is full (inflight={}, queued={})",
                    config.max_inflight, config.max_queued
                ),
            })?;
        }
    }
}

fn handle_job(job: Job, config: &AdapterConfig) {
    let request_id = job.routed.message.correlation_id().to_owned();
    let responder = RouteResponder::new(
        job.routed.route_id.clone(),
        job.routed.deadline_unix_ms,
        job.responses,
    );
    let result = handle(job.routed, &responder, config);
    if let Err(error) = result {
        let _ = responder.emit(Message::Error {
            request_id,
            detail: format!("llama.cpp adapter failed: {error}"),
        });
    }
}

fn handle(
    routed: RoutedMessage,
    responses: &RouteResponder,
    config: &AdapterConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    match routed.message {
        Message::Execute(request) => {
            inference::execute(&routed.route_id, responses, request, config)
        }
        Message::NodeCreate {
            operation_id,
            node_id,
            adapter_id,
            ..
        } => {
            config
                .nodes
                .write()
                .map_err(|_| "node registry lock poisoned")?
                .insert(node_id.clone());
            responses.emit(Message::NodeCreated {
                operation_id,
                node_id,
                adapter_id,
                state: "ready".into(),
                detail: "stock llama-server NodeSlot is ready without a model binding".into(),
            })
        }
        Message::ModelLoad {
            operation_id,
            node_id,
            deployment_id,
            binding_id,
            model,
            stage_plan,
            ..
        } => load_model(
            responses,
            operation_id,
            node_id,
            deployment_id,
            binding_id,
            model,
            stage_plan,
            config,
        ),
        Message::ModelUnload {
            operation_id,
            node_id,
            deployment_id,
            binding_id,
            ..
        } => {
            config
                .bindings
                .write()
                .map_err(|_| "binding registry lock poisoned")?
                .remove(&(node_id.clone(), binding_id.clone()));
            responses.emit(Message::ModelUnbound {
                operation_id,
                node_id,
                deployment_id,
                binding_id,
                detail: "binding removed; stock llama-server process remains available".into(),
            })
        }
        Message::HealthCheck {
            node_id,
            request_id,
            ..
        } => responses.emit(Message::Health {
            request_id,
            node_id,
            ready: true,
            detail: format!(
                "llama.cpp HTTP adapter -> {}; inflight_limit={}; batch_max={}; partial_linger_ms={}",
                config.endpoint, config.max_inflight, config.max_batch, config.batch_linger_ms
            ),
        }),
        other => responses.emit(Message::Error {
            request_id: other.correlation_id().into(),
            detail: "llama.cpp adapter accepts NodeCreate, ModelLoad, ModelUnload, EXECUTE or HEALTH_CHECK".into(),
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn load_model(
    responses: &RouteResponder,
    operation_id: String,
    node_id: String,
    deployment_id: String,
    binding_id: String,
    model: String,
    stage_plan: String,
    config: &AdapterConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    if !config
        .nodes
        .read()
        .map_err(|_| "node registry lock poisoned")?
        .contains(&node_id)
    {
        return responses.emit(Message::Error {
            request_id: operation_id,
            detail: format!("node {node_id} was not created"),
        });
    }
    model_load_options::require_process_start_compatible(&stage_plan)?;
    responses.emit(Message::LoadProgress {
        operation_id: operation_id.clone(),
        node_id: node_id.clone(),
        percent: 0,
        detail: "stock llama-server model is process-owned; checking configured model".into(),
    })?;
    if model != config.model {
        return responses.emit(Message::Error {
            request_id: operation_id,
            detail: format!("adapter is configured for model {}", config.model),
        });
    }
    responses.emit(Message::LoadProgress {
        operation_id: operation_id.clone(),
        node_id: node_id.clone(),
        percent: 100,
        detail: "configured stock llama-server is ready; P4 did not mutate its model".into(),
    })?;
    let generation = {
        let mut bindings = config
            .bindings
            .write()
            .map_err(|_| "binding registry lock poisoned")?;
        let key = (node_id.clone(), binding_id.clone());
        let generation = bindings.get(&key).copied().unwrap_or(0) + 1;
        bindings.insert(key, generation);
        generation
    };
    responses.emit(Message::ModelBound {
        operation_id,
        node_id,
        deployment_id,
        binding_id,
        runtime_generation: generation,
        state: "ready".into(),
        detail: "stock llama-server binding is ready; model process remains adapter-owned".into(),
    })
}

fn register_adapter(
    config: &AdapterConfig,
    listen: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = TcpStream::connect(&config.agent_endpoint)?;
    agent.set_nodelay(true)?;
    write_message(
        &mut agent,
        &Message::AdapterRegister {
            adapter_id: config.adapter_id.clone(),
            adapter_kind: "llamacpp".into(),
            endpoint: listen.into(),
            descriptor: format!(
                r#"{{"lifecycle":"process-owned-or-reusable","model_binding":"repeatable","model_load_options":"unsupported_after_start","max_inflight":{},"max_queued":{},"max_batch":{},"partial_linger_ms":{},"cycle_hint":"http_completion","batching":"llama_server_continuous"}}"#,
                config.max_inflight, config.max_queued, config.max_batch, config.batch_linger_ms
            ),
        },
    )?;
    match read_message(&mut agent)? {
        Message::AdapterRegistered { .. } => Ok(()),
        Message::Error { detail, .. } => Err(detail.into()),
        other => Err(format!("unexpected adapter registration response {other:?}").into()),
    }
}

fn configured_limit(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| (1..=4096).contains(value))
        .unwrap_or(default)
}

fn configured_nonnegative_limit(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value <= 60_000)
        .unwrap_or(default)
}

fn expired(deadline_unix_ms: u64) -> bool {
    deadline_unix_ms > 0
        && deadline_unix_ms
            <= std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
}
