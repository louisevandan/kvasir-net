//! Runtime-group unload and binding removal.

use crate::application::adapter::write_error;
use crate::domain::state::Config;
use crate::infrastructure::http::synchronous as http;
use p4_protocol::{Message, write_message};
use std::net::TcpStream;

pub(crate) fn unload(
    stream: &mut TcpStream,
    operation: &str,
    node: &str,
    deployment: &str,
    binding: &str,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let (status, _, text) = http::json(
        &config.host,
        "DELETE",
        &format!("/api/runtime-groups/{deployment}"),
        None,
    )?;
    if status != 200 && status != 404 {
        return write_error(
            stream,
            operation,
            format!("pipeline unload HTTP {status}: {text}"),
        );
    }
    config
        .bindings
        .write()
        .map_err(|_| "binding registry lock poisoned")?
        .remove(&(node.into(), binding.into()));
    config.capacity.forget(deployment);
    write_message(
        stream,
        &Message::ModelUnbound {
            operation_id: operation.into(),
            node_id: node.into(),
            deployment_id: deployment.into(),
            binding_id: binding.into(),
            detail: "Pipeline model deployment unloaded; NodeSlot remains ready".into(),
        },
    )?;
    Ok(())
}
