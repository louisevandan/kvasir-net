mod inference;
mod wire;

use p4_llamacpp_staged_adapter::v2::{
    BatchObservation, ERROR_CONTENT_TYPE, LOAD_CONTENT_TYPE, LOADED_CONTENT_TYPE, LoadCommand,
    NodeAddress, NodeRole, OutcomePayload, SESSION_CONTENT_TYPE, SESSION_READY_CONTENT_TYPE,
    SessionCommand, UNLOAD_CONTENT_TYPE, UNLOADED_CONTENT_TYPE, UnloadCommand,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, OuterEndpoint};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

const CREATE: &str = "application/vnd.p4.node.create-v2+json";
const DELETE: &str = "application/vnd.p4.node.delete-v2+json";
const NODE_RESULT: &str = "application/vnd.p4.node.result-v2+json";

#[derive(Clone, Debug, Deserialize)]
pub struct RunConfig {
    pub ingress_agent: String,
    pub channel: String,
    pub connection_generation: u64,
    pub load_generation: u64,
    pub session_id: String,
    pub request_id: String,
    pub nodes: Vec<NodeConfig>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub prompts: Vec<String>,
    pub max_tokens: u32,
    #[serde(default = "default_waves")]
    pub waves: Vec<ArrivalWave>,
    #[serde(default)]
    pub options: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ArrivalWave {
    pub after_ms: u64,
    pub count: usize,
}

fn default_waves() -> Vec<ArrivalWave> {
    vec![ArrivalWave {
        after_ms: 0,
        count: 1,
    }]
}

#[derive(Clone, Debug, Deserialize)]
pub struct NodeConfig {
    pub agent: String,
    pub node: String,
    pub binary: String,
    pub endpoint: String,
    pub plan: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: Vec<(String, String)>,
    pub n_batch: usize,
    pub n_ubatch: usize,
    pub context_size: usize,
    pub sequence_capacity: u32,
}

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
    pub response: String,
    pub outcomes: Vec<OutcomePayload>,
}

fn default_timeout() -> u64 {
    600_000
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
            "node_id":node.node,"adapter_kind":"llamacpp",
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
        let payload = serde_json::json!({"node_id":node.node});
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

fn validate(config: &RunConfig) -> Result<(), &'static str> {
    let request_count: usize = config.waves.iter().map(|wave| wave.count).sum();
    if config.nodes.len() < 2
        || (config.prompt.is_empty()
            && (config.prompts.len() != request_count
                || config.prompts.iter().any(String::is_empty)))
        || (!config.prompts.is_empty() && config.prompts.len() != request_count)
        || config.max_tokens == 0
        || config.channel.is_empty()
        || config.connection_generation == 0
        || config.load_generation == 0
        || config.waves.is_empty()
        || config.waves[0].after_ms != 0
        || config.waves.iter().any(|wave| wave.count == 0)
        || config
            .nodes
            .iter()
            .any(|node| node.n_batch == 0 || node.n_ubatch == 0 || node.n_ubatch > node.n_batch)
        || config
            .waves
            .windows(2)
            .any(|pair| pair[0].after_ms >= pair[1].after_ms)
    {
        return Err("run requires nodes, prompt, token budget, OUTER identity and ordered waves");
    }
    Ok(())
}

fn address(node: &NodeConfig) -> NodeAddress {
    NodeAddress {
        agent: node.agent.clone(),
        node: node.node.clone(),
    }
}

fn node_endpoint(node: &NodeConfig) -> Result<Endpoint, p4_protocol::ProtocolError> {
    Ok(Endpoint::node(
        Address::from_str(&node.agent)?,
        node.node.clone(),
    ))
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
                event_id: format!("outer:{}:{sequence}", self.outer.channel),
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
