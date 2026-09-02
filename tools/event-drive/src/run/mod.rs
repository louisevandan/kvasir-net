mod acceptance;
mod config;
mod inference;
mod inference_identity;
#[cfg(test)]
mod inference_identity_tests;
mod load;
mod replies;
mod wire;

pub use config::{AcceptanceConfig, ArrivalWave, ResponseExpectation, RunConfig};
use config::{address, node_endpoint, validate};

use p4_llamacpp_staged_adapter::v2::{
    BatchObservation, NodeRole, OutcomePayload, SESSION_CONTENT_TYPE, SESSION_READY_CONTENT_TYPE,
    SessionCommand, UNLOAD_CONTENT_TYPE, UNLOADED_CONTENT_TYPE, UnloadCommand,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass, OuterEndpoint};
use replies::{ExpectedReply, receive_exact};
use serde::Serialize;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;
use tokio::net::TcpStream;

const CREATE: &str = "application/vnd.p4.node.create-v3+json";
const DELETE: &str = "application/vnd.p4.node.delete-v3+json";
const NODE_RESULT: &str = "application/vnd.p4.node.result-v3+json";

#[derive(Debug, Serialize)]
pub struct RunArtifact {
    pub passed: bool,
    /// Which llama.cpp build every stage of this pipeline reported.
    /// Recorded because a measurement is only attributable to the code
    /// that produced it, and the upstream commit alone does not name that.
    pub build: p4_llamacpp_staged_adapter::v2::BuildIdentity,
    pub acceptance: acceptance::AcceptanceSummary,
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
    pub first_output_ms: Option<u128>,
    pub completed_ms: Option<u128>,
    pub prefill_rows: usize,
    pub decode_rows: usize,
    pub verify_rows: usize,
    pub replay_rows: usize,
    pub prefill_elapsed_ms: Option<u128>,
    pub generation_elapsed_ms: Option<u128>,
    pub logical_prefill_tps: Option<f64>,
    pub logical_generation_tps: Option<f64>,
    pub response: String,
    pub outcomes: Vec<OutcomePayload>,
}

fn per_second(rows: usize, elapsed_ms: u128) -> Option<f64> {
    (elapsed_ms > 0).then(|| rows as f64 * 1_000.0 / elapsed_ms as f64)
}

fn finish_phase_metrics(request: &mut RequestArtifact) {
    let Some(first_output_ms) = request.first_output_ms else {
        return;
    };
    let prefill_elapsed_ms = first_output_ms.saturating_sub(request.arrival_ms);
    request.prefill_elapsed_ms = Some(prefill_elapsed_ms);
    request.logical_prefill_tps = per_second(request.prefill_rows, prefill_elapsed_ms);
    if let Some(completed_ms) = request.completed_ms {
        let generation_elapsed_ms = completed_ms.saturating_sub(first_output_ms);
        request.generation_elapsed_ms = Some(generation_elapsed_ms);
        request.logical_generation_tps = per_second(request.decode_rows, generation_elapsed_ms);
    }
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

    let mut create_replies = Vec::with_capacity(config.nodes.len());
    for node in &config.nodes {
        let payload = serde_json::json!({
            "node_id":node.node,"node_generation":node.generation,"adapter_kind":"llamacpp",
            "queue_capacity":65536,"completion_capacity":65536,
        });
        let event = sender.event(
            Endpoint::agent(Address::from_str(&node.agent)?),
            EventClass::Control,
            CREATE,
            serde_json::to_vec(&payload)?,
            "create",
        );
        create_replies.push(ExpectedReply::from_request(&event));
        wire.send(event).await?;
    }
    receive_exact(
        &mut wire,
        NODE_RESULT,
        create_replies,
        "create",
        config.timeout_ms,
    )
    .await?;

    let build = load::drive(&config, &mut wire, &mut sender).await?;

    let first = address(&config.nodes[0]);
    let mut session_replies = Vec::with_capacity(config.nodes.len());
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
        let event = sender.event(
            node_endpoint(node)?,
            EventClass::Control,
            SESSION_CONTENT_TYPE,
            serde_json::to_vec(&command)?,
            "session",
        );
        session_replies.push(ExpectedReply::from_request(&event));
        wire.send(event).await?;
    }
    receive_exact(
        &mut wire,
        SESSION_READY_CONTENT_TYPE,
        session_replies,
        "session",
        config.timeout_ms,
    )
    .await?;

    if config.pre_inference_hold_ms > 0 {
        println!(
            "P4_EVENT_GATE_LOADED nodes={} pre_inference_hold_ms={}",
            config.nodes.len(),
            config.pre_inference_hold_ms
        );
        std::io::stdout().flush()?;
        tokio::time::sleep(Duration::from_millis(config.pre_inference_hold_ms)).await;
    }

    let run = inference::drive(&config, &mut wire, &mut sender).await?;

    let mut unload_replies = Vec::with_capacity(config.nodes.len());
    for node in &config.nodes {
        let event = sender.event(
            node_endpoint(node)?,
            EventClass::Control,
            UNLOAD_CONTENT_TYPE,
            serde_json::to_vec(&UnloadCommand {
                load_generation: config.load_generation,
            })?,
            "unload",
        );
        unload_replies.push(ExpectedReply::from_request(&event));
        wire.send(event).await?;
    }
    receive_exact(
        &mut wire,
        UNLOADED_CONTENT_TYPE,
        unload_replies,
        "unload",
        config.timeout_ms,
    )
    .await?;
    let mut delete_replies = Vec::with_capacity(config.nodes.len());
    for node in &config.nodes {
        let payload = serde_json::json!({
            "node_id":node.node,"node_generation":node.generation
        });
        let event = sender.event(
            Endpoint::agent(Address::from_str(&node.agent)?),
            EventClass::Control,
            DELETE,
            serde_json::to_vec(&payload)?,
            "delete",
        );
        delete_replies.push(ExpectedReply::from_request(&event));
        wire.send(event).await?;
    }
    receive_exact(
        &mut wire,
        NODE_RESULT,
        delete_replies,
        "delete",
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
    for request in &mut requests {
        finish_phase_metrics(request);
    }
    let acceptance = acceptance::evaluate(&config, &requests);
    let structurally_complete = run.error.is_none()
        && run.completed_count == run.request_count
        && run.released_count == run.request_count;
    Ok(RunArtifact {
        passed: structurally_complete && acceptance.passed,
        build,
        acceptance,
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

    #[test]
    fn phase_metrics_use_first_output_as_the_prefill_decode_boundary() {
        let mut request = RequestArtifact {
            request_id: "request".into(),
            prompt: "prompt".into(),
            arrival_ms: 10,
            first_output_ms: Some(210),
            completed_ms: Some(1_210),
            prefill_rows: 500,
            decode_rows: 100,
            verify_rows: 0,
            replay_rows: 0,
            prefill_elapsed_ms: None,
            generation_elapsed_ms: None,
            logical_prefill_tps: None,
            logical_generation_tps: None,
            response: String::new(),
            outcomes: Vec::new(),
        };
        finish_phase_metrics(&mut request);
        assert_eq!(request.prefill_elapsed_ms, Some(200));
        assert_eq!(request.generation_elapsed_ms, Some(1_000));
        assert_eq!(request.logical_prefill_tps, Some(2_500.0));
        assert_eq!(request.logical_generation_tps, Some(100.0));
    }
}
