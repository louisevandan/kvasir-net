//! An OUTER session against a fleet.
//!
//! The steps of a run — create, load, infer — each of them a message. Nothing
//! here reaches inside an agent. What came back is next door, in `replies`.

mod deploy;
mod discovery;

use crate::fleet::Fleet;
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
    /// What each load carries, indexed by deployment and then stage. Opaque
    /// here and read only by the backend — the shares of a distributed
    /// deployment differ from each other, and two replicas of one deployment
    /// differ again because they sit on different cards.
    plans: Vec<Vec<String>>,
    /// What every request asks. One prompt for all of them: a driver measures a
    /// deployment under a shape of work, and varying the prompt would vary the
    /// thing being measured.
    prompt: String,
    /// Sampling and generation settings, merged into the backend's request.
    /// Opaque here for the same reason a plan is.
    options: String,
    /// Discovery snapshot binding supplied by OUTER. Empty/zero keeps mock
    /// runs compatible; production callers must provide both values.
    pub(crate) capability_snapshot_id: String,
    pub(crate) capability_expires_at: u64,
    /// How long nothing may arrive before the driver stops waiting.
    quiet: Duration,
    /// Gap between arrivals. Zero sends the whole run at once, which measures a
    /// backlog draining rather than one forming.
    arrive: Duration,
    /// Whether each request gets a prompt of its own.
    ///
    /// Off measures one prompt many times, which is a cache as much as a
    /// model: a warm `llama-server` matched all sixty-four identical prompts
    /// against its prompt cache at similarity 1.000 and evicted a 143 MiB
    /// entry to admit each one, taking eight to seventeen seconds apiece while
    /// every connection sat established and every card idle. Real traffic does
    /// not repeat itself, so this exists to say which is being measured.
    vary: bool,
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
        plans: Vec<Vec<String>>,
        prompt: String,
        options: String,
        quiet: Duration,
        arrive: Duration,
        vary: bool,
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
            capability_snapshot_id: std::env::var("P4_DRIVE_CAPABILITY_SNAPSHOT_ID")
                .unwrap_or_default(),
            capability_expires_at: std::env::var("P4_DRIVE_CAPABILITY_EXPIRES_AT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            quiet,
            arrive,
            vary,
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

    /// What request `index` asks.
    ///
    /// The marker goes first because a prompt cache matches on the longest
    /// common prefix: appending would leave every request sharing all but its
    /// last line, which is the case that thrashed.
    fn ask(&self, index: usize) -> String {
        match self.vary {
            true => format!("Request {index}.\n\n{}", self.prompt),
            false => self.prompt.clone(),
        }
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

    /// `serving` are the stages an inference actually visits, which is not
    /// always all of them: a backend that spreads a model internally has shares
    /// that must be loaded and have no completions surface, and a hop sent to
    /// one is addressed to the wrong half of its own deployment.
    pub async fn infer(
        &self,
        fleet: &Fleet,
        serving: &[usize],
        requests: usize,
        tokens: u32,
    ) -> Outcome {
        // One chain per deployment, built once. They differ only in which
        // machines and which node names they name; the shape is the same,
        // because that is what makes them replicas.
        let mut chains = Vec::new();
        for deployment in 0..fleet.deployments().len() {
            let links: Vec<Link> = serving
                .iter()
                .map(|&stage| Link {
                    address: fleet.deployments()[deployment][stage].clone(),
                    // The name the node was created under, not its position in
                    // this chain — a stage that is skipped does not renumber
                    // the rest.
                    node: fleet.node_of(deployment, stage),
                    binding: "deployment".into(),
                    generation: 1,
                })
                .collect();
            let Ok(chain) = Chain::new(links) else {
                return Outcome::default();
            };
            chains.push(chain);
        }

        // Every machine in the fleet, including one holding a share that
        // serves nothing: its node has a queue too, and "nothing ever queued
        // there" is a claim worth being able to make.
        let watch = fleet.addresses();

        let already = self.replies.finished.load(SeqCst);
        for index in 0..requests {
            // Round robin rather than filling one and moving on. Two replicas
            // fed in turn are two deployments working; fed in blocks they are
            // one deployment working and one idle, which measures the same
            // thing the single-chain driver already measured, twice.
            let chain = &chains[index % chains.len()];
            let _ = self.send(
                chain.current().address.clone(),
                Recipient::node(chain.current().node.clone()),
                QueueClass::Prefill,
                &self.route(&format!("q{index}")),
                Some(chain.clone()),
                encode_to_node(&ToNode::Execute {
                    prompt: self.ask(index),
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
            why: streams.iter().find_map(|stream| stream.failed.clone()),
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
