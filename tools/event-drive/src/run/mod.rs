mod acceptance;
mod config;
#[cfg(test)]
mod consumer_budget_boundary_tests;
mod evidence_ledger;
mod inference;
pub use evidence_ledger::SubmittedAuthority;
mod inference_identity;
#[cfg(test)]
mod inference_identity_tests;
mod load;
mod output_budget;
mod release_ledger;
mod replies;
mod wire;

pub use config::{AcceptanceConfig, ArrivalWave, ResponseExpectation, RunConfig};
use config::{address, node_endpoint, validate};

use p4_llamacpp_staged_adapter::v2::{
    BatchObservation, OutcomePayload, SESSION_CONTENT_TYPE, SESSION_READY_CONTENT_TYPE,
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

/// One node's span for one batch, with which node it came from.
#[derive(Debug, Serialize)]
pub struct StageSpanArtifact {
    pub node: usize,
    #[serde(flatten)]
    pub span: p4_llamacpp_staged_adapter::v2::StageSpan,
}

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
    pub stage_spans: Vec<StageSpanArtifact>,
    pub elapsed_ms: u128,
    pub telemetry_complete_elapsed_ms: Option<u128>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RequestArtifact {
    pub request_id: String,
    pub submission_event_id: String,
    pub submission_authority: Option<SubmittedAuthority>,
    pub issued_work: Option<p4_llamacpp_staged_adapter::v2::IssuedWorkProof>,
    pub release_member: Option<p4_llamacpp_staged_adapter::v2::ReleaseMember>,
    pub released: bool,
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

    let mut session_replies = Vec::with_capacity(config.nodes.len());
    for event in session_events(&config, &mut sender)? {
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
    // drive installs the validated per-request observation totals once.
    // Re-aggregating here would double count the same physical work.
    for request in &mut requests {
        finish_phase_metrics(request);
    }
    let acceptance = acceptance::evaluate(&config, &requests);
    let structurally_complete = run.error.is_none()
        && run.telemetry_complete_elapsed_ms.is_some()
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
        stage_spans: run.stage_spans,
        elapsed_ms: run.elapsed_ms,
        telemetry_complete_elapsed_ms: run.telemetry_complete_elapsed_ms,
        error: run.error,
    })
}

