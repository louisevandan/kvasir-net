//! Runtime-group load, progress, draft measurement, and binding publication.

use crate::application::adapter::write_error;
use crate::domain::state::{Binding, Config};
use crate::infrastructure::http::synchronous as http;
use crate::infrastructure::local_transport;
use p4_protocol::{Message, write_message};
use serde_json::{Value, json};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

pub(crate) fn load(
    stream: &mut TcpStream,
    _controller: &str,
    node: &str,
    operation: &str,
    deployment: &str,
    binding: &str,
    model: &str,
    plan_revision: &str,
    stage_plan: &str,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if !config
        .nodes
        .read()
        .map_err(|_| "node registry lock poisoned")?
        .contains(node)
    {
        return write_error(stream, operation, format!("node {node} was not created"));
    }
    let mut start = serde_json::from_str::<Value>(stage_plan)?
        .as_object()
        .cloned()
        .ok_or("stage_plan must be a JSON object")?;
    local_transport::apply_agent_local_ipc_domain(&mut start, &config.agent_endpoint);
    start.insert("controller_id".into(), Value::String(deployment.into()));
    start.insert("model".into(), Value::String(model.into()));
    write_message(
        stream,
        &Message::LoadProgress {
            operation_id: operation.into(),
            node_id: node.into(),
            percent: 0,
            detail: "pipeline runtime start requested".into(),
        },
    )?;
    let (status, _, text) = http::json(
        &config.host,
        "POST",
        "/api/runtime-groups",
        Some(&Value::Object(start.clone())),
    )?;
    if status != 201 && status != 409 {
        return write_error(
            stream,
            operation,
            format!("pipeline start HTTP {status}: {text}"),
        );
    }
    let deadline = Instant::now() + Duration::from_secs(600);
    let mut last = None;
    loop {
        let (status, group, text) = http::json(
            &config.host,
            "GET",
            &format!("/api/runtime-groups/{deployment}"),
            None,
        )?;
        if status != 200 {
            return write_error(stream, operation, format!("pipeline group disappeared: {text}"));
        }
        let phase = group
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        // Native stages can report their own 100% before the supervisor has
        // completed its multi-stage stability probes. Only group=running is
        // a terminal P4 load completion.
        let percent = load_percent(&group).min(99);
        if last != Some(percent) {
            write_message(
                stream,
                &Message::LoadProgress {
                    operation_id: operation.into(),
                    node_id: node.into(),
                    percent,
                    detail: format!("pipeline phase={phase}"),
                },
            )?;
            last = Some(percent);
        }
        if phase == "running" {
            // Size this deployment's execution gate from what the controller
            // measured, so the throttle sits next to the GPU instead of three
            // tiers upstream of it.
            let capacity = config.capacity.declare(deployment, stage_plan);
            eprintln!(
                "P4_PIPELINE_CAPACITY deployment={deployment} max_sequences={capacity} source={}",
                if crate::domain::capacity::declared_max_sequences(stage_plan).is_some() {
                    "stage_plan"
                } else {
                    "fallback"
                }
            );
            draft(stream, operation, node, model, &group, config)?;
            write_message(
                stream,
                &Message::LoadProgress {
                    operation_id: operation.into(),
                    node_id: node.into(),
                    percent: 100,
                    detail: "all native pipeline stages ready".into(),
                },
            )?;
            let generation = {
                let mut bindings = config
                    .bindings
                    .write()
                    .map_err(|_| "binding registry lock poisoned")?;
                let key = (node.into(), binding.into());
                let generation = bindings
                    .get(&key)
                    .map(|value| value.generation)
                    .unwrap_or(0)
                    + 1;
                bindings.insert(
                    key,
                    Binding {
                        deployment_id: deployment.into(),
                        generation,
                    },
                );
                generation
            };
            write_message(
                stream,
                &Message::ModelBound {
                    operation_id: operation.into(),
                    node_id: node.into(),
                    deployment_id: deployment.into(),
                    binding_id: binding.into(),
                    runtime_generation: generation,
                    state: "ready".into(),
                    detail: format!("Pipeline deployment loaded with plan revision {plan_revision}"),
                },
            )?;
            return Ok(());
        }
        if phase == "error" || Instant::now() >= deadline {
            return write_error(stream, operation, format!("pipeline group phase={phase}"));
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn draft(
    stream: &mut TcpStream,
    operation: &str,
    node: &str,
    model: &str,
    group: &Value,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let (_, inspected, _) = http::json(
        &config.host,
        "POST",
        "/api/models/inspect",
        Some(&json!({"model": model})),
    )?;
    let model_bytes = inspected
        .get("totalWeight")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let layer_bytes = inspected
        .get("weightLayer")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).sum())
        .unwrap_or(0);
    let kv_bytes = group
        .get("processes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .flat_map(|process| {
                    process
                        .get("logs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                })
                .filter_map(Value::as_str)
                .filter_map(cache_bytes)
                .sum()
        })
        .unwrap_or(0);
    write_message(stream, &Message::DraftReport { operation_id: operation.into(), node_id: node.into(), model_bytes, kv_bytes, layer_bytes, ffn_bytes: 0, detail: "model/layer bytes are GGUF exact; kv bytes are native context allocations; stock runtime does not classify FFN allocations".into() })?;
    Ok(())
}

fn cache_bytes(line: &str) -> Option<u64> {
    let marker = "\"reserved_bytes\":";
    let start = line.find(marker)? + marker.len();
    line[start..]
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}
fn load_percent(group: &Value) -> u32 {
    group
        .get("processes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|process| {
            process
                .pointer("/loadProgress/overallPercent")
                .and_then(Value::as_u64)
        })
        .min()
        .unwrap_or(1) as u32
}
