//! Standing agents up over sockets, and the frames to drive them.
//!
//! Shared by every test in this directory. Each test binary compiles it
//! separately, so an item one binary does not use is not dead code.
#![allow(dead_code)]
use p4_adapter::Sequence;
use p4_agent_core::agent::{Agent, Duties, run};
use p4_agent_core::node::payload::Payload;
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

/// Reads a body as "prompt|remaining", which is all a hop needs and keeps the
/// core free of a message catalog.
pub struct Bodies;

impl Payload for Bodies {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        let text = String::from_utf8_lossy(&frame.body).into_owned();
        let (prompt, remaining) = text.rsplit_once('|')?;
        Some(Sequence {
            sequence: frame.envelope.route.clone(),
            position: 0,
            prompt: Some(prompt.to_owned()),
            remaining: remaining.parse().ok()?,
            options: "{}".into(),
        })
    }
}

/// Stands in for OUTER: keeps whatever comes back, per route.
#[derive(Default, Clone)]
pub struct Outer(Arc<Mutex<HashMap<String, Vec<Frame>>>>);

impl Duties for Outer {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        self.0
            .lock()
            .unwrap()
            .entry(frame.envelope.route.clone())
            .or_default()
            .push(frame);
    }
}

impl Outer {
    pub fn routes(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    pub fn frames_for(&self, route: &str) -> Vec<Frame> {
        self.0
            .lock()
            .unwrap()
            .get(route)
            .cloned()
            .unwrap_or_default()
    }

    pub fn total_frames(&self) -> usize {
        self.0.lock().unwrap().values().map(Vec::len).sum()
    }
}

/// A node that does nothing but pass messages on, which is what an agent
/// holding no node still has to do correctly.
#[derive(Default)]
pub struct Silent;

impl Duties for Silent {
    fn handle(&self, _: Frame, _: &Arc<Agent>) {}
}

pub async fn start(duties: Arc<dyn Duties>) -> Arc<Agent> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", port),
        duties,
        Arc::new(Bodies),
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(inbox::serve(listener, agent.queue(), 256));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

/// An agent reachable only across a bad link.
///
/// The relay listens, the agent binds somewhere else, and the agent calls
/// itself by the relay's address — which is what an agent behind any gateway
/// does, and the reason the advertised address is an argument at all. Peers
/// address the relay because that is the agent's name, so every frame to it
/// crosses the declared link and nothing in P4 is told the link exists.
pub async fn start_behind(duties: Arc<dyn Duties>, link: p4_link::Impairment) -> Arc<Agent> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front = relay.local_addr().unwrap().port();
    tokio::spawn(p4_link::relay::serve(relay, bound.to_string(), link));

    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", front),
        duties,
        Arc::new(Bodies),
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(inbox::serve(listener, agent.queue(), 256));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

/// An agent whose link can be taken away while work is in flight.
///
/// Returns the agent and the cut, so a scenario can partition it from the rest
/// of the fleet and put it back without restarting anything.
pub async fn start_cuttable(
    duties: Arc<dyn Duties>,
    link: p4_link::Impairment,
) -> (Arc<Agent>, p4_link::relay::Cut) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let front = relay.local_addr().unwrap().port();
    let cut = p4_link::relay::Cut::default();
    tokio::spawn(p4_link::relay::serve_with(
        relay,
        bound.to_string(),
        link,
        cut.clone(),
    ));

    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", front),
        duties,
        Arc::new(Bodies),
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(inbox::serve(listener, agent.queue(), 256));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    (agent, cut)
}

pub fn chain_over(agents: &[(&Arc<Agent>, &str)]) -> Chain {
    Chain::new(
        agents
            .iter()
            .map(|(agent, node)| Link {
                address: (*agent).address().clone(),
                node: (*node).into(),
                binding: "deployment".into(),
                generation: 1,
            })
            .collect(),
    )
    .unwrap()
}

pub fn request(route: &str, chain: &Chain, outer: &Arc<Agent>, tokens: u32) -> Frame {
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Prefill,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: Some(outer.address().clone()),
            chain: Some(chain.clone()),
        },
        body: format!("prompt|{tokens}").into_bytes(),
    }
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .unwrap()
}

pub async fn settle(millis: u64) {
    tokio::time::sleep(Duration::from_millis(millis)).await;
}

/// Waits for a condition instead of for a duration.
///
/// These runs share a machine with every other test, so a fixed sleep is a
/// guess that gets worse the busier the host is. The generous ceiling only
/// bounds a failure; a passing run leaves as soon as the condition holds.
pub async fn until(mut done: impl FnMut() -> bool) {
    for _ in 0..600 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A body of "load|<ceiling>" or "unload" is lifecycle; anything else is a
/// sequence. Still no message catalog in the core — this is the seam.
pub struct Lifecycle;

impl Payload for Lifecycle {
    fn sequence(&self, frame: &Frame) -> Option<Sequence> {
        Bodies.sequence(frame)
    }

    fn lifecycle(&self, frame: &Frame) -> Option<p4_adapter::Work> {
        let text = String::from_utf8_lossy(&frame.body).into_owned();
        let deployment = self.deployment(frame)?;
        if text.starts_with("load|") {
            return Some(p4_adapter::Work::Load(p4_adapter::Load {
                deployment,
                plan: text,
                artifact: "model".into(),
            }));
        }
        (text == "unload").then_some(p4_adapter::Work::Unload(p4_adapter::Unload { deployment }))
    }

    fn ceiling(&self, frame: &Frame) -> Option<usize> {
        String::from_utf8_lossy(&frame.body)
            .strip_prefix("load|")?
            .parse()
            .ok()
    }
}

pub async fn start_with(duties: Arc<dyn Duties>, payload: Arc<dyn Payload>) -> Arc<Agent> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (agent, receiver, in_flight) = Agent::new(
        Address::tcp("127.0.0.1", port),
        duties,
        payload,
        Lanes::default(),
        Budget::default(),
    );
    tokio::spawn(inbox::serve(listener, agent.queue(), 256));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

pub fn control(route: &str, chain: &Chain, outer: &Arc<Agent>, body: &str) -> Frame {
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Control,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: Some(outer.address().clone()),
            chain: Some(chain.clone()),
        },
        body: body.as_bytes().to_vec(),
    }
}
