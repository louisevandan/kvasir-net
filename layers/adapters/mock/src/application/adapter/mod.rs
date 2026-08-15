//! Startup, self-registration, and lifecycle dispatch.

use crate::application::lifecycle;
use crate::domain::state::{Config, Deployments};
use crate::infrastructure::listener;
use p4_protocol::{Message, read_message, write_message};
use std::collections::{HashMap, HashSet};
use std::env;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, RwLock};

pub(crate) fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: p4-mock LISTEN_ENDPOINT AGENT_ENDPOINT ADAPTER_ID".into());
    }
    let listener = TcpListener::bind(&args[0])?;
    let config = Config {
        agent_endpoint: args[1].clone(),
        adapter_id: args[2].clone(),
        nodes: Arc::new(RwLock::new(HashSet::new())),
        bindings: Arc::new(RwLock::new(HashMap::new())),
        deployments: Arc::new(Deployments::default()),
    };
    register(&config, &args[0])?;
    println!("P4_MOCK_READY listen={} agent={}", args[0], config.agent_endpoint);
    listener::serve(listener, config)
}

pub(crate) fn handle_message(
    mut stream: TcpStream,
    message: Message,
    config: Config,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_nodelay(true)?;
    match message {
        Message::Execute(_) => Err("execute must use the asynchronous relay".into()),
        Message::NodeCreate {
            operation_id,
            node_id,
            adapter_id,
            ..
        } => lifecycle::node_create(&mut stream, operation_id, node_id, adapter_id, &config),
        Message::ModelLoad {
            node_id,
            operation_id,
            deployment_id,
            binding_id,
            plan_revision,
            stage_plan,
            ..
        } => lifecycle::load(
            &mut stream,
            &operation_id,
            &node_id,
            &deployment_id,
            &binding_id,
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
        } => lifecycle::unload(
            &mut stream,
            &operation_id,
            &node_id,
            &deployment_id,
            &binding_id,
            &config,
        ),
        Message::HealthCheck {
            node_id,
            request_id,
            ..
        } => lifecycle::health(&mut stream, node_id, request_id, &config),
        other => lifecycle::error(
            &mut stream,
            "unknown",
            format!("mock adapter cannot handle {:?}", other.kind()),
        ),
    }
}

/// Declares no backend capability at all. A descriptor is opaque to P4, and
/// this one says only what the adapter is, so nothing downstream can come to
/// depend on a backend fact while running against the mock.
fn register(config: &Config, listen: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = TcpStream::connect(&config.agent_endpoint)?;
    agent.set_nodelay(true)?;
    write_message(
        &mut agent,
        &Message::AdapterRegister {
            adapter_id: config.adapter_id.clone(),
            adapter_kind: "mock".into(),
            endpoint: listen.into(),
            descriptor: r#"{"backend":"none","simulated":true}"#.into(),
        },
    )?;
    match read_message(&mut agent)? {
        Message::AdapterRegistered { .. } => Ok(()),
        Message::Error { detail, .. } => Err(detail.into()),
        other => Err(format!("unexpected registration response {other:?}").into()),
    }
}
