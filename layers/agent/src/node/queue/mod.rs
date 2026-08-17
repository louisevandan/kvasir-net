//! A node's own queue, and the fact that a hop is running.
//!
//! Holding work here is what releases the agent's worker. The worker moves a
//! message in and is done; whether the node acts in a microsecond or a minute
//! is no longer anything the agent's queue depth reflects. That separation is
//! the whole reason a slowdown can be attributed.

use crate::node::window::Waiting;
use p4_protocol::frame::Frame;
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

/// Work waiting for a hop, in arrival order within its lane. Its capacity is
/// the agent budget, so sustained arrivals cannot create unbounded node memory.
pub struct NodeQueue {
    waiting: Mutex<VecDeque<Frame>>,
    max_depth: usize,
    /// How many sequences are inside the adapter, which is the width of the
    /// window running now and zero between hops. A node runs one hop at a time
    /// by construction: it starts the next only when it observes the previous
    /// end, so a backend never receives overlapping work for one deployment.
    ///
    /// A count rather than a flag, because the flag could not answer the
    /// question it existed for. What the ceiling bounds is how many go over at
    /// once, and "something is running" is equally true at one and at a
    /// hundred — so an operator watching a backlog could not tell from it
    /// whether the ceiling was being kept, which is the whole claim that this
    /// queue holds the excess rather than the backend.
    running: Mutex<usize>,
}

impl NodeQueue {
    pub fn with_capacity(max_depth: usize) -> Self {
        Self {
            waiting: Mutex::new(VecDeque::new()),
            max_depth: max_depth.max(1),
            running: Mutex::new(0),
        }
    }

    pub fn push(&self, frame: Frame) -> bool {
        let mut waiting = self.waiting.lock().expect("node queue lock");
        if waiting.len() >= self.max_depth {
            return false;
        }
        waiting.push_back(frame);
        true
    }

    /// How deep this node is. The node-side half of the pair that attributes a
    /// slowdown: deep here beside a shallow agent queue puts the cause below
    /// P4.
    pub fn depth(&self) -> usize {
        self.waiting.lock().expect("node queue lock").len()
    }

    pub fn is_running(&self) -> bool {
        self.in_adapter() > 0
    }

    /// How many sequences this node has handed to the adapter right now.
    ///
    /// The ceiling bounds this and nothing else does: a backend is never asked
    /// to refuse and never told how much is waiting. Reported so that claim is
    /// checkable from outside rather than only readable in the code.
    pub fn in_adapter(&self) -> usize {
        *self.running.lock().expect("node running lock")
    }

    /// The scheduling view of what is waiting, for composing a window.
    pub fn waiting(&self) -> Vec<Waiting> {
        self.waiting
            .lock()
            .expect("node queue lock")
            .iter()
            .map(|frame| Waiting {
                route: frame.envelope.route.clone(),
                lane: frame.envelope.lane,
                deadline_unix_ms: frame.envelope.deadline_unix_ms,
            })
            .collect()
    }

    /// Removes the named routes and marks a hop as running.
    ///
    /// One call so the two cannot drift apart: taking work without marking
    /// would let a second hop start beside the first. One pass over the queue,
    /// not one per route.
    ///
    /// A window is as wide as the declared ceiling, and searching the queue
    /// once per named route made claiming quadratic in that width — which does
    /// not show at a ceiling of eight and dominates everything at a thousand.
    pub fn claim(&self, routes: &[String]) -> Vec<Frame> {
        let wanted: HashSet<&str> = routes.iter().map(String::as_str).collect();
        let mut waiting = self.waiting.lock().expect("node queue lock");
        let mut claimed = Vec::with_capacity(routes.len());
        let mut kept = VecDeque::with_capacity(waiting.len());
        for frame in waiting.drain(..) {
            if wanted.contains(frame.envelope.route.as_str()) {
                claimed.push(frame);
            } else {
                kept.push_back(frame);
            }
        }
        *waiting = kept;
        if !claimed.is_empty() {
            *self.running.lock().expect("node running lock") = claimed.len();
        }
        claimed
    }

    /// Marks the running hop finished. Called when the adapter says so, and
    /// only then — a timer here would let the node start work beside work.
    pub fn finished(&self) {
        *self.running.lock().expect("node running lock") = 0;
    }

    /// The first waiting frame a predicate accepts.
    ///
    /// One pass, and it clones only what it returns. Asking route by route
    /// meant a scan per item and a clone per hit, which is quadratic in queue
    /// depth on a path that runs after every single event.
    pub fn find(&self, accepts: impl Fn(&Frame) -> bool) -> Option<Frame> {
        self.waiting
            .lock()
            .expect("node queue lock")
            .iter()
            .find(|frame| accepts(frame))
            .cloned()
    }

    /// Removes work whose route matches, for cancellation and expiry.
    pub fn remove(&self, route: &str) -> Option<Frame> {
        let mut waiting = self.waiting.lock().expect("node queue lock");
        let index = waiting
            .iter()
            .position(|frame| frame.envelope.route == route)?;
        waiting.remove(index)
    }
}

impl Default for NodeQueue {
    fn default() -> Self {
        Self::with_capacity(4096)
    }
}

#[cfg(test)]
mod tests;
