//! The `Full` half of the relay, over the same real socket as
//! `agent_relay.rs`: an inbound frame that the backend genuinely refuses
//! for capacity has to end up completing anyway, without the requester ever
//! seeing the refusal.
//!
//! Everything about this needed the real server rather than a scripted
//! client. `Full` is not a fixed answer a stub returns on cue -- it is what
//! `coordinator.ts` decides from `capacitySnapshot`. The deployment client
//! retains the pending submission and owns the timed retry; P4 sees only the
//! eventual terminal result. A stub with no capacity and no client ledger
//! proves neither half.
//!
//! The fixture's capacity is 2 (`cross-wire-fixture.ts`), so three
//! submissions at once is the smallest arrangement that produces a real
//! one.

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
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::Fixture;

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
            deadline_unix_ms: frame.envelope.deadline_unix_ms,
            request: serde_json::json!({
                "prompt": prompt,
                "max_tokens": 32,
                "options": "{}",
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
async fn a_submission_the_backend_refuses_for_capacity_still_completes() {
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

    // Three at once against a capacity of two. The third is refused by the
    // real coordinator, on real capacity, with no test knob involved.
    let routes = ["outer-a", "outer-b", "outer-c"];
    for route in routes {
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
                    route: route.into(),
                    request_id: format!("request-{route}"),
                    stream_id: format!("stream-{route}"),
                    origin_agent: None,
                    return_channel: None,
                    ingress_generation: 0,
                    event_seq: 0,
                    // No deadline: this is the retry path, and an expired
                    // carrier is a different test's subject.
                    deadline_unix_ms: 0,
                    reply_to: Some(agent.address().clone()),
                    chain: Some(chain),
                },
                body: b"say hi".to_vec(),
            })
            .expect("the relay's queue accepts the inbound frame");
    }

    // Every route has to reach a terminal. Quiescence is the terminal
    // marker here for the same reason `agent_relay.rs` uses it: this test
    // has no dependency on `p4-service`'s wire vocabulary, so it cannot
    // read which reply is the last one.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        // Taken and released inside this block, never across the await
        // below: a guard held over a yield point can park the whole worker.
        let per_route = count_by_route(&seen.lock().unwrap());
        if routes
            .iter()
            .all(|route| per_route.get(*route).copied().unwrap_or(0) >= 2)
        {
            break;
        }
        if tokio::time::Instant::now() > deadline {
            panic!("not every route completed within 30s; per-route reply counts: {per_route:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Stated as a precondition, not assumed. Without this the test passes
    // unchanged on a backend that had room for all three, which is a green
    // light for a retry path that was never exercised.
    assert!(
        client.full_retry_count() > 0,
        "the backend never actually refused anything, so nothing here \
         exercised the retry this test exists for"
    );

    let replies = seen.lock().unwrap();
    for route in routes {
        let mut stream: Vec<&Frame> = replies
            .iter()
            .filter(|frame| frame.envelope.route == route)
            .collect();
        stream.sort_by_key(|frame| frame.envelope.event_seq);
        assert!(
            stream.len() >= 2,
            "{route} produced no token-plus-terminal stream: {stream:?}"
        );
        // Contiguous from one, per route: a retried submission must not
        // leave a gap or repeat a sequence number the requester already saw.
        for (index, frame) in stream.iter().enumerate() {
            assert_eq!(
                frame.envelope.event_seq,
                index as u64 + 1,
                "{route} has a discontinuous event_seq: {stream:?}"
            );
            assert_eq!(frame.envelope.lane, QueueClass::Response);
        }
        assert!(
            !stream.last().unwrap().body.is_empty(),
            "{route}'s terminal must carry a real reason, not an empty body"
        );
    }
    drop(replies);

    client.close();
}

fn count_by_route(replies: &[Frame]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for frame in replies {
        *counts.entry(frame.envelope.route.clone()).or_insert(0) += 1;
    }
    counts
}
