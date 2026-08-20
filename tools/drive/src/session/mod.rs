//! An OUTER session against a fleet.
//!
//! The steps of a run — create, load, infer — each of them a message. Nothing
//! here reaches inside an agent. What came back is next door, in `replies`.

mod admission;
mod cache_reconcile;
mod deploy;
mod discovery;

use crate::fleet::Fleet;
use crate::telemetry::model::TelemetryEvidence;
mod replies;
mod watch;

pub use replies::{Outcome, Stream};

pub struct StartOptions<'a> {
    pub listen: &'a str,
    pub advertise: Option<&'a str>,
    pub plans: Vec<Vec<String>>,
    pub prompt: String,
    pub options: String,
    pub quiet: Duration,
    pub arrive: Duration,
    pub vary: bool,
    /// Number of requests sent immediately at the beginning of an inference
    /// run. Zero disables scheduled batches.
    pub initial_burst: usize,
    /// Number of requests in each batch after the initial burst.
    pub batch_size: usize,
    /// Delay before each post-burst batch.
    pub batch_interval: Duration,
}

struct OutboundRequest<'a> {
    target: Address,
    recipient: Recipient,
    lane: QueueClass,
    route: &'a str,
    request_id: &'a str,
    chain: Option<Chain>,
    body: Vec<u8>,
}

use admission::{ChainAdmission, FailureClass, Terminal, classify_failure};
use p4_agent_core::agent::{Agent, run};
use p4_agent_core::queue::lane::{Budget, Lanes};
use p4_agent_core::transport::inbox;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};
use p4_service::Bodies;
use p4_service::message::wire::{encode_to_agent, encode_to_node};
use p4_service::message::{ToAgent, ToNode};
use replies::Replies;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

pub struct Session {
    agent: Arc<Agent>,
    replies: Replies,
    /// Logical OUTER return channel plus its unguessable bearer capability.
    /// Every hop copies this opaque value; agents use it to bind the response
    /// socket and to authorize ACKs.
    return_channel: String,
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
    /// The exact artifact used by discovery and every subsequent load.
    artifact: Mutex<String>,
    /// Snapshot binding returned by each selected agent. A fleet cannot use
    /// one agent's snapshot for another agent's local model state.
    capability_snapshots: Mutex<HashMap<String, (String, u64)>>,
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
    /// Durable coordinator journal root for cache transactions. Production
    /// callers should set this to the host's persistent state directory.
    pub(crate) cache_state_dir: PathBuf,
    /// Separates retry routes even when a caller reuses one logical sequence.
    pub(crate) cache_attempt: AtomicU64,
    /// One coordinator transaction at a time per outer session. This prevents
    /// two same-sequence calls from sharing a journal or operation identity.
    pub(crate) cache_transaction_lock: Arc<tokio::sync::Mutex<()>>,
    admission: Mutex<ChainAdmission>,
    /// Driver-only submission schedule. These do not change the node
    /// adapter's ceiling or the node queue's own scheduling policy.
    initial_burst: usize,
    batch_size: usize,
    batch_interval: Duration,
}

