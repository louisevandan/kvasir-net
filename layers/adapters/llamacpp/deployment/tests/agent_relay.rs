//! Proves the last piece the sealed contract still asked for: not
//! `Registry::try_submit` called directly (`broker_registry.rs`'s job), and
//! not `DeploymentClient::try_submit` called directly (`cross_wire.rs`'s
//! job), but a real *inbound P4 frame* -- the shape OUTER actually sends,
//! addressed to a node the way `Recipient::node` names one -- driven through
//! `p4_agent_core::agent::Agent::dispatch`'s own relay, over a real TCP +
//! HTTP-Upgrade socket, into the real llama v2 submission-stream server
//! (`cross-wire-fixture.ts`, spawned by `support::Fixture`), with the
//! streamed reply landing back on the same agent's own duties the way an
//! OUTER caller would actually observe it.
//!
//! `p4-llamacpp-deployment` does not depend on `p4-agent-core` outside
//! `[dev-dependencies]` -- only this test needs `Agent` at all; the crate's
//! own `DeploymentClient` never does.

mod support;

use p4_adapter::deployment::{Sink as DeploymentSink, Submit};
use p4_agent_core::agent::{Agent, AgentDeploymentSink, Duties, run};
use p4_agent_core::node::payload::Payload;
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_llamacpp_deployment::DeploymentClient;
use p4_llamacpp_deployment::transport::TransportFactory;
use p4_llamacpp_deployment::transport::tcp::TcpTransportFactory;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::Fixture;

/// The one seam this test's `Agent` needs a vocabulary for: reading a
/// prompt-carrying frame's body back into the chat-completion shape
/// `apps/llama`'s submission-stream server requires
/// (`parseRingChatRequest`). Mirrors `p4-service`'s real `Bodies::submission`
/// in shape; kept local rather than an added dependency on that crate,
/// which this one has no other reason to reach.
struct ChatPayload;

impl Payload for ChatPayload {
    fn sequence(&self, _frame: &Frame) -> Option<p4_adapter::Sequence> {
        None
    }

    fn submission(&self, frame: &Frame) -> Option<Submit> {
        let deployment_id = self.deployment(frame)?;
        let deployment_generation = frame.envelope.chain.as_ref()?.current().generation;
        let prompt = String::from_utf8_lossy(&frame.body).into_owned();
        Some(Submit {
            deployment_id,
            deployment_generation,
            submission_id: frame.envelope.route.clone(),
            request: serde_json::json!({
                "messages": [{ "role": "user", "content": prompt }],
                "max_tokens": 32,
                "stream": true,
            }),
        })
    }
}

#[derive(Default)]
struct Collect(Arc<Mutex<Vec<Frame>>>);

impl Duties for Collect {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        self.0.lock().unwrap().push(frame);
    }
}

#[tokio::test]
async fn a_real_inbound_frame_streams_tokens_and_one_terminal_through_the_relay() {
    let fixture = Fixture::spawn();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", 0),
        Arc::new(Collect(Arc::clone(&seen))),
        Arc::new(ChatPayload),
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));

    let relay_sink: Arc<dyn DeploymentSink> =
        Arc::new(AgentDeploymentSink::new(Arc::downgrade(&agent)));
    let factory: Arc<dyn TransportFactory> = Arc::new(TcpTransportFactory::new(fixture.addr));
    let client = DeploymentClient::connect(
        factory,
        relay_sink,
        fixture.deployment_id.clone(),
        fixture.deployment_generation,
        Duration::from_millis(50),
    )
    .expect("connect: TCP + the HTTP Upgrade handshake the server requires");
    agent.deployments().register(
        fixture.deployment_id.clone(),
        client.clone() as Arc<dyn p4_adapter::deployment::Client>,
    );

    let chain = Chain::new(vec![Link {
        address: agent.address().clone(),
        node: "node-1".into(),
        binding: fixture.deployment_id.clone(),
        generation: fixture.deployment_generation,
    }])
    .unwrap();
    agent
        .enqueue(Frame {
            envelope: Envelope {
                target: agent.address().clone(),
                recipient: Recipient::node("node-1"),
                lane: QueueClass::Prefill,
                route: "outer-route-1".into(),
                request_id: "outer-request-1".into(),
                stream_id: "outer-stream-1".into(),
                origin_agent: None,
                return_channel: None,
                ingress_generation: 0,
                event_seq: 0,
                deadline_unix_ms: 0,
                reply_to: Some(agent.address().clone()),
                chain: Some(chain),
            },
            body: b"say hi".to_vec(),
        })
        .expect("the relay's queue accepts the inbound frame");

    // Quiescence rather than a specific frame count or decoded terminal
    // marker: this test deliberately does not depend on `p4-service`'s wire
    // vocabulary (see `ChatPayload`'s own doc), so it has no way to *read*
    // which reply is the terminal. `Settled` fires exactly once, always
    // eventually (`SEALED-CONTRACT.md` §1), so once the reply count stops
    // growing for a full quiet window the stream is over.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut last_len = 0usize;
    let mut quiet_since = tokio::time::Instant::now();
    loop {
        let len = seen.lock().unwrap().len();
        if len != last_len {
            last_len = len;
            quiet_since = tokio::time::Instant::now();
        } else if len > 0 && tokio::time::Instant::now() - quiet_since > Duration::from_millis(400)
        {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!(
                "no reply reached the requester within 10s; seen so far: {:?}",
                seen.lock().unwrap()
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let seen = seen.lock().unwrap();
    assert!(
        seen.len() >= 2,
        "expected at least one streamed token plus a terminal, got {seen:?}"
    );
    for (index, frame) in seen.iter().enumerate() {
        assert_eq!(frame.envelope.lane, QueueClass::Response);
        assert_eq!(frame.envelope.route, "outer-route-1");
        // `event_seq` counts events the requester received, not laps -- each
        // relayed event is exactly one more than the one before it.
        assert_eq!(frame.envelope.event_seq, index as u64 + 1);
    }
    assert!(
        !seen.last().unwrap().body.is_empty(),
        "the terminal reply must carry a real reason, not an empty body"
    );

    client.close();
}
