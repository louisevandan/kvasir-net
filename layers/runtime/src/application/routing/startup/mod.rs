//! Startup argument parsing for Agent and compatibility relays.

use crate::foundation::transport::Result;
use std::collections::HashMap;
use std::env;

/// Parses `NODE_ID=ENDPOINT` routes for remote compatibility binaries.
pub fn parse_bindings() -> Result<(String, HashMap<String, String>)> {
    let values: Vec<String> = env::args().skip(1).collect();
    let (listen, bindings) = values
        .split_first()
        .ok_or("usage: PROGRAM LISTEN_ENDPOINT NODE_ID=ENDPOINT [...]")?;
    if bindings.is_empty() {
        return Err("usage: PROGRAM LISTEN_ENDPOINT NODE_ID=ENDPOINT [...]".into());
    }
    let mut routes = HashMap::new();
    for binding in bindings {
        let Some((node_id, endpoint)) = binding.split_once('=') else {
            return Err("binding must be NODE_ID=ENDPOINT".into());
        };
        if node_id.is_empty()
            || endpoint.is_empty()
            || routes.insert(node_id.into(), endpoint.into()).is_some()
        {
            return Err("node IDs must be unique and non-empty".into());
        }
    }
    Ok((listen.clone(), routes))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentOptions {
    pub listen: String,
    pub workers: Option<usize>,
}

/// Parses the listener and optional Tokio worker override for the local Agent.
pub fn parse_agent_options() -> Result<AgentOptions> {
    let values: Vec<String> = env::args().skip(1).collect();
    parse_agent_options_from(&values)
}

pub(super) fn parse_agent_options_from(values: &[String]) -> Result<AgentOptions> {
    let usage = "usage: p4-agent LISTEN_ENDPOINT [--workers 1..1024]";
    let Some(listen) = values.first().filter(|value| !value.is_empty()) else {
        return Err(usage.into());
    };
    let workers = match &values[1..] {
        [] => None,
        [flag, value] if flag == "--workers" => Some(
            value
                .parse::<usize>()
                .ok()
                .filter(|value| (1..=1024).contains(value))
                .ok_or(usage)?,
        ),
        _ => return Err(usage.into()),
    };
    Ok(AgentOptions {
        listen: listen.clone(),
        workers,
    })
}
