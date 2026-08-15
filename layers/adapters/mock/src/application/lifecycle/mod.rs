//! Node creation, load, unload and health against a declared profile.
//!
//! These run on the blocking lifecycle connection, the same shape a real
//! adapter uses: one request in, a sequence of frames out, one terminal.

use crate::domain::profile::{Fault, Profile};
use crate::domain::state::{Binding, Config};
use p4_protocol::{Allocation, Message, write_message};
use std::net::TcpStream;
use std::thread;

type Fallible = Result<(), Box<dyn std::error::Error>>;

pub(crate) fn node_create(
    stream: &mut TcpStream,
    operation_id: String,
    node_id: String,
    adapter_id: String,
    config: &Config,
) -> Fallible {
    config
        .nodes
        .write()
        .map_err(|_| "node registry lock poisoned")?
        .insert(node_id.clone());
    write_message(
        stream,
        &Message::NodeCreated {
            operation_id,
            node_id,
            adapter_id,
            state: "ready".into(),
            detail: "mock NodeSlot is ready and holds no deployment".into(),
        },
    )?;
    Ok(())
}

pub(crate) fn load(
    stream: &mut TcpStream,
    operation: &str,
    node: &str,
    deployment: &str,
    binding: &str,
    plan_revision: &str,
    stage_plan: &str,
    config: &Config,
) -> Fallible {
    if !config
        .nodes
        .read()
        .map_err(|_| "node registry lock poisoned")?
        .contains(node)
    {
        return error(stream, operation, format!("node {node} was not created"));
    }
    let profile = Profile::parse(stage_plan);
    let step = profile.load / profile.load_steps;
    for index in 1..=profile.load_steps {
        if !step.is_zero() {
            thread::sleep(step);
        }
        progress(
            stream,
            operation,
            node,
            index * 100 / profile.load_steps,
            "mock load advancing",
        )?;
    }
    if profile.fault == Fault::Load {
        return error(
            stream,
            operation,
            "mock deployment was asked to fail its load".into(),
        );
    }
    draft(stream, operation, node, &profile)?;
    config.deployments.declare(deployment, profile);
    let generation = bind(config, node, binding, deployment)?;
    write_message(
        stream,
        &Message::ModelBound {
            operation_id: operation.into(),
            node_id: node.into(),
            deployment_id: deployment.into(),
            binding_id: binding.into(),
            runtime_generation: generation,
            state: "ready".into(),
            detail: format!("mock deployment bound at plan revision {plan_revision}"),
        },
    )?;
    Ok(())
}

pub(crate) fn unload(
    stream: &mut TcpStream,
    operation: &str,
    node: &str,
    deployment: &str,
    binding: &str,
    config: &Config,
) -> Fallible {
    let key = (node.to_owned(), binding.to_owned());
    let removed = {
        let mut bindings = config
            .bindings
            .write()
            .map_err(|_| "binding registry lock poisoned")?;
        match bindings.get(&key) {
            Some(current) if current.deployment_id == deployment => bindings.remove(&key).is_some(),
            _ => false,
        }
    };
    // Unloading something that is not bound is a failure, not a courtesy
    // success. D-21 recorded the opposite behaviour in the HTTP adapter, and a
    // simulator that repeats it would hide the bug it exists to expose.
    if !removed {
        return error(
            stream,
            operation,
            format!("binding {binding} does not hold deployment {deployment}"),
        );
    }
    config.deployments.release(deployment);
    write_message(
        stream,
        &Message::ModelUnbound {
            operation_id: operation.into(),
            node_id: node.into(),
            deployment_id: deployment.into(),
            binding_id: binding.into(),
            detail: "mock deployment released".into(),
        },
    )?;
    Ok(())
}

pub(crate) fn health(
    stream: &mut TcpStream,
    node_id: String,
    request_id: String,
    config: &Config,
) -> Fallible {
    let ready = config
        .nodes
        .read()
        .map_err(|_| "node registry lock poisoned")?
        .contains(&node_id);
    write_message(
        stream,
        &Message::Health {
            request_id,
            node_id,
            ready,
            detail: if ready {
                "mock node is serving".into()
            } else {
                "mock node was never created".into()
            },
        },
    )?;
    Ok(())
}

pub(crate) fn error(stream: &mut TcpStream, request: &str, detail: String) -> Fallible {
    write_message(
        stream,
        &Message::Error {
            request_id: request.into(),
            detail,
        },
    )?;
    Ok(())
}

fn progress(
    stream: &mut TcpStream,
    operation: &str,
    node: &str,
    percent: u32,
    detail: &str,
) -> Fallible {
    write_message(
        stream,
        &Message::LoadProgress {
            operation_id: operation.into(),
            node_id: node.into(),
            percent,
            detail: detail.into(),
        },
    )?;
    Ok(())
}

/// Reports the reservation the plan declared, one entry per simulated stage.
/// The categories are this adapter's words, which is the point: a reader that
/// switches on them is reading a backend's shape out of a P4 message.
fn draft(stream: &mut TcpStream, operation: &str, node: &str, profile: &Profile) -> Fallible {
    let allocations: Vec<Allocation> = (0..profile.stages)
        .map(|stage| Allocation {
            category: format!("stage{stage}.declared_reservation"),
            bytes: profile.reserved_per_stage,
        })
        .collect();
    write_message(
        stream,
        &Message::DraftReport {
            operation_id: operation.into(),
            node_id: node.into(),
            total_bytes: allocations.iter().map(|entry| entry.bytes).sum(),
            allocations,
            detail: "declared by the mock plan; nothing was allocated".into(),
        },
    )?;
    Ok(())
}

fn bind(
    config: &Config,
    node: &str,
    binding: &str,
    deployment: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let mut bindings = config
        .bindings
        .write()
        .map_err(|_| "binding registry lock poisoned")?;
    let key = (node.to_owned(), binding.to_owned());
    let generation = bindings.get(&key).map(|value| value.generation).unwrap_or(0) + 1;
    bindings.insert(
        key,
        Binding {
            deployment_id: deployment.to_owned(),
            generation,
        },
    );
    Ok(generation)
}
