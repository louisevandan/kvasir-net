//! An OUTER session against a fleet.
//!
//! The steps of a run — create, load, infer — each of them a message. Nothing
//! here reaches inside an agent. What came back is next door, in `replies`.

mod replies;
mod watch;

pub use replies::{Outcome, Stream};

use p4_agent_core::agent::{Agent, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use p4_service::Bodies;
use p4_service::message::wire::{encode_to_agent, encode_to_node};
use p4_service::message::{ToAgent, ToNode};
use replies::Replies;
use std::sync::Arc;
use std::sync::atomic::Ordering::SeqCst;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

pub struct Session {
    agent: Arc<Agent>,
    replies: Replies,
    /// What each stage's load carries, one per stage. Opaque here and read only
    /// by the backend — the shares of a distributed deployment differ from each
    /// other, and only the backend knows how.
    plans: Vec<String>,
    /// What every request asks. One prompt for all of them: a driver measures a
    /// deployment under a shape of work, and varying the prompt would vary the
    /// thing being measured.
    prompt: String,
    /// Sampling and generation settings, merged into the backend's request.
    /// Opaque here for the same reason a plan is.
    options: String,
    /// How long nothing may arrive before the driver stops waiting.
    quiet: Duration,
    /// Gap between arrivals. Zero sends the whole run at once, which measures a
    /// backlog draining rather than one forming.
    arrive: Duration,
    /// What makes this run's route names its own.
    run: String,
}

impl Session {
    /// `advertise` is what the agents are told to answer to. A driver that
    /// names itself by the wildcard it bound is asking a remote machine to
    /// reply to its own loopback.
    pub async fn start(
        listen: &str,
        advertise: Option<&str>,
        plans: Vec<String>,
        prompt: String,
        options: String,
        quiet: Duration,
        arrive: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(listen).await?;
        let bound = listener.local_addr()?;
        let replies = Replies::default();
        let (agent, receiver, in_flight) = Agent::new(
            Address::advertised(advertise, &bound.ip().to_string(), bound.port())?,
            Arc::new(replies.clone()),
            Arc::new(Bodies),
            driver_lanes(),
            Budget::default(),
        );
        tokio::spawn(inbox::serve(listener, agent.queue(), 256));
        tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
        Ok(Self {
            agent,
            replies,
            plans,
            prompt,
            options,
            quiet,
            arrive,
            // The clock, because it is monotonic across restarts on one machine
            // and this only has to separate one run from the last.
            run: format!(
                "r{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_millis())
                    .unwrap_or_default()
            ),
        })
    }

    pub fn address(&self) -> &Address {
        self.agent.address()
    }

    /// Bytes, not tokens. This tool cannot count tokens without knowing the
    /// backend's tokeniser, and guessing a number that reads as authoritative
    /// is worse than reporting the one it actually knows.
    pub fn prompt_bytes(&self) -> usize {
        self.prompt.len()
    }

    /// A route name nobody else will pick.
    ///
    /// Agents outlive drivers, and an inference outlives a driver that walked
    /// away from it. Reusing `q0` across runs merged one run's leftover tokens
    /// into the next run's stream, which showed up as an ordering failure in a
    /// layer that had ordered them correctly — the tool inventing a defect in
    /// the thing it exists to check.
    fn route(&self, name: &str) -> String {
        format!("{}-{name}", self.run)
    }

    fn gave_up(&self, doing: &str) -> String {
        format!(
            "gave up {doing}: nothing arrived for {:?}. This is the driver's \
             patience, not a verdict on the deployment",
            self.quiet
        )
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
                &self.route(&format!("create-{stage}")),
                None,
                encode_to_agent(&ToAgent::CreateNode {
                    node: Self::node_of(stage, chain.len()),
                    adapter: adapter.to_owned(),
                }),
            )?;
        }
        if !self
            .until(|| self.replies.accepted.load(SeqCst) >= chain.len())
            .await
        {
            return Err(self.gave_up("creating nodes").into());
        }
        for stage in 0..chain.len() {
            let stream = self.stream(&self.route(&format!("create-{stage}")));
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
                &self.route(&format!("load-{stage}")),
                Some(single),
                encode_to_node(&ToNode::Load {
                    // A mock ignores this; a concrete backend reads it and is
                    // the only thing that knows what it means. The driver
                    // carries it rather than inventing one, because a plan is
                    // opaque above the adapter and inventing one here would be
                    // this tool knowing a backend.
                    plan: self.plans[stage].clone(),
                    artifact: "model".into(),
                    ceiling,
                }),
            )?;
        }
        if !self
            .until(|| self.replies.bound.load(SeqCst) >= chain.len())
            .await
        {
            return Err(self.gave_up("loading").into());
        }
        for stage in 0..chain.len() {
            let stream = self.stream(&self.route(&format!("load-{stage}")));
            if !stream.bound {
                // The backend's own words. A driver that reported only "never
                // bound" made every load failure look the same, which is the
                // one thing an operator cannot work from.
                return Err(format!(
                    "stage {stage} never bound: {}",
                    stream.failed.unwrap_or_else(|| "no answer".into())
                )
                .into());
            }
        }
        Ok(())
    }

    /// `serving` are the stages an inference actually visits, which is not
    /// always all of them: a backend that spreads a model internally has shares
    /// that must be loaded and have no completions surface, and a hop sent to
    /// one is addressed to the wrong half of its own deployment.
    pub async fn infer(
        &self,
        chain_of: &[Address],
        serving: &[usize],
        requests: usize,
        tokens: u32,
    ) -> Outcome {
        let links: Vec<Link> = serving
            .iter()
            .map(|&stage| Link {
                address: chain_of[stage].clone(),
                // The name the node was created under, not its position in this
                // chain — a stage that is skipped does not renumber the rest.
                node: Self::node_of(stage, chain_of.len()),
                binding: "deployment".into(),
                generation: 1,
            })
            .collect();
        let Ok(chain) = Chain::new(links) else {
            return Outcome::default();
        };
        let entry = chain.current().address.clone();
        let node = chain.current().node.clone();

        // Every machine in the deployment, including one holding a share that
        // serves nothing: its node has a queue too, and "nothing ever queued
        // there" is a claim worth being able to make.
        let mut watch: Vec<Address> = Vec::new();
        for address in chain_of {
            if !watch.contains(address) {
                watch.push(address.clone());
            }
        }

        let already = self.replies.finished.load(SeqCst);
        for index in 0..requests {
            let _ = self.send(
                entry.clone(),
                Recipient::node(node.clone()),
                QueueClass::Prefill,
                &self.route(&format!("q{index}")),
                Some(chain.clone()),
                encode_to_node(&ToNode::Execute {
                    prompt: self.prompt.clone(),
                    max_tokens: tokens,
                    options: self.options.clone(),
                }),
            );
            // Arrivals spread over time rather than all at once. A burst
            // measures a backlog draining; work actually arrives while earlier
            // work is still running, and the queues behave differently under
            // the two.
            if !self.arrive.is_zero() {
                for address in &watch {
                    let _ = self.send(
                        address.clone(),
                        Recipient::Agent,
                        QueueClass::Control,
                        &self.route("watch"),
                        None,
                        encode_to_agent(&ToAgent::Status),
                    );
                }
                tokio::time::sleep(self.arrive).await;
            }
        }
        // A counter, not a walk of the streams: walking them held the lock the
        // recording path needs, and grew with the tokens it was counting.
        let waited = self
            .until_watching(&watch, || {
                self.replies.finished.load(SeqCst) >= already + requests
            })
            .await;

        let streams: Vec<Stream> = (0..requests)
            .map(|index| self.stream(&self.route(&format!("q{index}"))))
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
            sample: streams
                .iter()
                .find(|stream| !stream.text.is_empty())
                .map(|stream| stream.text.clone())
                .unwrap_or_default(),
            quiet: !waited,
            node_depth: self.replies.peaks.node_depth.load(SeqCst),
            running: self.replies.peaks.running.load(SeqCst),
            lane: self.replies.peaks.lane.load(SeqCst),
            samples: self.replies.peaks.samples(),
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

    /// Waits for a condition, giving up only once nothing at all is arriving.
    ///
    /// `false` means the driver stopped waiting, which is not the same claim as
    /// the deployment having stalled — and the difference is the whole reason
    /// this is written against progress rather than a count of polls. A fixed
    /// ceiling was fine while runs generated sixty-four tokens and became a lie
    /// at five thousand: it reported a stall at the exact token the driver ran
    /// out of patience on, while the backend went on to finish normally.
    async fn until(&self, done: impl FnMut() -> bool) -> bool {
        self.until_watching(&[], done).await
    }

    /// The same wait, asking each address what it is doing as it goes.
    ///
    /// The queues only exist while the run does, so a claim about them has to
    /// be observed from inside it — and observed by asking over the socket,
    /// which is the only way OUTER can observe anything.
    async fn until_watching(&self, watch: &[Address], mut done: impl FnMut() -> bool) -> bool {
        let mut seen = self.replies.events.load(SeqCst);
        let mut since = Instant::now();
        let mut tick = 0usize;
        loop {
            if done() {
                return true;
            }
            // Often enough to catch a peak between two hops, rare enough that
            // the asking is not itself the load.
            if !watch.is_empty() && tick % 10 == 0 {
                for address in watch {
                    let _ = self.send(
                        address.clone(),
                        Recipient::Agent,
                        QueueClass::Control,
                        &self.route("watch"),
                        None,
                        encode_to_agent(&ToAgent::Status),
                    );
                }
            }
            tick += 1;
            tokio::time::sleep(Duration::from_millis(20)).await;
            let now = self.replies.events.load(SeqCst);
            if now != seen {
                seen = now;
                since = Instant::now();
            } else if since.elapsed() >= self.quiet {
                return false;
            }
        }
    }
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
