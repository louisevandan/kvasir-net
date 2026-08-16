//! An OUTER session against a fleet.
//!
//! Holds the agent that receives replies, and the record of what came back per
//! route. Every step is a message: nothing here reaches inside an agent.

use p4_agent_core::agent::{Agent, Duties, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use p4_service::Bodies;
use p4_service::message::wire::{decode_reply, encode_to_agent, encode_to_node};
use p4_service::message::{Reply, ToAgent, ToNode};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

/// What a route produced.
#[derive(Default, Clone, Debug)]
pub struct Stream {
    pub tokens: Vec<u32>,
    pub done: Option<u32>,
    pub failed: Option<String>,
    pub progress: usize,
    pub bound: bool,
    pub released: bool,
    pub accepted: bool,
}

impl Stream {
    pub fn is_finished(&self) -> bool {
        self.done.is_some() || self.failed.is_some()
    }

    /// Whether the tokens arrived in the order they were produced. The one
    /// thing a caller cannot check any other way.
    pub fn is_ordered(&self) -> bool {
        self.tokens.windows(2).all(|pair| pair[0] < pair[1])
    }
}

#[derive(Default, Clone)]
struct Replies {
    streams: Arc<Mutex<HashMap<String, Stream>>>,
    /// Counted as replies land, so waiting on progress never has to walk the
    /// streams. Polling by cloning them held the same lock the recording path
    /// needs, and got slower as the tokens it was counting accumulated — the
    /// measurement starving the thing it measured.
    finished: Arc<AtomicUsize>,
    bound: Arc<AtomicUsize>,
    accepted: Arc<AtomicUsize>,
}

impl Duties for Replies {
    fn handle(&self, frame: Frame, _: &Arc<Agent>) {
        let Ok(reply) = decode_reply(&frame.body) else {
            return;
        };
        let mut streams = self.streams.lock().expect("reply lock");
        let stream = streams.entry(frame.envelope.route.clone()).or_default();
        match reply {
            Reply::Token { index, .. } => stream.tokens.push(index),
            Reply::Done { generated, .. } => {
                stream.done = Some(generated);
                self.finished.fetch_add(1, SeqCst);
            }
            Reply::Failed { detail } => {
                stream.failed = Some(detail);
                self.finished.fetch_add(1, SeqCst);
            }
            Reply::Progress { .. } => stream.progress += 1,
            Reply::Bound { .. } => {
                stream.bound = true;
                self.bound.fetch_add(1, SeqCst);
            }
            Reply::Released => stream.released = true,
            Reply::Accepted { .. } => {
                stream.accepted = true;
                self.accepted.fetch_add(1, SeqCst);
            }
            Reply::Machine { .. } => {}
        }
    }
}

pub struct Session {
    agent: Arc<Agent>,
    replies: Replies,
}

impl Session {
    pub async fn start(listen: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(listen).await?;
        let bound = listener.local_addr()?;
        let replies = Replies::default();
        let (agent, receiver, in_flight) = Agent::new(
            Address::tcp(bound.ip().to_string(), bound.port()),
            Arc::new(replies.clone()),
            Arc::new(Bodies),
            driver_lanes(),
            Budget::default(),
        );
        tokio::spawn(inbox::serve(listener, agent.queue(), 256));
        tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
        Ok(Self { agent, replies })
    }

    pub fn address(&self) -> &Address {
        self.agent.address()
    }

    fn stream(&self, route: &str) -> Stream {
        self.replies
            .streams
            .lock()
            .expect("reply lock")
            .get(route)
            .cloned()
            .unwrap_or_default()
    }

    fn streams(&self) -> Vec<Stream> {
        self.replies
            .streams
            .lock()
            .expect("reply lock")
            .values()
            .cloned()
            .collect()
    }

    /// Names one node per stage. A staged backend reads its position from the
    /// name, so the naming is part of the placement rather than cosmetic.
    fn node_of(stage: usize, total: usize) -> String {
        if stage + 1 == total {
            format!("tail-{stage}")
        } else {
            format!("stage-{stage}")
        }
    }

    pub async fn create_nodes(
        &self,
        chain: &[Address],
        adapter: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (stage, address) in chain.iter().enumerate() {
            self.send(
                address.clone(),
                Recipient::Agent,
                QueueClass::Control,
                &format!("create-{stage}"),
                None,
                encode_to_agent(&ToAgent::CreateNode {
                    node: Self::node_of(stage, chain.len()),
                    adapter: adapter.to_owned(),
                }),
            )?;
        }
        self.until(|| self.replies.accepted.load(SeqCst) >= chain.len())
            .await;
        for stage in 0..chain.len() {
            let stream = self.stream(&format!("create-{stage}"));
            if !stream.accepted {
                return Err(format!(
                    "stage {stage} refused the node: {}",
                    stream.failed.unwrap_or_else(|| "no answer".into())
                )
                .into());
            }
        }
        Ok(())
    }

    pub async fn load(
        &self,
        chain: &[Address],
        ceiling: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (stage, address) in chain.iter().enumerate() {
            let single = Chain::new(vec![Link {
                address: address.clone(),
                node: Self::node_of(stage, chain.len()),
                binding: "deployment".into(),
                generation: 1,
            }])?;
            self.send(
                address.clone(),
                Recipient::node(Self::node_of(stage, chain.len())),
                QueueClass::Control,
                &format!("load-{stage}"),
                Some(single),
                encode_to_node(&ToNode::Load {
                    plan: r#"{"simulated":true}"#.into(),
                    artifact: "model".into(),
                    ceiling,
                }),
            )?;
        }
        self.until(|| self.replies.bound.load(SeqCst) >= chain.len())
            .await;
        for stage in 0..chain.len() {
            if !self.stream(&format!("load-{stage}")).bound {
                return Err(format!("stage {stage} never bound").into());
            }
        }
        Ok(())
    }

    pub async fn infer(&self, chain: &[Address], requests: usize, tokens: u32) -> Outcome {
        let links: Vec<Link> = chain
            .iter()
            .enumerate()
            .map(|(stage, address)| Link {
                address: address.clone(),
                node: Self::node_of(stage, chain.len()),
                binding: "deployment".into(),
                generation: 1,
            })
            .collect();
        let Ok(chain) = Chain::new(links) else {
            return Outcome::default();
        };
        let entry = chain.current().address.clone();
        let node = chain.current().node.clone();

        for index in 0..requests {
            let _ = self.send(
                entry.clone(),
                Recipient::node(node.clone()),
                QueueClass::Prefill,
                &format!("q{index}"),
                Some(chain.clone()),
                encode_to_node(&ToNode::Execute {
                    prompt: "simulated prompt".into(),
                    max_tokens: tokens,
                    options: "{}".into(),
                }),
            );
        }
        // A counter, not a walk of the streams: walking them held the lock the
        // recording path needs, and grew with the tokens it was counting.
        let already = self.replies.finished.load(SeqCst);
        self.until(|| self.replies.finished.load(SeqCst) >= already + requests)
            .await;

        let streams: Vec<Stream> = (0..requests)
            .map(|index| self.stream(&format!("q{index}")))
            .collect();
        let stalled: Vec<usize> = streams
            .iter()
            .filter(|stream| !stream.is_finished())
            .map(|stream| stream.tokens.len())
            .take(12)
            .collect();
        Outcome {
            stalled,
            completed: streams.iter().filter(|s| s.done.is_some()).count(),
            failed: streams.iter().filter(|s| s.failed.is_some()).count(),
            unanswered: streams.iter().filter(|s| !s.is_finished()).count(),
            out_of_order: streams.iter().filter(|s| !s.is_ordered()).count(),
            tokens: streams.iter().map(|s| s.tokens.len()).sum(),
            routes: self.streams().len(),
        }
    }

    fn send(
        &self,
        target: Address,
        recipient: Recipient,
        lane: QueueClass,
        route: &str,
        chain: Option<Chain>,
        body: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.agent
            .enqueue(Frame {
                envelope: Envelope {
                    target,
                    recipient,
                    lane,
                    route: route.to_owned(),
                    deadline_unix_ms: 0,
                    reply_to: Some(self.agent.address().clone()),
                    chain,
                },
                body,
            })
            .map_err(|_| "the driver's own queue refused a frame".into())
    }

    /// Waits on the condition rather than on a duration, with a ceiling that
    /// only bounds a failure.
    async fn until(&self, mut done: impl FnMut() -> bool) {
        for _ in 0..3_000 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

#[derive(Default, Debug)]
pub struct Outcome {
    /// How many tokens each unfinished route managed before it stopped. The
    /// shape of this says where a stall is: all zero means work never
    /// started, all near the target means a terminal was lost.
    pub stalled: Vec<usize>,
    pub completed: usize,
    pub failed: usize,
    pub unanswered: usize,
    pub out_of_order: usize,
    pub tokens: usize,
    pub routes: usize,
}

/// A driver receives every token of every request it sent, so its response
/// lane has to be sized for the whole answer rather than for a request rate.
/// An agent's defaults are sized for relaying, which is a different job.
fn driver_lanes() -> Lanes {
    Lanes {
        response: 1 << 20,
        ..Lanes::default()
    }
}
