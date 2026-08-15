//! Adapter startup, registration, and lifecycle dispatch.

use crate::application::lifecycle;
use crate::domain::capability;
use crate::domain::capacity::CapacityRegistry;
use crate::domain::state::Config;
use crate::infrastructure::listener;
use p4_protocol::{Message, read_message, write_message};
use std::collections::{HashMap, HashSet};
use std::env;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "usage: p4-pipeline LISTEN_ENDPOINT AGENT_ENDPOINT ADAPTER_ID LLAMA_HOST_ENDPOINT"
                .into(),
        );
    }
    let listener = TcpListener::bind(&args[0])?;
    let config = Config {
        agent_endpoint: args[1].clone(),
        adapter_id: args[2].clone(),
        host: args[3].clone(),
        nodes: Arc::new(RwLock::new(HashSet::new())),
        bindings: Arc::new(RwLock::new(HashMap::new())),
        capacity: Arc::new(CapacityRegistry::from_env()),
    };
    register_adapter(&config, &args[0])?;
    println!("P4_PIPELINE_READY listen={} host={}", args[0], config.host);
    listener::serve(listener, config)
}

pub(crate) fn handle_message(
    mut stream: TcpStream,
    message: Message,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_nodelay(true)?;
    match message {
        Message::Execute(_) => Err("execute must use the asynchronous Pipeline relay".into()),
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
            write_message(
                &mut stream,
                &Message::NodeCreated {
                    operation_id,
                    node_id,
                    adapter_id,
                    state: "ready".into(),
                    detail: "Pipeline NodeSlot is ready without a model deployment".into(),
                },
            )
            .map_err(Into::into)
        }
        Message::ModelLoad {
            controller_id,
            node_id,
            operation_id,
            deployment_id,
            binding_id,
            model,
            plan_revision,
            stage_plan,
        } => lifecycle::load::load(
            &mut stream,
            &controller_id,
            &node_id,
            &operation_id,
            &deployment_id,
            &binding_id,
            &model,
            &plan_revision,
            &stage_plan,
            &config,
        ),
        Message::ModelUnload {
            operation_id,
            node_id,
            deployment_id,
            binding_id,
            ..
        } => lifecycle::unload::unload(
            &mut stream,
            &operation_id,
            &node_id,
            &deployment_id,
            &binding_id,
            &config,
        ),
        Message::HealthCheck {
            controller_id,
            node_id,
            request_id,
        } => lifecycle::health::health(&mut stream, &controller_id, node_id, request_id, &config),
        other => write_message(
            &mut stream,
            &Message::Error {
                request_id: "unknown".into(),
                detail: format!("adapter cannot handle {other:?}"),
            },
        )
        .map_err(Into::into),
    }
}

pub(crate) fn write_error(
    stream: &mut TcpStream,
    request: &str,
    detail: String,
) -> Result<(), Box<dyn std::error::Error>> {
    write_message(
        stream,
        &Message::Error {
            request_id: request.into(),
            detail,
        },
    )?;
    Ok(())
}

fn register_adapter(config: &Config, listen: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = TcpStream::connect(&config.agent_endpoint)?;
    agent.set_nodelay(true)?;
    write_message(
        &mut agent,
        &Message::AdapterRegister {
            adapter_id: config.adapter_id.clone(),
            adapter_kind: "pipeline".into(),
            endpoint: listen.into(),
            descriptor: capability::descriptor(&config.host),
        },
    )?;
    match read_message(&mut agent)? {
        Message::AdapterRegistered { .. } => Ok(()),
        Message::Error { detail, .. } => Err(detail.into()),
        other => Err(format!("unexpected adapter registration response {other:?}").into()),
    }
}
