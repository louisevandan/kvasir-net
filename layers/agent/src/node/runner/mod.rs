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
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Sends adapter events to the node that owns them.
///
/// Unbounded on purpose. An adapter only raises events for work it was handed,
/// so depth is already bounded by the window; dropping one would lose a hop
/// completion and leave the node idle forever with work still queued.
#[derive(Clone)]
struct Sink(mpsc::UnboundedSender<Event>);

impl EventSink for Sink {
    fn raise(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

pub struct Node {
    queue: Arc<NodeQueue>,
    adapter: Arc<dyn Adapter>,
    payload: Arc<dyn Payload>,
    out: Sender,
    /// What the load declared. A ceiling: never derived here, never exceeded.
    ceiling: Mutex<usize>,
    /// Frames of the hop in flight, keyed by the sequence id they were given,
    /// so an outcome can be matched back to the route it came from.
    in_flight: Mutex<HashMap<String, Frame>>,
    events: Sink,
}

/// What a caller keeps to feed a running node.
#[derive(Clone)]
pub struct Handle {
    work: mpsc::UnboundedSender<Frame>,
    queue: Arc<NodeQueue>,
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
        let queue = Arc::new(NodeQueue::default());
        let node = Node {
            queue: Arc::clone(&queue),
            adapter,
            payload,
            out,
            ceiling: Mutex::new(ceiling.max(1)),
            in_flight: Mutex::new(HashMap::new()),
            events: Sink(event_tx),
        };
        tokio::spawn(node.run(work_rx, event_rx));
        Handle {
            work: work_tx,
            queue,
        }
    }

    async fn run(
        self,
        mut work: mpsc::UnboundedReceiver<Frame>,
        mut events: mpsc::UnboundedReceiver<Event>,
    ) {
        loop {
            tokio::select! {
                biased;
                // A hop ending is handled first. It is what frees the node,
                // and letting new arrivals go ahead of it would keep a node
                // that has finished looking busy.
                Some(event) = events.recv() => self.on_event(event),
                Some(frame) = work.recv() => {
                    self.queue.push(frame);
                    self.drain();
                }
                else => return,
            }
        }
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
        let ceiling = *self.ceiling.lock().expect("ceiling lock");
        let Some(window) = compose(&self.queue.waiting(), ceiling, now) else {
            return;
        };
        let routes: Vec<String> = window.items.iter().map(|item| item.route.clone()).collect();
        let claimed = self.queue.claim(&routes);
        if claimed.is_empty() {
            return;
        }
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
                let carriers = std::mem::take(&mut *self.in_flight.lock().expect("in-flight lock"));
                // Released before the outcomes are routed, so a lap this hop
                // produces can be picked up by the drain below rather than
                // waiting for the next arrival.
                self.queue.finished();
                for outcome in outcomes {
                    let Some(carrier) = carriers.get(&outcome.sequence) else {
                        continue;
                    };
                    for frame in next(carrier, &outcome).frames() {
                        self.emit(frame);
                    }
                }
                self.drain();
            }
            Event::Failed {
                sequence, detail, ..
            } => {
                let mut in_flight = self.in_flight.lock().expect("in-flight lock");
                let failed: Vec<Frame> = match sequence {
                    Some(id) => in_flight.remove(&id).into_iter().collect(),
                    None => std::mem::take(&mut *in_flight).into_values().collect(),
                };
                drop(in_flight);
                self.queue.finished();
                for frame in failed {
                    self.reply_error(&frame, &detail);
                }
                self.drain();
            }
            // Load and unload reporting belongs to whoever asked for the load,
            // and reaches them through the same reply path as anything else.
            _ => {}
        }
    }

    fn emit(&self, frame: Frame) {
        if let Err(refused) = self.out.offer(frame) {
            // The agent's queue is full. Nothing here can wait — waiting is
            // what turns a bounded queue unbounded — so the route is answered
            // with a refusal rather than silently held.
            self.reply_error(&refused.0, "agent queue refused this frame");
        }
    }

    fn reply_error(&self, carrier: &Frame, detail: &str) {
        let Some(envelope) = carrier.envelope.to_reply() else {
            return;
        };
        let _ = self.out.offer(Frame {
            envelope,
            body: detail.as_bytes().to_vec(),
        });
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
