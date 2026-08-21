//! The agent: everything above, wired together.
//!
//! A worker takes a frame, makes the two decisions, and is done. Forwarding
//! hands the frame to a peer connection. Node-bound work is moved to that
//! node's queue. Only what the agent itself owns is handled here, and even
//! that never blocks — it enqueues.

use crate::continuation::Continuations;
use crate::node::payload::Payload;
use crate::node::runner::{ActiveHop, Handle, Node, WaitingRequest};
use crate::queue::lane::{Budget, Lanes};
use crate::queue::main::{Receiver, Sender, channel};
use crate::transport::inbox::{SubscriptionMetrics, Subscriptions};
use crate::transport::outbound::Peers;
use crate::worker::judge::{Verdict, judge};
use p4_adapter::Adapter;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, QueueClass, Recipient};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::{Mutex, Semaphore, mpsc};

/// What the agent itself does with a message addressed to it.
///
/// Node creation and deletion, inference intake and hardware inspection live
/// behind this, so the core never learns a message catalog. It is a procedure:
/// its output is whatever it puts on the queue.
///
/// The agent arrives as a shared handle rather than a borrow, because a duty
/// that needs to do anything asynchronous has to outlive this call — waiting
/// inside it is the one thing the CPS rule forbids.
pub trait Duties: Send + Sync {
    fn handle(&self, frame: Frame, agent: &Arc<Agent>);
}

pub struct Agent {
    own: Address,
    /// What the two decisions actually did. Counted rather than inferred,
    /// because a frame that vanishes leaves no other trace.
    forwarded: AtomicUsize,
    consumed: AtomicUsize,
    to_nodes: AtomicUsize,
    unrouted: AtomicUsize,
    refused: AtomicUsize,
    emergency_lost: Arc<AtomicUsize>,
    ack_rejected: AtomicUsize,
    status_sequence: AtomicU64,
    queue: Sender,
    peers: Peers,
    subscriptions: Subscriptions,
    nodes: Mutex<HashMap<String, Handle>>,
    continuations: Continuations<Frame>,
    duties: Arc<dyn Duties>,
    payload: Arc<dyn Payload>,
    node_queue_depth: usize,
    /// A separate bounded retry lane for errors generated while the normal
    /// response lane is full. It prevents a refusal from being silently lost
    /// while keeping the reader non-blocking.
    emergency: mpsc::Sender<Frame>,
}

impl Agent {
    pub fn new(
        own: Address,
        duties: Arc<dyn Duties>,
        payload: Arc<dyn Payload>,
        lanes: Lanes,
        budget: Budget,
    ) -> (Arc<Self>, Receiver, Arc<Semaphore>) {
        Self::new_with_subscriptions(
            own,
            duties,
            payload,
            lanes,
            budget,
            Subscriptions::default(),
        )
    }

