//! The node's event loop.
//!
//! It advances on two events and no others: work arriving, and a hop ending.
//! There is no timer and no polling, because a node's pace is the backend's
//! pace and inventing a third trigger would mean guessing at it.

pub mod bound;
pub mod events;
pub mod handle;

pub use handle::{Counts, Handle};

use bound::Bound;
use events::Sink;

use crate::node::payload::Payload;
use crate::node::queue::NodeQueue;
use crate::node::window::{compose, expired_items};
use crate::queue::main::Sender;
use p4_adapter::{Adapter, Event, Hop, Phase, Work};
use p4_protocol::QueueClass;
use p4_protocol::frame::Frame;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

pub struct Node {
    queue: Arc<NodeQueue>,
    adapter: Arc<dyn Adapter>,
    payload: Arc<dyn Payload>,
    /// What the load declared. A ceiling: never derived here, never exceeded.
    ceiling: Mutex<usize>,
    /// Frames of the hop in flight, keyed by the sequence id they were given,
    /// so an outcome can be matched back to the route it came from.
    in_flight: Mutex<HashMap<String, Frame>>,
    /// The load or unload in flight, if any. Kept apart from `in_flight`
    /// because it belongs to a deployment rather than to a sequence.
    lifecycle: Mutex<Option<Frame>>,
    /// Whether this node may serve, and for which deployment.
    bound: Mutex<Bound>,
    events: Sink,
    counts: Arc<Counts>,
    /// Frames on their way to the agent queue. Unbounded, and bounded in
    /// practice by the work in flight, because everything here was produced by
    /// a hop this node already admitted.
    outbox: mpsc::UnboundedSender<Frame>,
}

impl Node {
    /// Starts the node and returns the handle used to feed it.
    /// What the backend behind this node says it is doing.
    ///
    /// Relayed, never read. The core has no idea what the string means, which
    /// is the same arrangement a plan has going the other way.
    pub fn report(&self) -> String {
        self.adapter.report()
    }

    pub fn spawn(
        adapter: Arc<dyn Adapter>,
        payload: Arc<dyn Payload>,
        out: Sender,
        ceiling: usize,
    ) -> Handle {
        Self::spawn_with_capacity(adapter, payload, out, ceiling, 4096)
    }

    pub fn spawn_with_capacity(
        adapter: Arc<dyn Adapter>,
        payload: Arc<dyn Payload>,
        out: Sender,
        ceiling: usize,
        max_queue_depth: usize,
    ) -> Handle {
        let (work_tx, work_rx) = mpsc::channel(max_queue_depth.max(1));
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (outbox_tx, mut outbox_rx) = mpsc::unbounded_channel::<Frame>();
        let queue = Arc::new(NodeQueue::with_capacity(max_queue_depth));
        let reporting = Arc::clone(&adapter);
        let counts = Arc::new(Counts::default());
        let queued = out;
        let node = Node {
            queue: Arc::clone(&queue),
            adapter,
            payload,
            ceiling: Mutex::new(ceiling.max(1)),
            in_flight: Mutex::new(HashMap::new()),
            lifecycle: Mutex::new(None),
            bound: Mutex::new(Bound::Never),
            events: Sink::new(
                event_tx,
                Arc::clone(&counts.raised),
                Arc::clone(&counts.lost),
            ),
            counts: Arc::clone(&counts),
            outbox: outbox_tx,
        };
        tokio::spawn(async move {
            // Waits for room rather than dropping. A node that outruns its
            // agent is held here, which slows its next hop without ever
            // blocking the task that has to see that hop end.
            while let Some(frame) = outbox_rx.recv().await {
                if queued.send(frame).await.is_err() {
                    return;
                }
            }
        });
        tokio::spawn(node.run(work_rx, event_rx));
        Handle::new(work_tx, queue, counts, reporting)
    }

    async fn run(
        self,
        mut work: mpsc::Receiver<Frame>,
        mut events: mpsc::UnboundedReceiver<Event>,
    ) {
        loop {
            // Deliberately not biased. Preferring events looked right — a hop
            // ending is what frees the node — but a node under load produces
            // an event per hop without pause, and a biased select then never
            // polls arrivals at all. They sat unread in the channel: not in the
            // node's queue, not in the agent's, invisible to every depth
            // reading, and the request behind each one never answered.
            //
            // Fairness costs nothing here. An event still reaches `on_event`
            // on the next turn of the loop, and the hop it reports has already
            // finished by the time it was sent.
            tokio::select! {
                Some(event) = events.recv() => self.on_event(event),
                frame = work.recv() => match frame {
                    Some(frame) => {
                        self.counts.received.fetch_add(1, Ordering::Relaxed);
                        if let Some(refused) = self.refusal(&frame) {
                            self.reply_error(&frame, &refused);
                            continue;
                        }
                        if !self.queue.push(frame.clone()) {
                            self.reply_error(&frame, "node queue is full");
                            continue;
                        }
                        self.counts.queued.fetch_add(1, Ordering::Relaxed);
                        self.drain();
                    }
                    // The handle is gone: this node was deleted or replaced,
                    // and nothing can reach it again. Returning is what frees
                    // it — the node holds its own event sender, so waiting for
                    // that channel to close waits forever. Written as a
                    // disabled `Some(...)` branch it parked here instead,
                    // keeping the adapter, the queue and the in-flight map for
                    // the life of the process. Every replaced node was still
                    // resident; the count only ever went up.
                    None => return,
                },
                else => return,
            }
        }
    }

