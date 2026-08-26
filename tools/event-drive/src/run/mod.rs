mod config;
mod inference;
mod wire;

pub use config::{ArrivalWave, RunConfig};
use config::{address, node_endpoint, validate};

use p4_llamacpp_staged_adapter::v2::{
    BatchObservation, ERROR_CONTENT_TYPE, LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE, LoadCommand,
    NodeRole, OutcomePayload, SESSION_CONTENT_TYPE, SESSION_READY_CONTENT_TYPE, SessionCommand,
    UNLOAD_CONTENT_TYPE, UNLOADED_CONTENT_TYPE, UnloadCommand,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, OuterEndpoint};
use serde::Serialize;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

const CREATE: &str = "application/vnd.p4.node.create-v3+json";
const DELETE: &str = "application/vnd.p4.node.delete-v3+json";
const NODE_RESULT: &str = "application/vnd.p4.node.result-v3+json";

#[derive(Debug, Serialize)]
pub struct RunArtifact {
    pub passed: bool,
    pub prompt: String,
    pub response: String,
    pub outcomes: Vec<OutcomePayload>,
    pub request_count: usize,
    pub completed_count: usize,
    pub released_count: usize,
    pub requests: Vec<RequestArtifact>,
    pub batch_observations: Vec<BatchObservation>,
    pub elapsed_ms: u128,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RequestArtifact {
    pub request_id: String,
    pub prompt: String,
    pub arrival_ms: u128,
    pub completed_ms: Option<u128>,
    pub prefill_rows: usize,
    pub decode_rows: usize,
    pub verify_rows: usize,
    pub replay_rows: usize,
    pub response: String,
    pub outcomes: Vec<OutcomePayload>,
}

pub async fn execute(config: RunConfig) -> Result<RunArtifact, Box<dyn std::error::Error>> {
    validate(&config)?;
    let ingress = Address::from_str(&config.ingress_agent)?;
    let stream = TcpStream::connect((ingress.host.as_str(), ingress.port)).await?;
    stream.set_nodelay(true)?;
    let (reader, writer) = stream.into_split();
    let mut wire = wire::EventWire::new(reader, writer);
    let outer = OuterEndpoint {
        ingress_agent: ingress.clone(),
        channel: config.channel.clone(),
        connection_generation: config.connection_generation,
    };
    let mut sender = Sender::new(outer);

    for node in &config.nodes {
        let payload = serde_json::json!({
            "node_id":node.node,"node_generation":node.generation,"adapter_kind":"llamacpp",
            "queue_capacity":65536,"completion_capacity":65536,
        });
        wire.send(sender.event(
            Endpoint::agent(Address::from_str(&node.agent)?),
            EventClass::Control,
            CREATE,
            serde_json::to_vec(&payload)?,
            "create",
        ))
        .await?;
    }
    receive_exact(
        &mut wire,
        NODE_RESULT,
        config.nodes.len(),
        config.timeout_ms,
    )
    .await?;

    for node in &config.nodes {
        let command = LoadCommand {
            load_generation: config.load_generation,
            binary: node.binary.clone(),
            endpoint: node.endpoint.clone(),
            plan: node.plan.clone(),
            args: node.args.clone(),
            environment: node.environment.clone(),
            n_batch: node.n_batch,
            n_ubatch: node.n_ubatch,
            context_size: node.context_size,
            total_context_size: node.total_context_size,
            sequence_capacity: node.sequence_capacity,
            ready_timeout_ms: config.timeout_ms,
            io_timeout_ms: config.timeout_ms,
        };
        wire.send(sender.event(
            node_endpoint(node)?,
            EventClass::Control,
            LOAD_CONTENT_TYPE,
            serde_json::to_vec(&command)?,
            "load",
        ))
        .await?;
    }
    receive_exact(
        &mut wire,
        LOADED_CONTENT_TYPE,
        config.nodes.len(),
        config.timeout_ms,
    )
    .await?;

    let first = address(&config.nodes[0]);
    for (index, node) in config.nodes.iter().enumerate() {
        let last = index + 1 == config.nodes.len();
        let command = SessionCommand {
            load_generation: config.load_generation,
            session_id: config.session_id.clone(),
            role: if index == 0 {
                NodeRole::First
            } else if last {
                NodeRole::Last
            } else {
                NodeRole::Middle
            },
            next: (!last).then(|| address(&config.nodes[index + 1])),
            first: first.clone(),
        };
        wire.send(sender.event(
            node_endpoint(node)?,
            EventClass::Control,
            SESSION_CONTENT_TYPE,
            serde_json::to_vec(&command)?,
            "session",
        ))
        .await?;
    }
    receive_exact(
        &mut wire,
        SESSION_READY_CONTENT_TYPE,
        config.nodes.len(),
        config.timeout_ms,
    )
    .await?;

    let run = inference::drive(&config, &mut wire, &mut sender).await?;

    for node in &config.nodes {
        wire.send(sender.event(
            node_endpoint(node)?,
            EventClass::Control,
            UNLOAD_CONTENT_TYPE,
            serde_json::to_vec(&UnloadCommand {
                load_generation: config.load_generation,
            })?,
            "unload",
        ))
        .await?;
    }
    receive_exact(
        &mut wire,
        UNLOADED_CONTENT_TYPE,
        config.nodes.len(),
        config.timeout_ms,
    )
    .await?;
    for node in &config.nodes {
        let payload = serde_json::json!({
            "node_id":node.node,"node_generation":node.generation
        });
        wire.send(sender.event(
            Endpoint::agent(Address::from_str(&node.agent)?),
            EventClass::Control,
            DELETE,
            serde_json::to_vec(&payload)?,
            "delete",
        ))
        .await?;
    }
    receive_exact(
        &mut wire,
        NODE_RESULT,
        config.nodes.len(),
        config.timeout_ms,
    )
    .await?;

    let mut requests = run.requests;
    for observation in &run.batch_observations {
        for batch in &observation.physical_batches {
            for measured in &batch.requests {
                let request = requests
                    .iter_mut()
                    .find(|request| request.request_id == measured.request_id)
                    .ok_or("batch observation references an unknown request")?;
                request.prefill_rows += measured.prefill_rows;
                request.decode_rows += measured.decode_rows;
                request.verify_rows += measured.verify_rows;
                request.replay_rows += measured.replay_rows;
            }
        }
    }
    Ok(RunArtifact {
        passed: run.error.is_none()
            && run.completed_count == run.request_count
            && run.released_count == run.request_count
            && requests.iter().all(|request| !request.response.is_empty()),
        prompt: config.prompt,
        response: requests
            .first()
            .map(|value| value.response.clone())
            .unwrap_or_default(),
        outcomes: requests
            .first()
            .map(|value| value.outcomes.clone())
            .unwrap_or_default(),
        request_count: run.request_count,
        completed_count: run.completed_count,
        released_count: run.released_count,
        requests,
        batch_observations: run.batch_observations,
        elapsed_ms: run.elapsed_ms,
        error: run.error,
    })
}

async fn receive_exact<R, W>(
    wire: &mut wire::EventWire<R, W>,
    content_type: &str,
    count: usize,
    timeout_ms: u64,
) -> Result<(), Box<dyn std::error::Error>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut received = 0;
    while received < count {
        let event = wire.receive(deadline).await?;
        if event.envelope.payload_content_type == ERROR_CONTENT_TYPE {
            return Err(String::from_utf8_lossy(&event.payload).into_owned().into());
        }
        if event.envelope.payload_content_type == content_type {
            if content_type == NODE_RESULT {
                let result: serde_json::Value = serde_json::from_slice(&event.payload)?;
                if result.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
                    let detail = result
                        .get("detail")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("node control failed");
                    return Err(detail.to_owned().into());
                }
            }
            received += 1;
        }
    }
    Ok(())
}

