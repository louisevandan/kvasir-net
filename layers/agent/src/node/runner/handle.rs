//! What the agent holds, and what it can read off a node.
//!
//! Its own file because it changes for a different reason than the loop next
//! door: this is the surface an operator and the agent see — depth, what is
//! inside the adapter, which routes are waiting, what passed through — and it
//! moves when a question needs answering rather than when scheduling changes.
//!
//! A frame that goes missing leaves no trace in a depth reading, because depth
//! only shows what is still waiting. The counts are what show it passed.

use crate::node::queue::NodeQueue;
use p4_protocol::frame::Frame;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::sync::mpsc;

/// What a caller keeps to feed a running node.
#[derive(Clone)]
pub struct Handle {
    /// Asked on a status request, and only then.
    pub(super) backend: Arc<dyn p4_adapter::Adapter>,
    work: mpsc::Sender<Frame>,
    queue: Arc<NodeQueue>,
    counts: Arc<Counts>,
}

/// What the node did, counted at each step it could lose something.
///
/// A frame that goes missing leaves no trace in a depth reading, because
/// depth only shows what is still waiting. These show what passed through.
#[derive(Default)]
pub struct Counts {
    pub received: AtomicUsize,
    pub queued: AtomicUsize,
    pub claimed: AtomicUsize,
    pub hops: AtomicUsize,
    pub completions: AtomicUsize,
    pub outcomes: AtomicUsize,
    pub routed: AtomicUsize,
    pub orphaned: AtomicUsize,
    pub emitted: AtomicUsize,
    pub raised: Arc<AtomicUsize>,
    pub lost: Arc<AtomicUsize>,
}

impl Handle {
    pub(super) fn new(
        work: mpsc::Sender<Frame>,
        queue: Arc<NodeQueue>,
        counts: Arc<Counts>,
        backend: Arc<dyn p4_adapter::Adapter>,
    ) -> Self {
        Self {
            work,
            queue,
            counts,
            backend,
        }
    }

    /// What the backend behind this node says it is doing. Relayed, never
    /// read: the core has no idea what the string means, which is the same
    /// arrangement a plan has going the other way.
    pub fn report(&self) -> String {
        self.backend.report()
    }

    /// Moves work to this node without waiting. A full ingress channel is
    /// returned to the caller so the worker can report refusal immediately.
    pub fn offer(&self, frame: Frame) -> Result<(), Frame> {
        self.work.try_send(frame).map_err(|error| match error {
            mpsc::error::TrySendError::Full(frame) => frame,
            mpsc::error::TrySendError::Closed(frame) => frame,
        })
    }

    /// How deep this node is. Read beside the agent's queue depth, the pair
    /// says whether a slowdown is above or below the adapter boundary.
    pub fn depth(&self) -> usize {
        self.queue.depth()
    }

    pub fn is_running(&self) -> bool {
        self.queue.is_running()
    }

    /// How many sequences this node has handed to the adapter right now.
    ///
    /// The load ceiling bounds adapter admission; node queue capacity bounds
    /// waiting memory. Both are observable separately.
    pub fn in_adapter(&self) -> usize {
        self.queue.in_adapter()
    }

    pub fn counts(&self) -> &Counts {
        &self.counts
    }

    /// Drops queued work for a route.
    ///
    /// A hop already handed to a backend is not interrupted, because there is
    /// no way to interrupt one and no need: not starting the next is the whole
    /// mechanism. So this cancels what has not started, and returns whether
    /// anything was still waiting.
    pub fn cancel(&self, route: &str) -> bool {
        self.queue.remove(route).is_some()
    }

    /// The routes this node is holding, in the order it would take them.
    ///
    /// A caller asking "where has my request got to" is asking this. Depth
    /// alone answers how many, never which, and a route that has gone missing
    /// is exactly the one a count cannot show.
    pub fn waiting_routes(&self) -> Vec<String> {
        self.queue
            .waiting()
            .into_iter()
            .map(|waiting| waiting.route)
            .collect()
    }
}
