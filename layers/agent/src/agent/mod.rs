//! The agent: everything above, wired together.
//!
//! A worker takes a frame, makes the two decisions, and is done. Forwarding
//! hands the frame to a peer connection. Node-bound work is moved to that
//! node's queue. Only what the agent itself owns is handled here, and even
//! that never blocks — it enqueues.

use crate::continuation::Continuations;
use crate::node::payload::Payload;
use crate::node::runner::{Handle, Node};
use crate::queue::lane::{Budget, Lanes};
use crate::queue::main::{Receiver, Sender, channel};
use crate::transport::outbound::Peers;
use crate::worker::judge::{Verdict, judge};
use p4_adapter::Adapter;
use p4_protocol::frame::Frame;
use p4_protocol::{Address, QueueClass};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    queue: Sender,
    peers: Peers,
    nodes: Mutex<HashMap<String, Handle>>,
    continuations: Continuations<Frame>,
    duties: Arc<dyn Duties>,
    payload: Arc<dyn Payload>,
}

impl Agent {
    pub fn new(
        own: Address,
        duties: Arc<dyn Duties>,
        payload: Arc<dyn Payload>,
        lanes: Lanes,
        budget: Budget,
    ) -> (Arc<Self>, Receiver, Arc<Semaphore>) {
        let (queue, receiver, in_flight) = channel(lanes, budget);
        let agent = Arc::new(Self {
            own,
            forwarded: AtomicUsize::new(0),
            consumed: AtomicUsize::new(0),
            to_nodes: AtomicUsize::new(0),
            unrouted: AtomicUsize::new(0),
            queue,
            peers: Peers::default(),
            nodes: Mutex::new(HashMap::new()),
            continuations: Continuations::default(),
            duties,
            payload,
        });
        (agent, receiver, in_flight)
    }

    pub fn address(&self) -> &Address {
        &self.own
    }

    pub fn queue(&self) -> Sender {
        self.queue.clone()
    }

    pub fn continuations(&self) -> &Continuations<Frame> {
        &self.continuations
    }

    /// Creates a node. It is an id and an adapter and nothing else until a
    /// load materialises something behind it.
    pub async fn create_node(
        &self,
        id: impl Into<String>,
        adapter: Arc<dyn Adapter>,
        ceiling: usize,
    ) {
        let handle = Node::spawn(
            adapter,
            Arc::clone(&self.payload),
            self.queue.clone(),
            ceiling,
        );
        self.nodes.lock().await.insert(id.into(), handle);
    }

    pub async fn delete_node(&self, id: &str) -> bool {
        self.nodes.lock().await.remove(id).is_some()
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
        self.nodes
            .lock()
            .await
            .values()
            .fold(false, |found, handle| handle.cancel(route) || found)
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
                    "node={id} received={} queued={} claimed={} hops={} completions={} outcomes={} routed={} orphaned={} emitted={} raised={} lost={} depth={} running={}",
                    load(&c.received),
                    load(&c.queued),
                    load(&c.claimed),
                    load(&c.hops),
                    load(&c.completions),
                    load(&c.outcomes),
                    load(&c.routed),
                    load(&c.orphaned),
                    load(&c.emitted),
                    c.raised.load(Ordering::Relaxed),
                    c.lost.load(Ordering::Relaxed),
                    handle.depth(),
                    handle.is_running(),
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
        match judge(&frame.envelope, &self.own) {
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
                        handle.offer(frame)
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
        if frame.envelope.lane == QueueClass::Response
            && self
                .continuations
                .resolve(&frame.envelope.route.clone(), frame.clone())
        {
            return;
        }
        self.duties.handle(frame, self);
    }

    fn answer_locally(&self, carrier: Frame, detail: &str) {
        let Some(envelope) = carrier.envelope.to_reply() else {
            return;
        };
        let _ = self.queue.offer(Frame {
            envelope,
            body: detail.as_bytes().to_vec(),
        });
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
        let worker = &lanes[route_worker(&frame.envelope.route, workers)];
        if worker.send(frame).await.is_err() {
            return;
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
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traffic {
    pub forwarded: usize,
    pub consumed: usize,
    pub to_nodes: usize,
    pub unrouted: usize,
}
