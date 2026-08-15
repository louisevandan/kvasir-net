//! Runtime-group health projection.

use crate::domain::state::Config;
use crate::infrastructure::http::synchronous as http;
use p4_protocol::{Message, write_message};
use serde_json::Value;
use std::net::TcpStream;

pub(crate) fn health(
    stream: &mut TcpStream,
    controller: &str,
    node: String,
    request_id: String,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let deployment = config
        .bindings
        .read()
        .map_err(|_| "binding registry lock poisoned")?
        .iter()
        .find_map(|((bound_node, _), binding)| {
            (bound_node == &node).then_some(binding.deployment_id.clone())
        })
        .unwrap_or_else(|| controller.into());
    let (status, group, _) = http::json(
        &config.host,
        "GET",
        &format!("/api/runtime-groups/{deployment}"),
        None,
    )?;
    let phase = group
        .get("phase")
        .and_then(Value::as_str)
        .unwrap_or("unavailable");
    write_message(
        stream,
        &Message::Health {
            request_id,
            node_id: node,
            ready: status == 200 && phase == "running",
            detail: format!("linker pipeline group phase={phase}"),
        },
    )?;
    Ok(())
}
