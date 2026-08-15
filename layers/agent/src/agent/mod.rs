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
use tokio::sync::{Mutex, Semaphore};

/// What the agent itself does with a message addressed to it.
///
/// Node creation and deletion, inference intake and hardware inspection live
/// behind this, so the core never learns a message catalog. It is a procedure:
/// its output is whatever it puts on the queue.
pub trait Duties: Send + Sync {
    fn handle(&self, frame: Frame, agent: &Agent);
}

pub struct Agent {
    own: Address,
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
    pub async fn create_node(&self, id: impl Into<String>, adapter: Arc<dyn Adapter>, ceiling: usize) {
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

    /// One message, one worker, two decisions.
    async fn dispatch(self: &Arc<Self>, frame: Frame) {
        match judge(&frame.envelope, &self.own) {
            Verdict::Forward(_) => {
                if let Err(returned) = self.peers.send(frame).await {
                    self.answer_locally(returned, "peer queue is full");
                }
            }
            Verdict::Agent => self.consume(frame).await,
            Verdict::Node(id) => {
                let handle = self.nodes.lock().await.get(&id).cloned();
                match handle {
                    // Moving it in is the whole of the worker's job here. It
                    // does not wait to see what the node makes of it.
                    Some(handle) => handle.offer(frame),
                    None => self.answer_locally(frame, "no such node on this agent"),
                }
            }
        }
    }

    async fn consume(self: &Arc<Self>, frame: Frame) {
        // A response is the reply to something this agent asked for, so the
        // continuation registered at send time is what it belongs to. An
        // unclaimed one falls through to duties rather than being dropped.
        if frame.envelope.lane == QueueClass::Response
            && self.continuations.resolve(&frame.envelope.route.clone(), frame.clone())
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

/// Drains the queue forever, bounded by the in-flight budget.
///
/// The budget is taken before the work is spawned, so how much this agent is
/// doing at once stays independent of how much it is remembering.
pub async fn run(agent: Arc<Agent>, mut receiver: Receiver, in_flight: Arc<Semaphore>) {
    while let Some(frame) = receiver.take().await {
        let Ok(permit) = Arc::clone(&in_flight).acquire_owned().await else {
            return;
        };
        let agent = Arc::clone(&agent);
        tokio::spawn(async move {
            agent.dispatch(frame).await;
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests;
