use super::*;
use crate::transport::inbox;
use p4_adapter::{Distribution, Event, EventSink, Outcome, Sequence, Work};
use p4_protocol::frame::{self};
use p4_protocol::{Chain, Envelope, Link, Recipient};
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tokio::net::TcpListener;

/// Answers every hop immediately, finishing each sequence.
struct Instant;

impl Adapter for Instant {
    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        let Work::Hop(hop) = work else { return };
        events.raise(Event::HopComplete {
            outcomes: hop
                .sequences
                .iter()
                .map(|sequence| Outcome {
                    sequence: sequence.sequence.clone(),
                    text: "t".into(),
                    position: sequence.position + 1,
                    stop: Some("stop".into()),
                })
                .collect(),
            deployment: hop.deployment,
        });
    }
}

struct Bodies;

impl Payload for Bodies {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        Some(Sequence {
            sequence: frame.envelope.route.clone(),
            position: 0,
            prompt: Some(String::from_utf8_lossy(&frame.body).into_owned()),
            remaining: 1,
            options: "{}".into(),
        })
    }
}

#[derive(Default)]
struct Collect(Arc<StdMutex<Vec<Frame>>>);

impl Duties for Collect {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        self.0.lock().unwrap().push(frame);
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap()
}

async fn agent_at(port: u16, seen: Arc<StdMutex<Vec<Frame>>>) -> Arc<Agent> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    let bound = listener.local_addr().unwrap().port();
    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", bound),
        Arc::new(Collect(seen)),
        Arc::new(Bodies),
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(inbox::serve(listener, agent.queue(), 64));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

fn control(target: Address) -> Frame {
    Frame {
        envelope: Envelope {
            target,
            recipient: Recipient::Agent,
            lane: QueueClass::Control,
            route: "route-1".into(),
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: b"hello".to_vec(),
    }
}

#[test]
fn a_message_for_this_agent_reaches_its_duties() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_at(0, Arc::clone(&seen)).await;
        agent.enqueue(control(agent.address().clone())).unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(seen.lock().unwrap().len(), 1);
    });
}

#[test]
fn a_message_for_another_agent_is_carried_there_over_a_socket() {
    // The relay, end to end and across a real connection: one agent receives a
    // frame addressed to another and forwards it without opening the body.
    runtime().block_on(async {
        let far_seen = Arc::new(StdMutex::new(Vec::new()));
        let far = agent_at(0, Arc::clone(&far_seen)).await;
        let near_seen = Arc::new(StdMutex::new(Vec::new()));
        let near = agent_at(0, Arc::clone(&near_seen)).await;

        near.enqueue(control(far.address().clone())).unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(
            near_seen.lock().unwrap().len(),
            0,
            "the near agent kept nothing"
        );
        assert_eq!(
            far_seen.lock().unwrap().len(),
            1,
            "the far agent received it"
        );
        assert_eq!(far_seen.lock().unwrap()[0].body, b"hello");
    });
}

#[test]
fn work_for_a_node_this_agent_does_not_have_is_answered_rather_than_dropped() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let agent = agent_at(0, Arc::clone(&seen)).await;

        let mut frame = control(agent.address().clone());
        frame.envelope.recipient = Recipient::node("absent");
        frame.envelope.reply_to = Some(agent.address().clone());
        agent.enqueue(frame).unwrap();

        tokio::time::sleep(Duration::from_millis(150)).await;
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body, b"no such node on this agent");
    });
}

#[test]
fn an_inference_crosses_two_agents_and_comes_back() {
    // The whole path: a chain whose two links live on different machines, work
    // hopping between them over sockets, and the last node replying to the
    // address the request named.
    runtime().block_on(async {
        let outer_seen = Arc::new(StdMutex::new(Vec::new()));
        let outer = agent_at(0, Arc::clone(&outer_seen)).await;
        let second = agent_at(0, Arc::new(StdMutex::new(Vec::new()))).await;
        let first = agent_at(0, Arc::new(StdMutex::new(Vec::new()))).await;

        first.create_node("n0", Arc::new(Instant), 4).await;
        second.create_node("n1", Arc::new(Instant), 4).await;

        let chain = Chain::new(vec![
            Link {
                address: first.address().clone(),
                node: "n0".into(),
                binding: "deployment".into(),
                generation: 1,
            },
            Link {
                address: second.address().clone(),
                node: "n1".into(),
                binding: "deployment".into(),
                generation: 1,
            },
        ])
        .unwrap();

        first
            .enqueue(Frame {
                envelope: Envelope {
                    target: first.address().clone(),
                    recipient: Recipient::node("n0"),
                    lane: QueueClass::Prefill,
                    route: "inference-1".into(),
                    deadline_unix_ms: 0,
                    reply_to: Some(outer.address().clone()),
                    chain: Some(chain),
                },
                body: b"prompt".to_vec(),
            })
            .unwrap();

        tokio::time::sleep(Duration::from_millis(600)).await;
        let seen = outer_seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "one terminal came back to the caller");
        assert_eq!(seen[0].envelope.route, "inference-1");
        assert_eq!(seen[0].envelope.lane, QueueClass::Response);
    });
}

#[test]
fn a_frame_survives_the_wire_between_two_agents_unchanged() {
    runtime().block_on(async {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let far = agent_at(0, Arc::clone(&seen)).await;
        let near = agent_at(0, Arc::new(StdMutex::new(Vec::new()))).await;

        let mut sent = control(far.address().clone());
        sent.envelope.deadline_unix_ms = 1_800_000_000_000;
        sent.body = vec![0, 159, 146, 150];
        near.enqueue(sent.clone()).unwrap();

        tokio::time::sleep(Duration::from_millis(300)).await;
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body, sent.body, "a body is bytes, not text");
        assert_eq!(
            seen[0].envelope.deadline_unix_ms,
            sent.envelope.deadline_unix_ms
        );
    });
}

#[test]
fn frame_encoding_is_what_crosses_the_wire() {
    let one = control(Address::tcp("127.0.0.1", 19001));
    let bytes = frame::encode(&one.envelope, &one.body).unwrap();
    assert_eq!(frame::decode(&bytes).unwrap(), one);
}
