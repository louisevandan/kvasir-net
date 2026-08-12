//! OpenAI-compatible HTTP/SSE execution bridge.

use crate::application::options;
use crate::application::response::RouteResponder;
use crate::domain::config::AdapterConfig;
use crate::infrastructure::http::chunked::ChunkedReader;
use crate::infrastructure::http::endpoint::endpoint;
use p4_protocol::{ExecutionDone, ExecutionRequest, ExecutionToken, Message};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

pub(crate) fn execute(
    route_id: &str,
    responses: &RouteResponder,
    request: ExecutionRequest,
    config: &AdapterConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let binding = (request.node_id.clone(), request.binding_id.clone());
    if config
        .bindings
        .read()
        .map_err(|_| "binding registry lock poisoned")?
        .get(&binding)
        != Some(&request.runtime_generation)
    {
        responses.emit(Message::Error {
            request_id: request.request_id,
            detail: "llama.cpp binding is not ready for this runtime generation".into(),
        })?;
        return Ok(());
    }
    println!(
        "P4_LLAMACPP_EXECUTE request={} session={} max_tokens={}",
        request.request_id, request.session_id, request.max_tokens
    );
    let body = match options::request_body(&request, &config.model) {
        Ok(body) => body,
        Err(detail) => {
            responses.emit(Message::Error {
                request_id: request.request_id,
                detail: format!("invalid llama.cpp execution options: {detail}"),
            })?;
            return Ok(());
        }
    };
    let (host, port) = endpoint(&config.endpoint)?;
    let mut upstream = TcpStream::connect((host.as_str(), port))?;
    upstream.set_nodelay(true)?;
    config
        .active_upstreams
        .lock()
        .map_err(|_| "active upstream registry lock poisoned")?
        .insert(route_id.into(), upstream.try_clone()?);
    let _active = ActiveUpstream {
        route_id: route_id.into(),
        config: config.clone(),
    };
    let request_head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        host,
        port,
        body.len()
    );
    upstream.write_all(request_head.as_bytes())?;
    upstream.write_all(body.as_bytes())?;
    upstream.flush()?;
    let mut reader = BufReader::new(upstream);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    if !status.contains(" 200 ") {
        let mut rest = String::new();
        reader.read_to_string(&mut rest)?;
        responses.emit(Message::Error {
            request_id: request.request_id,
            detail: format!(
                "llama-server rejected request: {} {}",
                status.trim(),
                rest.trim()
            ),
        })?;
        return Ok(());
    }
    let mut chunked = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if line.to_ascii_lowercase().starts_with("transfer-encoding:")
            && line.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    let emitted = if chunked {
        let chunked_reader = ChunkedReader::new(reader);
        let mut events = BufReader::new(chunked_reader);
        forward_sse(&mut events, responses, &request)?
    } else {
        forward_sse(&mut reader, responses, &request)?
    };
    responses.emit(Message::Done(ExecutionDone {
        controller_id: request.controller_id,
        node_id: request.node_id,
        request_id: request.request_id,
        session_id: request.session_id,
        reason: "stop".into(),
        generated_tokens: emitted,
    }))?;
    Ok(())
}

fn forward_sse(
    reader: &mut impl BufRead,
    responses: &RouteResponder,
    request: &ExecutionRequest,
) -> Result<u32, Box<dyn std::error::Error>> {
    let mut emitted = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let Some(data) = line.trim().strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let event: Value = serde_json::from_str(data)?;
        let text = event
            .pointer("/choices/0/delta/content")
            .and_then(Value::as_str)
            .or_else(|| event.pointer("/choices/0/text").and_then(Value::as_str))
            .unwrap_or("");
        if !text.is_empty() {
            responses.emit(Message::Token(ExecutionToken {
                controller_id: request.controller_id.clone(),
                node_id: request.node_id.clone(),
                request_id: request.request_id.clone(),
                session_id: request.session_id.clone(),
                phase: request.phase.clone(),
                position: request.position,
                index: emitted,
                text: text.into(),
            }))?;
            emitted += 1;
        }
    }
    Ok(emitted)
}

struct ActiveUpstream {
    route_id: String,
    config: AdapterConfig,
}

impl Drop for ActiveUpstream {
    fn drop(&mut self) {
        if let Ok(mut streams) = self.config.active_upstreams.lock() {
            streams.remove(&self.route_id);
        }
    }
}
