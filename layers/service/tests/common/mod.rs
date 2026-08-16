//! Standing agents up over sockets, and the vocabulary to talk to them.
//!
//! Shared by every test in this directory. Each test binary compiles it
//! separately, so an item one binary does not use is not dead code.
#![allow(dead_code)]
use p4_agent_core::agent::{Agent, Duties, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_mock::Mock;
use p4_mock::profile::{Fault, Profile};
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use p4_service::message::wire::{decode_reply, encode_to_agent, encode_to_node};
use p4_service::message::{Reply, ToAgent, ToNode};
use p4_service::{Bodies, Registry};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

/// Stands in for OUTER: keeps every reply, per route.
#[derive(Default, Clone)]
pub struct Outer(Arc<Mutex<HashMap<String, Vec<Reply>>>>);

impl Duties for Outer {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        if let Ok(reply) = decode_reply(&frame.body) {
            self.0
                .lock()
                .unwrap()
                .entry(frame.envelope.route.clone())
                .or_default()
                .push(reply);
        }
    }
}

impl Outer {
    pub fn replies(&self, route: &str) -> Vec<Reply> {
        self.0
            .lock()
            .unwrap()
            .get(route)
            .cloned()
            .unwrap_or_default()
    }
}

pub fn backends() -> Registry {
    let mut registry = Registry::new();
    registry.register_fn("mock-lead", |_| {
        Arc::new(Mock::staged(0, Profile::default()))
    });
    registry.register_fn("mock-tail", |_| {
        Arc::new(Mock::terminal(1, Profile::default()))
    });
    registry.register_fn("mock-solo", |_| {
        Arc::new(Mock::internal(Profile::default()))
    });
    // A backend slow enough that a caller can ask what is happening while it
    // is still happening. Anything instant leaves nothing to observe.
    registry.register_fn("mock-slow", |_| {
        Arc::new(Mock::terminal(
            0,
            Profile {
                leading_hop: Duration::from_millis(60),
                trailing_hop: Duration::from_millis(60),
                ..Profile::default()
            },
        ))
    });
    // A stage that cannot load. Its neighbours load perfectly well, which is
    // what makes a distributed load a transaction rather than a list.
    registry.register_fn("mock-unloadable", |_| {
        Arc::new(Mock::staged(
            0,
            Profile {
                fault: Fault::Load,
                ..Profile::default()
            },
        ))
    });
    registry
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
    tokio::spawn(inbox::serve(listener, agent.queue(), 128));
    tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
    agent
}

pub fn to_agent(target: &Arc<Agent>, outer: &Arc<Agent>, route: &str, message: ToAgent) -> Frame {
    Frame {
        envelope: Envelope {
            target: target.address().clone(),
            recipient: Recipient::Agent,
            lane: QueueClass::Control,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: Some(outer.address().clone()),
            chain: None,
        },
        body: encode_to_agent(&message),
    }
}

pub fn to_node(
    chain: &Chain,
    outer: &Arc<Agent>,
    route: &str,
    lane: QueueClass,
    message: ToNode,
) -> Frame {
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: Some(outer.address().clone()),
            chain: Some(chain.clone()),
        },
        body: encode_to_node(&message),
    }
}

pub fn chain_over(links: &[(&Arc<Agent>, &str)]) -> Chain {
    Chain::new(
        links
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

pub async fn until(mut done: impl FnMut() -> bool) {
    for _ in 0..500 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(6)
        .enable_all()
        .build()
        .unwrap()
}
