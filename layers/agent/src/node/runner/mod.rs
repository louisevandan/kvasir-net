//! The node's event loop.
//!
//! It advances on two events and no others: work arriving, and a hop ending.
//! There is no timer and no polling, because a node's pace is the backend's
//! pace and inventing a third trigger would mean guessing at it.

use crate::node::outcome::next;
use crate::node::payload::Payload;
use crate::node::queue::NodeQueue;
use crate::node::window::{compose, expired_items};
use crate::queue::main::Sender;
use p4_adapter::{Adapter, Event, EventSink, Hop, Phase, Work};
use p4_protocol::QueueClass;
use p4_protocol::frame::Frame;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Sends adapter events to the node that owns them.
///
/// Unbounded on purpose. An adapter only raises events for work it was handed,
/// so depth is already bounded by the window; dropping one would lose a hop
/// completion and leave the node idle forever with work still queued.
#[derive(Clone)]
struct Sink {
    events: mpsc::UnboundedSender<Event>,
    raised: Arc<AtomicUsize>,
    lost: Arc<AtomicUsize>,
}

impl EventSink for Sink {
    fn raise(&self, event: Event) {
        self.raised.fetch_add(1, Ordering::Relaxed);
        if self.events.send(event).is_err() {
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}

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

/// What a caller keeps to feed a running node.
#[derive(Clone)]
pub struct Handle {
    work: mpsc::UnboundedSender<Frame>,
    queue: Arc<NodeQueue>,
    counts: Arc<Counts>,
}

/// Whether this node may serve, and for which deployment.
///
/// A load is a transaction across machines that no single machine can see the
/// whole of. Nothing here coordinates it — there is nowhere to put a
/// coordinator that would not become a controller — so each stage enforces its
/// own half: it serves the generation it bound, and a stage whose load failed
/// serves nothing. A chain composed over a failed stage is refused by that
/// stage rather than answered from half a model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    /// Never loaded. A backend that needs no load is legitimate, so this
    /// serves — the ones that must not serve are the two below.
    Never,
    At(u64),
    /// A load failed here. Nothing runs until one succeeds.
    Refused,
}

impl Bound {
    /// Whether work carrying `generation` may run.
    fn admits(self, generation: u64) -> bool {
        match self {
            Self::Never => true,
            Self::At(bound) => bound == generation,
            Self::Refused => false,
        }
    }

    fn why(self, generation: u64) -> String {
        match self {
            Self::Never => String::new(),
            Self::At(bound) => {
                format!("node is bound at generation {bound}, work carries {generation}")
            }
            Self::Refused => "node has no model: its load failed".into(),
        }
    }
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
    /// Moves work to this node. Returns immediately — this call is the whole
    /// of a worker's job for a node-bound message.
    pub fn offer(&self, frame: Frame) {
        let _ = self.work.send(frame);
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
    /// The ceiling bounds this and nothing else does — a backend is never
    /// asked to refuse, and never told how much is waiting. Reported so that
    /// claim is checkable from outside rather than only in the code.
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

impl Node {
    /// Starts the node and returns the handle used to feed it.
    pub fn spawn(
        adapter: Arc<dyn Adapter>,
        payload: Arc<dyn Payload>,
        out: Sender,
        ceiling: usize,
    ) -> Handle {
        let (work_tx, work_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (outbox_tx, mut outbox_rx) = mpsc::unbounded_channel::<Frame>();
        let queue = Arc::new(NodeQueue::default());
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
            events: Sink {
                events: event_tx,
                raised: Arc::clone(&counts.raised),
                lost: Arc::clone(&counts.lost),
            },
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
        Handle {
            work: work_tx,
            queue,
            counts,
        }
    }

    async fn run(
        self,
        mut work: mpsc::UnboundedReceiver<Frame>,
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
                        self.queue.push(frame);
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

    fn on_event(&self, event: Event) {
        match event {
            Event::HopComplete { outcomes, .. } => {
                self.counts.completions.fetch_add(1, Ordering::Relaxed);
                self.counts
                    .outcomes
                    .fetch_add(outcomes.len(), Ordering::Relaxed);
                let carriers = std::mem::take(&mut *self.in_flight.lock().expect("in-flight lock"));
                // Released before the outcomes are routed, so a lap this hop
                // produces can be picked up by the drain below rather than
                // waiting for the next arrival.
                self.queue.finished();
                for outcome in outcomes {
                    let Some(carrier) = carriers.get(&outcome.sequence) else {
                        self.counts.orphaned.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    self.counts.routed.fetch_add(1, Ordering::Relaxed);
                    for frame in next(carrier, &outcome, self.payload.as_ref()).frames() {
                        self.emit(frame);
                    }
                }
                self.drain();
            }
            Event::Failed {
                sequence, detail, ..
            } => {
                let mut in_flight = self.in_flight.lock().expect("in-flight lock");
                let mut failed: Vec<Frame> = match sequence {
                    Some(id) => in_flight.remove(&id).into_iter().collect(),
                    None => std::mem::take(&mut *in_flight).into_values().collect(),
                };
                drop(in_flight);
                // A load can fail too, and its caller is waiting on the same
                // route. Leaving it here would hold the node running forever
                // over an instruction that already ended.
                let lifecycle = self.lifecycle.lock().expect("lifecycle lock").take();
                if lifecycle.is_some() {
                    // A stage of a distributed model that did not load is a
                    // stage that must not serve. Its neighbours may have bound
                    // perfectly well, and a chain that runs anyway produces
                    // answers from half a model — which looks like a working
                    // deployment and is the worst outcome available.
                    *self.bound.lock().expect("generation lock") = Bound::Refused;
                }
                failed.extend(lifecycle);
                self.queue.finished();
                for frame in failed {
                    self.reply_error(&frame, &detail);
                }
                self.drain();
            }
            // Load and unload reporting belongs to whoever asked, and reaches
            // them through the same reply path as anything else.
            // A cache instruction is lifecycle-shaped — one instruction, run
            // alone, answered to whoever asked — so it ends the same way, and
            // notably does not touch the binding: persisting a conversation
            // says nothing about which model is loaded.
            Event::Cached {
                sequence,
                bytes,
                detail,
                ..
            } => self.finish_lifecycle(self.payload.cached(&sequence, bytes, &detail)),
            Event::Loaded { generation, .. } => {
                *self.bound.lock().expect("generation lock") = Bound::At(generation);
                self.finish_lifecycle(self.payload.bound(generation))
            }
            Event::Unloaded { .. } => self.finish_lifecycle(self.payload.released()),
            // Progress is reported as it happens rather than held until the
            // end, because a distributed load's slowest stage is the fact
            // worth seeing early.
            Event::LoadProgress { stage, percent, .. } => {
                let carrier = self.lifecycle.lock().expect("lifecycle lock").clone();
                if let Some(carrier) = carrier {
                    self.reply(&carrier, self.payload.progress(stage, percent));
                }
            }
        }
    }

    fn finish_lifecycle(&self, body: Vec<u8>) {
        let carrier = self.lifecycle.lock().expect("lifecycle lock").take();
        self.queue.finished();
        if let Some(carrier) = carrier {
            self.reply(&carrier, body);
        }
        self.drain();
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