impl Session {
    pub(super) fn trace(&self, message: impl AsRef<str>) {
        if std::env::var("P4_DRIVE_TRACE").is_ok_and(|value| value != "0") {
            eprintln!("P4_DRIVE_TRACE {}", message.as_ref());
            use std::io::Write;
            let _ = std::io::stderr().flush();
        }
    }
    /// `advertise` is what the agents are told to answer to. A driver that
    /// names itself by the wildcard it bound is asking a remote machine to
    /// reply to its own loopback.
    pub async fn start(options: StartOptions<'_>) -> Result<Self, Box<dyn std::error::Error>> {
        let StartOptions {
            listen,
            advertise,
            plans,
            prompt,
            options,
            quiet,
            arrive,
            vary,
            initial_burst,
            batch_size,
            batch_interval,
        } = options;
        let listener = TcpListener::bind(listen).await?;
        let bound = listener.local_addr()?;
        let replies = Replies::default();
        let own = Address::advertised(advertise, &bound.ip().to_string(), bound.port())?;
        let (agent, receiver, in_flight) = Agent::new(
            own.clone(),
            Arc::new(replies.clone()),
            Arc::new(Bodies::default()),
            driver_lanes(),
            Budget::default(),
        );
        tokio::spawn(inbox::serve(listener, agent.queue(), 256));
        tokio::spawn(run(Arc::clone(&agent), receiver, in_flight));
        let return_channel = return_channel(&own)?;
        Ok(Self {
            agent,
            replies,
            return_channel,
            plans,
            prompt,
            options,
            artifact: Mutex::new(
                std::env::var("P4_DRIVE_ARTIFACT").unwrap_or_else(|_| "model".into()),
            ),
            capability_snapshots: Mutex::new(HashMap::new()),
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
            cache_state_dir: std::env::var_os("P4_DRIVE_CACHE_STATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("p4-drive-cache")),
            cache_attempt: AtomicU64::new(0),
            cache_transaction_lock: Arc::new(tokio::sync::Mutex::new(())),
            admission: Mutex::new(ChainAdmission::new_unbounded()),
            initial_burst,
            batch_size,
            batch_interval,
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

    pub(crate) fn telemetry(&self, elapsed: Duration) -> TelemetryEvidence {
        self.replies.telemetry(elapsed)
    }

    pub(crate) fn bind_discovery(
        &self,
        artifact: String,
        snapshots: HashMap<String, (String, u64)>,
    ) {
        *self.artifact.lock().expect("artifact lock") = artifact;
        *self.capability_snapshots.lock().expect("snapshot lock") = snapshots;
    }

    pub(crate) fn artifact(&self) -> String {
        self.artifact.lock().expect("artifact lock").clone()
    }

    pub(crate) fn capability_for(&self, address: &Address) -> (String, u64) {
        self.capability_snapshots
            .lock()
            .expect("snapshot lock")
            .get(&address.to_string())
            .cloned()
            .unwrap_or_else(|| {
                (
                    self.capability_snapshot_id.clone(),
                    self.capability_expires_at,
                )
            })
    }

    pub(crate) fn capability_is_valid(snapshot_id: &str, expires_at: u64, now: u64) -> bool {
        !snapshot_id.is_empty() && expires_at > now
    }

    pub(crate) fn unix_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis() as u64)
            .unwrap_or_default()
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
            .values()
            .find(|stream| stream.request_id == route || stream.stream_id == route)
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

    fn reconcile_admission(&self) {
        let mut admission = self.admission.lock().expect("admission lock");
        for stream in self.streams() {
            let terminal = match (&stream.done, &stream.failed) {
                (Some(_), _) => Some(Terminal::Done),
                (None, Some(detail)) => Some(Terminal::Failed(classify_failure(detail))),
                (None, None) => None,
            };
            if let Some(terminal) = terminal {
                admission.finish(&stream.request_id, terminal);
            }
        }
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
        let mut admission_rejected = 0;
        for index in 0..requests {
            // This schedule belongs to the driver only. It controls when
            // frames enter the agent; it must not be confused with the node
            // adapter's active window, which remains bounded by its loaded
            // capacity and drains the node queue independently.
            if self.initial_burst > 0 && !self.batch_interval.is_zero() {
                let boundary = if index == self.initial_burst {
                    true
                } else {
                    index > self.initial_burst
                        && self.batch_size > 0
                        && (index - self.initial_burst).is_multiple_of(self.batch_size)
                };
                if boundary {
                    tokio::time::sleep(self.batch_interval).await;
                }
            }
            // Round robin rather than filling one and moving on. Two replicas
            // fed in turn are two deployments working; fed in blocks they are
            // one deployment working and one idle, which measures the same
            // thing the single-chain driver already measured, twice.
            let chain = &chains[index % chains.len()];
            let route = self.route(&format!("q{index}"));
            // One attempt, not a retry: OUTER's admission is unbounded, so the
            // only answers are a lease and a route this run already knows
            // about. Asking again would give the same answer forever.
            self.reconcile_admission();
            let admitted = match self
                .admission
                .lock()
                .expect("admission lock")
                .begin(&route, chain.len())
            {
                admission::Admission::Acquired(lease) => Some(lease),
                admission::Admission::Full => {
                    unreachable!("OUTER admission is unbounded; node capacity belongs to the agent")
                }
                admission::Admission::AlreadyActive | admission::Admission::AlreadyTerminal => None,
            };
            if admitted.is_none() {
                admission_rejected += 1;
                continue;
            }
            self.trace(format!(
                "infer_send route={} target={} node={} chain_len={} tokens={}",
                route,
                chain.current().address,
                chain.current().node,
                chain.len(),
                tokens,
            ));
            self.replies.begin_request(&route, &self.return_channel);
            let send_result = self.send(
                chain.current().address.clone(),
                Recipient::node(chain.current().node.clone()),
                QueueClass::Prefill,
                &route,
                Some(chain.clone()),
                encode_to_node(&ToNode::Execute {
                    prompt: self.ask(index),
                    max_tokens: tokens,
                    options: self.options.clone(),
                }),
            );
            self.trace(format!(
                "infer_send_result route={} result={:?}",
                route, send_result
            ));
            if send_result.is_err() {
                self.admission
                    .lock()
                    .expect("admission lock")
                    .finish(&route, Terminal::Failed(FailureClass::NonRetryable));
                admission_rejected += 1;
            }
            // Arrivals spread over time rather than all at once. A burst
            // measures a backlog draining; work actually arrives while earlier
            // work is still running, and the queues behave differently under
            // the two.
            if !self.arrive.is_zero() {
                // Do not enqueue the control probe before the prefill frame
                // has had a chance to leave OUTER; control dispatch is
                // intentionally higher priority than prefill.
                tokio::time::sleep(self.arrive).await;
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
            }
        }
        // A counter, not a walk of the streams: walking them held the lock the
        // recording path needs, and grew with the tokens it was counting.
        // Do not inject control/status frames while a real hop may be
        // occupying the native worker.  Long M3 hops can legitimately hold
        // that worker for minutes; periodic probes then accumulate behind
        // the computation and eventually fill the worker queue.  The final
        // snapshot below is sufficient for a completed run, while the
        // inference path must remain free of observer traffic.
        let waited = self
            .until_watching(&[], || {
                self.replies.finished.load(SeqCst) >= already + requests
            })
            .await;

        // A terminal reply can arrive before the next status snapshot. Flush
        // one final snapshot so retained runtime samples from the last
        // sequence are included in the evidence report.
        self.flush_telemetry(&watch).await;

        let streams: Vec<Stream> = (0..requests)
            .map(|index| self.stream(&self.route(&format!("q{index}"))))
            .collect();
        self.reconcile_admission();
        let stalled: Vec<usize> = streams
            .iter()
            .filter(|stream| !stream.is_finished())
            .map(|stream| stream.tokens.len())
            .take(12)
            .collect();
        Outcome {
            stalled,
            completed: streams.iter().filter(|s| s.done.is_some()).count(),
            failed: streams.iter().filter(|s| s.failed.is_some()).count() + admission_rejected,
            why: streams
                .iter()
                .find_map(|stream| stream.failed.clone())
                .or_else(|| {
                    (admission_rejected > 0).then_some("logical admission rejected".into())
                }),
            unanswered: streams.iter().filter(|s| !s.is_finished()).count(),
            out_of_order: streams.iter().filter(|s| !s.is_ordered()).count(),
            tokens: streams.iter().map(|s| s.tokens.len()).sum(),
            routes: self.streams().len(),
            latency_us: streams
                .iter()
                .filter_map(|stream| stream.latency_us)
                .collect(),
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
            streams,
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
        self.send_with_request_id(OutboundRequest {
            target,
            recipient,
            lane,
            route,
            request_id: route,
            chain,
            body,
        })
    }

    fn send_with_request_id(
        &self,
        request: OutboundRequest<'_>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let OutboundRequest {
            target,
            recipient,
            lane,
            route,
            request_id,
            chain,
            body,
        } = request;
        self.agent
            .enqueue(Frame {
                envelope: Envelope {
                    target,
                    recipient,
                    lane,
                    route: route.to_owned(),
                    request_id: request_id.to_owned(),
                    stream_id: route.to_owned(),
                    origin_agent: Some(self.agent.address().clone()),
                    return_channel: Some(self.return_channel.clone()),
                    ingress_generation: 0,
                    event_seq: 0,
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
            // Let the just-enqueued inference frame leave the OUTER first.
            // Control-lane status probes have higher dispatch priority than
            // prefill; probing at tick zero could therefore starve the first
            // HOP behind an endless stream of telemetry frames.
            if !watch.is_empty() && tick > 10 && tick.is_multiple_of(10) {
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

    async fn flush_telemetry(&self, watch: &[Address]) {
        if watch.is_empty() {
            return;
        }
        for address in watch {
            let _ = self.send(
                address.clone(),
                Recipient::Agent,
                QueueClass::Control,
                &self.route("telemetry-final"),
                None,
                encode_to_agent(&ToAgent::Status),
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn return_channel(address: &Address) -> Result<String, Box<dyn std::error::Error>> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let capability = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(p4_protocol::return_channel::with_capability(
        &address.to_string(),
        &capability,
    ))
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

#[cfg(test)]
mod tests {
    use super::{Session, StartOptions};
    use p4_protocol::frame;
    use p4_protocol::{Envelope, QueueClass, Recipient};
    use p4_service::message::Reply;
    use p4_service::message::wire::encode_reply;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;

    #[test]
    fn distributed_load_requires_a_nonempty_unexpired_snapshot() {
        assert!(Session::capability_is_valid("cap-1", 101, 100));
        assert!(!Session::capability_is_valid("", 101, 100));
        assert!(!Session::capability_is_valid("cap-1", 100, 100));
        assert!(!Session::capability_is_valid("cap-1", 0, 100));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tcp_duplicate_cache_replies_poison_the_collector_route() {
        let session = Session::start(StartOptions {
            listen: "127.0.0.1:0",
            advertise: None,
            plans: Vec::new(),
            prompt: "prompt".into(),
            options: "{}".into(),
            quiet: Duration::from_secs(1),
            arrive: Duration::ZERO,
            vary: false,
            initial_burst: 0,
            batch_size: 1,
            batch_interval: Duration::ZERO,
        })
        .await
        .unwrap();
        let target = session.agent.address().clone();
        let reply = Reply::CacheStatus {
            deployment: "d".into(),
            stage_id: "s".into(),
            generation: 1,
            operation_id: "op-tcp".into(),
            sequence: "seq".into(),
            state: "committed".into(),
            bytes: 1,
            detail: "ok".into(),
        };
        let envelope = Envelope {
            target: target.clone(),
            recipient: Recipient::Agent,
            lane: QueueClass::Response,
            route: "tcp-cache-route".into(),
            request_id: "op-tcp".into(),
            stream_id: "tcp-stream".into(),
            origin_agent: None,
            return_channel: Some("outer".into()),
            ingress_generation: 0,
            event_seq: 1,
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        };
        let bytes = frame::encode(&envelope, &encode_reply(&reply)).unwrap();
        let mut first_socket = TcpStream::connect((target.host.as_str(), target.port))
            .await
            .unwrap();
        first_socket.write_all(&bytes).await.unwrap();
        drop(first_socket);
        let mut reconnected_socket = TcpStream::connect((target.host.as_str(), target.port))
            .await
            .unwrap();
        reconnected_socket.write_all(&bytes).await.unwrap();
        drop(reconnected_socket);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if session.replies.is_cache_route_poisoned("tcp-cache-route") {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            session
                .replies
                .take_cache_reply("tcp-cache-route")
                .is_none()
        );
        assert!(session.replies.has_consumed_cache_route("tcp-cache-route"));
    }
}