    /// Why this frame must not run here, if it must not.
    ///
    /// Only sequence work is checked. Lifecycle is how a node stops being
    /// refused, so a load that had to pass the check to be allowed to fix the
    /// thing the check is complaining about could never succeed.
    fn refusal(&self, frame: &Frame) -> Option<String> {
        let generation = frame.envelope.chain.as_ref()?.current().generation;
        if self.payload.lifecycle(frame).is_some() {
            return None;
        }
        let bound = *self.bound.lock().expect("generation lock");
        (!bound.admits(generation)).then(|| bound.why(generation))
    }

    /// Starts a hop if one can start. Called after every event, which is what
    /// makes progress event-driven rather than timed.
    fn drain(&self) {
        if self.queue.is_running() {
            return;
        }
        let now = now_unix_ms();
        for stale in expired_items(&self.queue.waiting(), now) {
            if let Some(frame) = self.queue.remove(&stale.route) {
                self.reply_error(&frame, "deadline passed before this work started");
            }
        }
        if self.start_lifecycle() {
            return;
        }
        let ceiling = *self.ceiling.lock().expect("ceiling lock");
        let Some(window) = compose(&self.queue.waiting(), ceiling, now) else {
            return;
        };
        let routes: Vec<String> = window.items.iter().map(|item| item.route.clone()).collect();
        let claimed = self.queue.claim(&routes);
        if claimed.is_empty() {
            return;
        }
        self.counts
            .claimed
            .fetch_add(claimed.len(), Ordering::Relaxed);
        match self.hop(&claimed, window.lane) {
            Some(hop) => {
                let mut in_flight = self.in_flight.lock().expect("in-flight lock");
                for (frame, sequence) in claimed.iter().zip(hop.sequences.iter()) {
                    in_flight.insert(sequence.sequence.clone(), frame.clone());
                }
                drop(in_flight);
                // An adapter is a procedure and is allowed to block — a real
                // one waits on a device. Running it on a blocking thread is
                // what keeps that from stalling the workers that still have to
                // relay and answer while this node is busy.
                let adapter = Arc::clone(&self.adapter);
                let events = self.events.clone();
                self.counts.hops.fetch_add(1, Ordering::Relaxed);
                tokio::task::spawn_blocking(move || {
                    adapter.start(Work::Hop(hop), &events);
                });
            }
            None => {
                // Nothing executable came out of the window. The node is not
                // running after all, so it must be released or it stalls.
                self.queue.finished();
                for frame in claimed {
                    self.reply_error(&frame, "work could not be read as a sequence");
                }
                self.drain();
            }
        }
    }

    /// Runs a waiting load or unload, alone.
    ///
    /// Lifecycle never shares a hop: materialising or releasing a model is one
    /// instruction about a whole deployment, and batching it beside sequences
    /// would let execution start against something half-built.
    fn start_lifecycle(&self) -> bool {
        let Some(frame) = self
            .queue
            .find(|frame| self.payload.lifecycle(frame).is_some())
        else {
            return false;
        };
        let Some(work) = self.payload.lifecycle(&frame) else {
            return false;
        };
        let route = frame.envelope.route.clone();
        let claimed = self.queue.claim(&[route]);
        if claimed.is_empty() {
            return false;
        }
        if let Some(ceiling) = self.payload.ceiling(&frame) {
            *self.ceiling.lock().expect("ceiling lock") = ceiling.max(1);
        }
        *self.lifecycle.lock().expect("lifecycle lock") = Some(frame);
        let adapter = Arc::clone(&self.adapter);
        let events = self.events.clone();
        tokio::task::spawn_blocking(move || {
            adapter.start(work, &events);
        });
        true
    }

    fn hop(&self, claimed: &[Frame], lane: QueueClass) -> Option<Hop> {
        let deployment = self.payload.deployment(claimed.first()?)?;
        let sequences: Vec<_> = claimed
            .iter()
            .filter_map(|frame| self.payload.sequence(frame))
            .collect();
        if sequences.len() != claimed.len() {
            return None;
        }
        Some(Hop {
            deployment,
            phase: match lane {
                QueueClass::Decode => Phase::Decode,
                _ => Phase::Prefill,
            },
            sequences,
        })
    }

    fn reply(&self, carrier: &Frame, body: Vec<u8>) {
        let Some(envelope) = carrier.envelope.to_reply() else {
            return;
        };
        // Through the outbox, like every other frame a node produces. A reply
        // that took the direct path would be the one thing this node can still
        // lose to a full lane.
        self.emit(Frame { envelope, body });
    }

    /// Hands a frame to this node's outbox.
    ///
    /// The outbox is drained by a task of its own, which waits for room on the
    /// agent queue. That waiting is backpressure and belongs somewhere — but
    /// not here: this is the same task that receives hop completions, and a
    /// node blocked mid-emit could not observe the hop it is waiting on. The
    /// outbox is the seam that keeps a full lane from becoming a stall.
    fn emit(&self, frame: Frame) {
        self.counts.emitted.fetch_add(1, Ordering::Relaxed);
        let _ = self.outbox.send(frame);
    }

    fn reply_error(&self, carrier: &Frame, detail: &str) {
        self.reply(carrier, self.payload.failure(detail));
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