/// The actual execute path uses this builder. It declares the same complete
/// logical topology to each recipient, changing only that recipient's index.
/// Physical device placement and transport delivery are not performed here.
fn session_events(
    config: &RunConfig,
    sender: &mut Sender,
) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let stages = config.nodes.iter().map(address).collect::<Vec<_>>();
    let mut events = Vec::with_capacity(config.nodes.len());
    for (index, node) in config.nodes.iter().enumerate() {
        let command = SessionCommand {
            load_generation: config.load_generation,
            session_id: config.session_id.clone(),
            stages: stages.clone(),
            stage_index: index,
        };
        events.push(sender.event(
            node_endpoint(node)?,
            EventClass::Control,
            SESSION_CONTENT_TYPE,
            serde_json::to_vec(&command)?,
            "session",
        ));
    }
    Ok(events)
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

    fn three_stage_session_config() -> RunConfig {
        let nodes = [
            ("tcp://127.0.0.1:53101", "head", 3),
            ("tcp://127.0.0.2:53102", "middle", 7),
            ("tcp://127.0.0.3:53103", "tail", 11),
        ]
        .into_iter()
        .map(|(agent, node, generation)| config::NodeConfig {
            agent: agent.into(),
            node: node.into(),
            generation,
            binary: "not-started".into(),
            endpoint: "tcp://127.0.0.1:53999".into(),
            plan: "not-loaded".into(),
            args: Vec::new(),
            environment: Vec::new(),
            n_batch: 8,
            n_ubatch: 8,
            context_size: 16,
            total_context_size: 16,
            sequence_capacity: 1,
        })
        .collect();
        RunConfig {
            ingress_agent: "tcp://127.0.0.1:53100".into(),
            channel: "session-builder".into(),
            connection_generation: 17,
            load_generation: 23,
            session_id: "ordered-pipeline".into(),
            request_id: "not-issued".into(),
            nodes,
            prompt: "not-inferred".into(),
            prompts: Vec::new(),
            session_key_template: String::new(),
            max_tokens: 2,
            waves: vec![ArrivalWave {
                after_ms: 0,
                count: 1,
            }],
            options: String::new(),
            pre_inference_hold_ms: 0,
            acceptance: AcceptanceConfig::default(),
            timeout_ms: 1000,
        }
    }

    fn session_builder_fixture() -> (Vec<Event>, OuterEndpoint) {
        let config = three_stage_session_config();
        validate(&config).unwrap();
        let outer = OuterEndpoint {
            ingress_agent: Address::tcp("127.0.0.1", 53100),
            channel: "session-builder".into(),
            connection_generation: 17,
        };
        let mut sender = Sender::new(outer.clone());
        // LOAD/CREATE already consume IDs in execute. The SESSION builder
        // must preserve its caller's stream, not create a new Sender.
        let prior = sender.event(
            Endpoint::Agent(outer.ingress_agent.clone()),
            EventClass::Control,
            CREATE,
            Vec::new(),
            "create",
        );
        let events = session_events(&config, &mut sender).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(sender.sequence, 5);
        assert!(
            events
                .iter()
                .all(|event| event.envelope.event_id != prior.envelope.event_id)
        );
        (events, outer)
    }

    #[test]
    fn session_builder_declares_the_full_ordered_v4_pipeline_to_each_recipient() {
        let (events, outer) = session_builder_fixture();
        let stages = serde_json::json!([
            {"agent":"tcp://127.0.0.1:53101","node":"head","generation":3},
            {"agent":"tcp://127.0.0.2:53102","node":"middle","generation":7},
            {"agent":"tcp://127.0.0.3:53103","node":"tail","generation":11}
        ]);
        let roles = [
            p4_llamacpp_staged_adapter::v2::NodeRole::First,
            p4_llamacpp_staged_adapter::v2::NodeRole::Middle,
            p4_llamacpp_staged_adapter::v2::NodeRole::Last,
        ];
        let mut ids = std::collections::BTreeSet::new();
        for (index, event) in events.iter().enumerate() {
            let bytes = p4_protocol::event::encode(event).unwrap();
            assert_eq!(p4_protocol::event::decode(&bytes).unwrap(), *event);
            assert_eq!(
                event.envelope.payload_content_type,
                "application/vnd.p4.llamacpp.session-v4+json"
            );
            assert_eq!(event.envelope.source, Endpoint::Outer(outer.clone()));
            assert_eq!(event.envelope.return_route, Some(outer.clone()));
            assert_eq!(event.envelope.class, EventClass::Control);
            assert_eq!(event.envelope.correlation_id, "session");
            assert_eq!(event.envelope.causation_id, None);
            assert_eq!(event.envelope.adapter_kind.as_deref(), Some("llamacpp"));
            assert_eq!(event.envelope.sequence, index as u64 + 2);
            assert!(ids.insert(event.envelope.event_id.clone()));
            let expected_stage = &stages[index];
            assert_eq!(
                event.envelope.target,
                Endpoint::node(
                    Address::from_str(expected_stage["agent"].as_str().unwrap()).unwrap(),
                    expected_stage["node"].as_str().unwrap(),
                    expected_stage["generation"].as_u64().unwrap()
                )
            );
            let body: serde_json::Value = serde_json::from_slice(&event.payload).unwrap();
            // Exact JSON excludes old independent role/first/next fields and
            // prevents head->tail shortcutting from erasing the middle stage.
            assert_eq!(
                body,
                serde_json::json!({
                    "load_generation":23,"session_id":"ordered-pipeline",
                    "stages":stages,"stage_index":index
                })
            );
            let command: SessionCommand = serde_json::from_slice(&event.payload).unwrap();
            command.validate().unwrap();
            assert_eq!(command.role(), roles[index]);
        }
    }

    fn session_ready_event(request: &Event, index: usize) -> Event {
        let sources = [
            Endpoint::node(Address::tcp("127.0.0.1", 53101), "head", 3),
            Endpoint::node(Address::tcp("127.0.0.2", 53102), "middle", 7),
            Endpoint::node(Address::tcp("127.0.0.3", 53103), "tail", 11),
        ];
        let mut reply = request.clone();
        reply.envelope.event_id = format!("fixture-ready-{index}");
        reply.envelope.causation_id = Some(request.envelope.event_id.clone());
        reply.envelope.source = sources[index].clone();
        reply.envelope.target = request.envelope.source.clone();
        reply.envelope.payload_content_type =
            "application/vnd.p4.llamacpp.session-ready-v4+json".into();
        reply.payload = Vec::new();
        reply
    }

    // Uses the production reply consumer over bounded in-memory EventWire.
    // No execute TCP/bootstrap, stage process, model or GPU is exercised.
    async fn consume_session_replies(
        events: &[Event],
        replies: Vec<Event>,
    ) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
        let (client, peer) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(client);
        let (peer_reader, peer_writer) = tokio::io::split(peer);
        let mut wire = wire::EventWire::new(reader, writer);
        let producer = tokio::spawn(async move {
            let mut peer = wire::EventWire::new(peer_reader, peer_writer);
            for event in replies {
                peer.send(event).await.unwrap();
            }
        });
        let result = receive_exact(
            &mut wire,
            SESSION_READY_CONTENT_TYPE,
            events.iter().map(ExpectedReply::from_request).collect(),
            "session",
            1000,
        )
        .await;
        producer.await.unwrap();
        result
    }

    #[tokio::test]
    async fn session_builder_replies_match_each_original_event_and_stage_in_any_order() {
        let (events, _) = session_builder_fixture();
        assert_eq!(
            SESSION_READY_CONTENT_TYPE,
            "application/vnd.p4.llamacpp.session-ready-v4+json"
        );
        let replies = (0..3)
            .rev()
            .map(|index| session_ready_event(&events[index], index))
            .collect();
        assert_eq!(
            consume_session_replies(&events, replies)
                .await
                .unwrap()
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn session_builder_cannot_accept_another_stage_or_repeated_submission_reply() {
        for duplicate in [false, true] {
            let (events, _) = session_builder_fixture();
            let first = session_ready_event(&events[0], 0);
            let mut second = session_ready_event(&events[1], 1);
            if duplicate {
                second.envelope.causation_id = first.envelope.causation_id.clone();
            } else {
                second.envelope.source = first.envelope.source.clone();
            }
            let error = consume_session_replies(&events, vec![first, second])
                .await
                .err()
                .expect("reply authority must not be transferable")
                .to_string();
            assert!(
                error.contains(if duplicate {
                    "duplicate or unknown causation_id"
                } else {
                    "source mismatch"
                }),
                "wrong refusal: {error}"
            );
        }
    }

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
            submission_event_id: "sent-request".into(),
            submission_authority: None,
            issued_work: None,
            release_member: None,
            released: false,
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
