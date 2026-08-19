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
use p4_protocol::{QueueClass, frame::Frame};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::sync::{mpsc, watch};

/// What a caller keeps to feed a running node.
#[derive(Clone)]
pub struct Handle {
    /// Asked on a status request, and only then.
    pub(super) backend: Arc<dyn p4_adapter::Adapter>,
    work: mpsc::Sender<Frame>,
    queue: Arc<NodeQueue>,
    counts: Arc<Counts>,
    active: Arc<std::sync::Mutex<Option<ActiveHop>>>,
    admission_closed: Arc<std::sync::Mutex<bool>>,
    stop: watch::Sender<bool>,
    done: watch::Sender<bool>,
    outbox_done: watch::Sender<bool>,
    outbox_stop: watch::Sender<bool>,
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
    /// Adapter completion rejected because its expected/outcome set was not
    /// an exact match for the active hop.
    pub invalid_events: AtomicUsize,
    pub emitted: AtomicUsize,
    pub raised: Arc<AtomicUsize>,
    pub lost: Arc<AtomicUsize>,
    /// Node output frames accepted by the node but lost because the bounded
    /// outbox's downstream queue closed or teardown deliberately interrupted
    /// delivery after its bounded shutdown grace period.
    pub outbox_lost: Arc<AtomicUsize>,
}

impl Handle {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        work: mpsc::Sender<Frame>,
        queue: Arc<NodeQueue>,
        counts: Arc<Counts>,
        backend: Arc<dyn p4_adapter::Adapter>,
        active: Arc<std::sync::Mutex<Option<ActiveHop>>>,
        admission_closed: Arc<std::sync::Mutex<bool>>,
        stop: watch::Sender<bool>,
        done: watch::Sender<bool>,
        outbox_done: watch::Sender<bool>,
        outbox_stop: watch::Sender<bool>,
    ) -> Self {
        Self {
            work,
            queue,
            counts,
            backend,
            active,
            admission_closed,
            stop,
            done,
            outbox_done,
            outbox_stop,
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
    #[allow(clippy::result_large_err)]
    pub fn offer(&self, frame: Frame) -> Result<(), Frame> {
        let admission = self.admission_closed.lock().expect("node admission lock");
        if *admission {
            return Err(frame);
        }
        // Keep the fence held through the non-blocking send. Otherwise a
        // shutdown can drain the node between the closed check and try_send,
        // leaving a late frame in a receiver that is about to disappear.
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

    pub(crate) fn is_same_node(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.queue, &other.queue)
    }

    /// Explicitly stops the runner and waits for queued, active, and
    /// lifecycle carriers to receive a terminal failure within the bounded
    /// shutdown window. A timeout returns local completion without proving
    /// downstream or OUTER delivery.
    pub async fn shutdown(&self) {
        let mut done = self.done.subscribe();
        let mut outbox_done = self.outbox_done.subscribe();
        {
            let mut closed = self.admission_closed.lock().expect("node admission lock");
            if !*closed {
                *closed = true;
                let _ = self.stop.send(true);
            }
        }
        if !*done.borrow()
            && tokio::time::timeout(std::time::Duration::from_secs(1), done.changed())
                .await
                .is_err()
        {
            // Give teardown replies a bounded opportunity to enter the
            // outbox. Only after the runner itself remains blocked do we
            // cancel an emit and force the outbox pump to account the loss.
            let _ = self.outbox_stop.send(true);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), done.changed()).await;
        }
        if !*outbox_done.borrow() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), outbox_done.changed())
                .await;
        }
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
        self.cancel_frame(route).is_some()
    }

    /// Removes queued work and returns its carrier so the owner can emit the
    /// terminal cancellation result on the original reply path.
    pub fn cancel_frame(&self, route: &str) -> Option<Frame> {
        self.queue.remove(route)
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

    /// Request identity for work still waiting at this node. This is a
    /// monitoring projection; scheduling continues to use the opaque frame.
    pub fn waiting_requests(&self) -> Vec<WaitingRequest> {
        self.queue
            .frames()
            .into_iter()
            .map(|frame| WaitingRequest {
                route: frame.envelope.route,
                request_id: frame.envelope.request_id,
                stream_id: frame.envelope.stream_id,
                lane: frame.envelope.lane,
                deadline_unix_ms: frame.envelope.deadline_unix_ms,
            })
            .collect()
    }

    pub fn active_hop(&self) -> Option<ActiveHop> {
        self.active.lock().expect("active telemetry lock").clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitingRequest {
    pub route: String,
    pub request_id: String,
    pub stream_id: String,
    pub lane: QueueClass,
    pub deadline_unix_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveHop {
    pub id: u64,
    pub deployment: String,
    pub phase: p4_adapter::Phase,
    /// Cancellation was requested, but the adapter has not emitted its
    /// terminal event. The node remains occupied while this is true.
    pub timed_out: bool,
    pub requests: Vec<WaitingRequest>,
}