    pub fn new_with_subscriptions(
        own: Address,
        duties: Arc<dyn Duties>,
        payload: Arc<dyn Payload>,
        lanes: Lanes,
        budget: Budget,
        subscriptions: Subscriptions,
    ) -> (Arc<Self>, Receiver, Arc<Semaphore>) {
        let (queue, receiver, in_flight) = channel(lanes, budget);
        let (emergency, mut emergency_rx) = mpsc::channel::<Frame>(budget.depth.max(1));
        let retry_queue = queue.clone();
        let emergency_lost = Arc::new(AtomicUsize::new(0));
        let retry_lost = Arc::clone(&emergency_lost);
        tokio::spawn(async move {
            while let Some(mut frame) = emergency_rx.recv().await {
                loop {
                    match retry_queue.offer(frame) {
                        Ok(()) => break,
                        Err(refused) => {
                            if retry_queue.is_closed() {
                                // The main dispatcher has terminated. Retrying
                                // a closed queue forever strands the task and
                                // gives teardown no completion boundary.
                                retry_lost.fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                            frame = refused.0;
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                        }
                    }
                }
            }
        });
        let agent = Arc::new(Self {
            peers: Peers::new(own.clone()),
            subscriptions,
            own,
            forwarded: AtomicUsize::new(0),
            consumed: AtomicUsize::new(0),
            to_nodes: AtomicUsize::new(0),
            unrouted: AtomicUsize::new(0),
            refused: AtomicUsize::new(0),
            emergency_lost,
            ack_rejected: AtomicUsize::new(0),
            status_sequence: AtomicU64::new(0),
            queue,
            nodes: Mutex::new(HashMap::new()),
            continuations: Continuations::default(),
            duties,
            payload,
            node_queue_depth: budget.depth,
            emergency,
        });
        (agent, receiver, in_flight)
    }

    pub fn address(&self) -> &Address {
        &self.own
    }

    pub fn queue(&self) -> Sender {
        self.queue.clone()
    }

    /// The connections this agent is holding open. A count that only rises
    /// over hours is a peer leak rather than a busy fleet.
    pub fn peers(&self) -> &Peers {
        &self.peers
    }

    pub fn continuations(&self) -> &Continuations<Frame> {
        &self.continuations
    }

    pub async fn deliver_subscription(&self, channel: &str, frame: Frame) -> bool {
        self.subscriptions.deliver(channel, frame).await
    }

    pub async fn acknowledge_subscription(
        &self,
        channel: &str,
        generation: u64,
        stream_id: &str,
        event_seq: u64,
    ) -> bool {
        let accepted = self
            .subscriptions
            .acknowledge(channel, generation, stream_id, event_seq)
            .await;
        if !accepted {
            self.ack_rejected.fetch_add(1, Ordering::Relaxed);
        }
        accepted
    }

    /// Counts an ACK rejected before the subscription registry, such as a
    /// body/envelope channel mismatch. The count is aggregate process-local
    /// telemetry; it is not a durable event or request-level trace.
    pub fn record_ack_rejection(&self) {
        self.ack_rejected.fetch_add(1, Ordering::Relaxed);
    }

    pub fn ack_rejected(&self) -> usize {
        self.ack_rejected.load(Ordering::Relaxed)
    }

    pub async fn subscription_metrics(&self) -> SubscriptionMetrics {
        self.subscriptions.metrics().await
    }

    /// Creates a node. It is an id and an adapter and nothing else until a
    /// load materialises something behind it.
    pub async fn create_node(
        &self,
        id: impl Into<String>,
        adapter: Arc<dyn Adapter>,
        ceiling: usize,
    ) {
        let handle = Node::spawn_with_capacity(
            adapter,
            Arc::clone(&self.payload),
            self.queue.clone(),
            ceiling,
            self.node_queue_depth,
        );
        let old = self.nodes.lock().await.insert(id.into(), handle);
        if let Some(old) = old {
            old.shutdown().await;
        }
    }

    pub async fn delete_node(&self, id: &str) -> bool {
        let Some(node) = self.nodes.lock().await.get(id).cloned() else {
            return false;
        };
        node.shutdown().await;
        let mut nodes = self.nodes.lock().await;
        if nodes
            .get(id)
            .is_some_and(|current| current.is_same_node(&node))
        {
            nodes.remove(id);
        }
        true
    }

    pub async fn node_depth(&self, id: &str) -> Option<usize> {
        Some(self.nodes.lock().await.get(id)?.depth())
    }

    /// Cancels queued work for a route across this agent's nodes.
    ///
    /// Work already inside a backend runs to its hop boundary; cancelling
    /// means the next hop never starts. Returns whether anything was found
    /// waiting, so a caller can tell a cancellation from a request that had
    /// already finished.
    pub async fn cancel(&self, route: &str) -> bool {
        self.cancel_frame(route).await.is_some()
    }

    /// Cancels queued work and returns one removed carrier for terminalizing
    /// the original request. Every node is still scanned so a chained request
    /// cannot leave later queued stages behind.
    // See docs/protocol-outer.md#단절과-kv-흐름.
    pub async fn cancel_frame(&self, route: &str) -> Option<Frame> {
        let mut removed = None;
        self.nodes.lock().await.values().for_each(|handle| {
            if removed.is_none() {
                removed = handle.cancel_frame(route);
            } else {
                let _ = handle.cancel(route);
            }
        });
        removed
    }

    /// Every node, what it is doing, and which routes it is holding.
    ///
    /// The thing a caller needs to answer "where is my request": the counts
    /// say how much passed through, the routes say what is there now.
    pub async fn node_status(&self) -> Vec<NodeStatus> {
        let nodes = self.nodes.lock().await;
        let mut status: Vec<NodeStatus> = nodes
            .iter()
            .map(|(id, handle)| NodeStatus {
                node: id.clone(),
                depth: handle.depth(),
                running: handle.in_adapter(),
                outbox_lost: handle.counts().outbox_lost.load(Ordering::Relaxed),
                waiting: handle.waiting_routes(),
                waiting_requests: handle.waiting_requests(),
                active_hop: handle.active_hop(),
                backend: handle.report(),
            })
            .collect();
        status.sort_by(|left, right| left.node.cmp(&right.node));
        status
    }

    /// What every node on this agent counted at each step it could lose work.
    pub async fn node_counts(&self) -> Vec<String> {
        let nodes = self.nodes.lock().await;
        let mut lines: Vec<String> = nodes
            .iter()
            .map(|(id, handle)| {
                let c = handle.counts();
                let load = |value: &std::sync::atomic::AtomicUsize| value.load(Ordering::Relaxed);
                format!(
                    "node={id} received={} queued={} claimed={} hops={} completions={} outcomes={} routed={} orphaned={} invalid_events={} emitted={} raised={} lost={} blocked={} outbox_lost={} depth={} running={} backend=[{}]",
                    load(&c.received),
                    load(&c.queued),
                    load(&c.claimed),
                    load(&c.hops),
                    load(&c.completions),
                    load(&c.outcomes),
                    load(&c.routed),
                    load(&c.orphaned),
                    load(&c.invalid_events),
                    load(&c.emitted),
                    c.raised.load(Ordering::Relaxed),
                    c.lost.load(Ordering::Relaxed),
                    c.blocked.load(Ordering::Relaxed),
                    c.outbox_lost.load(Ordering::Relaxed),
                    handle.depth(),
                    handle.in_adapter(),
                    // The same words the protocol carries. An operator at the
                    // machine and a caller on another one must not have to
                    // compare two different accounts of the same node.
                    handle.report(),
                )
            })
            .collect();
        lines.sort();
        lines
    }

    /// Total work sitting on node queues. Read beside the agent queue's depth,
    /// the two say which side of the adapter boundary is slow.
    pub async fn node_depth_total(&self) -> usize {
        self.nodes
            .lock()
            .await
            .values()
            .map(|handle| handle.depth())
            .sum()
    }

    /// Puts a frame on this agent's own queue. Used by duties answering a
    /// message, because a response is a message like any other.
    ///
    // The refused frame is the error, because nothing on this path may drop:
    // a caller has to be handed back what it could not enqueue.
    #[allow(clippy::result_large_err)]
    pub fn enqueue(&self, frame: Frame) -> Result<(), Frame> {
        self.queue.offer(frame).map_err(|refused| refused.0)
    }

    /// Hands a frame to the connection for its target.
    ///
    /// Done in the order frames left the queue, not in a task of its own.
    /// Registration order is what guarantees ordering here — a node enqueues a
    /// token before the lap that will produce the next one — and dispatching
    /// each frame into its own task would let two frames of one route race,
    /// which is visible to a caller as P4 reordering its stream. Forwarding is
    /// a hand-off to a per-peer queue, so keeping it in line costs nothing.
    async fn forward(&self, frame: Frame) {
        if let Err(returned) = self.peers.send(frame).await {
            self.answer_locally(returned, "peer queue is full");
        }
    }

    /// One message, two decisions.
    async fn dispatch(self: &Arc<Self>, frame: Frame) {
        let verdict = judge(&frame.envelope, &self.own);
        if routing_trace_enabled() {
            eprintln!(
                "P4_AGENT_ROUTE own={} target={} recipient={:?} lane={:?} route={} verdict={:?}",
                self.own,
                frame.envelope.target,
                frame.envelope.recipient,
                frame.envelope.lane,
                frame.envelope.route,
                verdict,
            );
        }
        match verdict {
            Verdict::Forward(_) => {
                self.forwarded.fetch_add(1, Ordering::Relaxed);
                self.forward(frame).await
            }
            Verdict::Agent => {
                self.consumed.fetch_add(1, Ordering::Relaxed);
                self.consume(frame).await
            }
            Verdict::Node(id) => {
                let handle = self.nodes.lock().await.get(&id).cloned();
                match handle {
                    // Moving it in is the whole of the worker's job here. It
                    // does not wait to see what the node makes of it.
                    Some(handle) => {
                        self.to_nodes.fetch_add(1, Ordering::Relaxed);
                        if let Err(refused) = handle.offer(frame) {
                            self.answer_locally(refused, "node ingress is full");
                        }
                    }
                    None => {
                        self.unrouted.fetch_add(1, Ordering::Relaxed);
                        self.answer_locally(frame, "no such node on this agent")
                    }
                }
            }
        }
    }

    async fn consume(self: &Arc<Self>, frame: Frame) {
        // A response is the reply to something this agent asked for, so the
        // continuation registered at send time is what it belongs to. An
        // unclaimed one falls through to duties rather than being dropped.
        if frame.envelope.lane == QueueClass::Response {
            if self
                .continuations
                .resolve(&frame.envelope.return_key(), frame.clone())
                || self
                    .continuations
                    .resolve(&frame.envelope.route, frame.clone())
            {
                return;
            }
            if let Some(channel) = frame.envelope.return_channel.as_deref()
                && self.deliver_subscription(channel, frame.clone()).await
            {
                return;
            }
        }
        self.duties.handle(frame, self);
    }

    fn answer_locally(&self, carrier: Frame, detail: &str) {
        self.refused.fetch_add(1, Ordering::Relaxed);
        let Some(mut envelope) = carrier.envelope.to_reply() else {
            return;
        };
        // A refusal is a terminal response event. Give a request that has not
        // emitted an event yet its first sequence number so subscription
        // replay can retain it; later refusals advance the carrier sequence.
        envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
        let frame = Frame {
            envelope,
            body: self.payload.failure(detail),
        };
        if self.emergency.try_send(frame).is_err() {
            self.emergency_lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Drains the queue forever across a pool of workers.
///
/// Workers are chosen by route, not taken at random. Order within a route is
/// what the CPS rule buys — a node enqueues a token before the lap that will
/// produce the next one — and handing consecutive frames of one route to
/// different workers throws that away, which a caller sees as P4 reordering
/// its stream. Routing by hash keeps each route sequential while leaving
/// different routes free to run at once.
pub async fn run(agent: Arc<Agent>, mut receiver: Receiver, in_flight: Arc<Semaphore>) {
    let workers = worker_count(in_flight.available_permits());
    let mut lanes = Vec::with_capacity(workers);
    for _ in 0..workers {
        let (sender, mut work) = mpsc::channel::<Frame>(WORKER_DEPTH);
        let agent = Arc::clone(&agent);
        tokio::spawn(async move {
            while let Some(frame) = work.recv().await {
                agent.dispatch(frame).await;
            }
        });
        lanes.push(sender);
    }
    while let Some(frame) = receiver.take().await {
        // A node is itself a single scheduling boundary. Hashing its frames
        // by route would let two different Restore requests race through
        // different workers and arrive at that boundary backwards. Keep all
        // work for one node on one worker; different nodes still run in
        // parallel, and OUTER/peer traffic retains route sharding.
        let worker = match &frame.envelope.recipient {
            Recipient::Node(node) => &lanes[node_worker(&frame.envelope.target, node, workers)],
            _ => &lanes[route_worker(&frame.envelope.route, workers)],
        };
        match worker.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(frame)) => {
                // Never await one hashed worker here: doing so creates
                // head-of-line blocking for every other route. The worker
                // inbox is deliberately bounded, so refusal is the only
                // bounded policy that preserves per-route ordering.
                agent.answer_locally(frame, "worker queue is full");
            }
            Err(mpsc::error::TrySendError::Closed(frame)) => {
                agent.answer_locally(frame, "worker is closed");
            }
        }
    }
}

/// Depth of one worker's inbox. Small: a deep worker inbox would move the
/// queueing decision out of the lanes that were sized for it.
const WORKER_DEPTH: usize = 64;

fn worker_count(in_flight: usize) -> usize {
    in_flight
        .min(
            std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(1)
                * 2,
        )
        .max(1)
}

/// Cheap, stable, and dependent on nothing but the route.
fn route_worker(route: &str, workers: usize) -> usize {
    let mut hash: u64 = 1469598103934665603;
    for byte in route.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    (hash % workers as u64) as usize
}

fn node_worker(target: &Address, node: &str, workers: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    target.hash(&mut hasher);
    node.hash(&mut hasher);
    (hasher.finish() % workers as u64) as usize
}

fn routing_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("P4_AGENT_TRACE_ROUTING").is_some())
}

