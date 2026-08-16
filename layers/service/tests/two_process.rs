//! The service on the core, end to end.
//!
//! Three agents, each with its own listener, talking only over sockets: nodes
//! created by message, a model loaded by message, an inference chained across
//! them, and the answer arriving where the request said to send it. This is
//! what the running system does, minus the process boundary.

use p4_agent_core::agent::{Agent, Duties, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use p4_service::message::wire::{decode_reply, encode_to_agent, encode_to_node};
use p4_service::message::{Reply, ToAgent, ToNode};
use p4_service::{Bodies, Registry, Standard};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

/// Stands in for OUTER: keeps every reply, per route.
#[derive(Default, Clone)]
struct Outer(Arc<Mutex<HashMap<String, Vec<Reply>>>>);

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
    fn replies(&self, route: &str) -> Vec<Reply> {
        self.0
            .lock()
            .unwrap()
            .get(route)
            .cloned()
            .unwrap_or_default()
    }
}

fn backends() -> Registry {
    let mut registry = Registry::new();
    registry.register_fn("mock-lead", |_| {
        Arc::new(Mock::staged(0, Profile::default()))
    });
    registry.register_fn("mock-tail", |_| {
        Arc::new(Mock::terminal(1, Profile::default()))
    });
    registry.register_fn("mock-solo", |_| Arc::new(Mock::internal(Profile::default())));
    registry
}

async fn start(duties: Arc<dyn Duties>) -> Arc<Agent> {
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

fn to_agent(target: &Arc<Agent>, outer: &Arc<Agent>, route: &str, message: ToAgent) -> Frame {
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

fn to_node(
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

fn chain_over(links: &[(&Arc<Agent>, &str)]) -> Chain {
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

async fn until(mut done: impl FnMut() -> bool) {
    for _ in 0..500 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(6)
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn a_deployment_is_created_loaded_and_run_entirely_by_message() {
    // Every step is a frame over a socket. Nothing is configured in process.
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let lead = start(Arc::new(Standard::new(backends()))).await;
        let tail = start(Arc::new(Standard::new(backends()))).await;

        // 1. Create the nodes. An id and an adapter name; nothing is
        //    materialised yet.
        lead.enqueue(to_agent(&lead, &outer, "create-0", ToAgent::CreateNode {
            node: "n0".into(),
            adapter: "mock-lead".into(),
        }))
        .unwrap();
        tail.enqueue(to_agent(&tail, &outer, "create-1", ToAgent::CreateNode {
            node: "n1".into(),
            adapter: "mock-tail".into(),
        }))
        .unwrap();
        until(|| !seen.replies("create-0").is_empty() && !seen.replies("create-1").is_empty()).await;
        assert!(matches!(
            seen.replies("create-0").first(),
            Some(Reply::Accepted { .. })
        ));

        let chain = chain_over(&[(&lead, "n0"), (&tail, "n1")]);

        // 2. Load each node's share, declaring the concurrency it admits.
        for (index, agent) in [(&lead, "n0"), (&tail, "n1")].iter().enumerate() {
            let single = chain_over(&[(
                if index == 0 { &lead } else { &tail },
                agent.1,
            )]);
            let owner = if index == 0 { &lead } else { &tail };
            owner
                .enqueue(to_node(
                    &single,
                    &outer,
                    &format!("load-{index}"),
                    QueueClass::Control,
                    ToNode::Load {
                        plan: r#"{"layers":"0-19"}"#.into(),
                        artifact: "model.gguf".into(),
                        ceiling: 8,
                    },
                ))
                .unwrap();
        }
        until(|| {
            seen.replies("load-0")
                .iter()
                .any(|reply| matches!(reply, Reply::Bound { .. }))
                && seen
                    .replies("load-1")
                    .iter()
                    .any(|reply| matches!(reply, Reply::Bound { .. }))
        })
        .await;

        // A distributed load reports per stage before it binds.
        let load = seen.replies("load-0");
        assert!(
            load.iter().any(|reply| matches!(reply, Reply::Progress { .. })),
            "stages were reported: {load:?}"
        );

        // 3. Run an inference across both machines.
        lead.enqueue(to_node(
            &chain,
            &outer,
            "infer-1",
            QueueClass::Prefill,
            ToNode::Execute {
                prompt: "안녕하세요".into(),
                max_tokens: 4,
                options: r#"{"temperature":0.2}"#.into(),
            },
        ))
        .unwrap();
        until(|| {
            seen.replies("infer-1")
                .iter()
                .any(|reply| matches!(reply, Reply::Done { .. }))
        })
        .await;

        let stream = seen.replies("infer-1");
        let tokens: Vec<u32> = stream
            .iter()
            .filter_map(|reply| match reply {
                Reply::Token { index, .. } => Some(*index),
                _ => None,
            })
            .collect();
        assert_eq!(tokens, vec![0, 1, 2], "tokens arrived in order: {stream:?}");
        assert!(
            matches!(stream.last(), Some(Reply::Done { generated: 4, .. })),
            "one terminal, counting every token: {stream:?}"
        );
    });
}

#[test]
fn a_node_can_be_unloaded_and_deleted_by_message() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let agent = start(Arc::new(Standard::new(backends()))).await;

        agent
            .enqueue(to_agent(&agent, &outer, "create", ToAgent::CreateNode {
                node: "solo".into(),
                adapter: "mock-solo".into(),
            }))
            .unwrap();
        until(|| !seen.replies("create").is_empty()).await;

        let chain = chain_over(&[(&agent, "solo")]);
        agent
            .enqueue(to_node(
                &chain,
                &outer,
                "unload",
                QueueClass::Control,
                ToNode::Unload,
            ))
            .unwrap();
        until(|| !seen.replies("unload").is_empty()).await;
        assert!(matches!(seen.replies("unload").last(), Some(Reply::Released)));

        agent
            .enqueue(to_agent(&agent, &outer, "delete", ToAgent::DeleteNode {
                node: "solo".into(),
            }))
            .unwrap();
        until(|| !seen.replies("delete").is_empty()).await;
        assert!(matches!(seen.replies("delete").last(), Some(Reply::Released)));
        assert_eq!(agent.node_depth("solo").await, None);
    });
}