pub(super) struct Sender {
    outer: OuterEndpoint,
    sequence: u64,
}

impl Sender {
    fn new(outer: OuterEndpoint) -> Self {
        Self { outer, sequence: 1 }
    }

    pub(super) fn event(
        &mut self,
        target: Endpoint,
        class: EventClass,
        content_type: &str,
        payload: Vec<u8>,
        correlation: &str,
    ) -> Event {
        let sequence = self.sequence;
        self.sequence += 1;
        Event {
            envelope: Envelope {
                protocol_version: Envelope::VERSION,
                event_id: outer_event_id(&self.outer, sequence),
                correlation_id: correlation.into(),
                causation_id: None,
                source: Endpoint::Outer(self.outer.clone()),
                target,
                return_route: Some(self.outer.clone()),
                class,
                sequence,
                deadline_unix_ms: None,
                adapter_kind: Some("llamacpp".into()),
                payload_content_type: content_type.into(),
            },
            payload,
        }
    }
}

fn outer_event_id(outer: &OuterEndpoint, sequence: u64) -> String {
    format!(
        "outer:{}:{}:{}:{sequence}",
        outer.ingress_agent, outer.channel, outer.connection_generation
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_event_identity_includes_the_ingress_agent() {
        let first = OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 52001),
            channel: "shared".into(),
            connection_generation: 1,
        };
        let second = OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 52002),
            ..first.clone()
        };
        assert_ne!(outer_event_id(&first, 1), outer_event_id(&second, 1));
    }
}