#[cfg(test)]
mod worker_tests;

#[cfg(test)]
mod tests;

impl Agent {
    /// What the two decisions did, since the process started.
    ///
    /// Counted because a frame that goes missing leaves nothing else behind:
    /// depth only says what is waiting, never what already left.
    pub fn traffic(&self) -> Traffic {
        Traffic {
            forwarded: self.forwarded.load(Ordering::Relaxed),
            consumed: self.consumed.load(Ordering::Relaxed),
            to_nodes: self.to_nodes.load(Ordering::Relaxed),
            unrouted: self.unrouted.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            emergency_lost: self.emergency_lost.load(Ordering::Relaxed),
        }
    }

    /// Monotonic sequence for machine-readable status snapshots.
    pub fn next_status_sequence(&self) -> u64 {
        self.status_sequence.fetch_add(1, Ordering::Relaxed) + 1
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traffic {
    pub forwarded: usize,
    pub consumed: usize,
    pub to_nodes: usize,
    pub unrouted: usize,
    pub refused: usize,
    pub emergency_lost: usize,
}

/// One node, as of the moment it was asked.
///
/// `waiting` is the part a count cannot give: which requests are sitting on
/// this node right now, in the order it will take them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeStatus {
    pub node: String,
    pub depth: usize,
    /// How many sequences are inside the backend at this moment.
    ///
    /// A count rather than a flag, because the flag could not answer the
    /// question it was there for. What the ceiling bounds is how many a node
    /// hands over at once, and "something is running" is equally true at one
    /// and at a hundred — an operator watching a queue drain cannot tell from
    /// it whether the ceiling is being kept, which is the whole claim that
    /// P4 queues rather than the backend.
    pub running: usize,
    /// Node output frames that did not reach the downstream agent queue.
    /// This is an aggregate local counter, not OUTER delivery proof.
    pub outbox_lost: usize,
    pub waiting: Vec<String>,
    /// Request identities for the queued work represented by `waiting`.
    pub waiting_requests: Vec<WaitingRequest>,
    /// The hop currently handed to the adapter, if any.
    pub active_hop: Option<ActiveHop>,
    /// What the backend behind this node says it is doing, in its own words.
    /// Carried, never interpreted.
    pub backend: String,
}