#[test]
fn many_inferences_across_two_machines_all_answer() {
    runtime().block_on(async {
        let seen = Outer::default();
        let outer = start(Arc::new(seen.clone())).await;
        let lead = start(Arc::new(Standard::new(backends()))).await;
        let tail = start(Arc::new(Standard::new(backends()))).await;

        lead.enqueue(to_agent(&lead, &outer, "c0", ToAgent::CreateNode {
            node: "n0".into(),
            adapter: "mock-lead".into(),
        }))
        .unwrap();
        tail.enqueue(to_agent(&tail, &outer, "c1", ToAgent::CreateNode {
            node: "n1".into(),
            adapter: "mock-tail".into(),
        }))
        .unwrap();
        until(|| !seen.replies("c0").is_empty() && !seen.replies("c1").is_empty()).await;

        let chain = chain_over(&[(&lead, "n0"), (&tail, "n1")]);
        for index in 0..24 {
            lead.enqueue(to_node(
                &chain,
                &outer,
                &format!("q{index}"),
                QueueClass::Prefill,
                ToNode::Execute {
                    prompt: "p".into(),
                    max_tokens: 2,
                    options: "{}".into(),
                },
            ))
            .unwrap();
        }
        until(|| {
            (0..24).all(|index| {
                seen.replies(&format!("q{index}"))
                    .iter()
                    .any(|reply| matches!(reply, Reply::Done { .. }))
            })
        })
        .await;

        for index in 0..24 {
            let stream = seen.replies(&format!("q{index}"));
            let terminals = stream
                .iter()
                .filter(|reply| matches!(reply, Reply::Done { .. }))
                .count();
            assert_eq!(terminals, 1, "route {index} ended once: {stream:?}");
        }
    });
}
