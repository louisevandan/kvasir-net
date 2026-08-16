//! What the adapter says back, and the channel it says it on.
//!
//! The other direction from the loop next door. `mod.rs` decides what to hand
//! a backend; this decides what an answer means — a token routed on, a hop
//! ended, a load bound or refused. The two change apart: scheduling moves when
//! batching does, and this moves when the adapter's event vocabulary does.

use super::{Bound, Node};
use crate::node::outcome::next;
use p4_adapter::{Event, EventSink};
use p4_protocol::frame::Frame;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// Sends adapter events to the node that owns them.
///
/// Unbounded on purpose. An adapter only raises events for work it was handed,
/// so depth is already bounded by the window; dropping one would lose a hop
/// completion and leave the node idle forever with work still queued.
#[derive(Clone)]
pub(super) struct Sink {
    events: mpsc::UnboundedSender<Event>,
    raised: Arc<AtomicUsize>,
    lost: Arc<AtomicUsize>,
}

impl Sink {
    pub(super) fn new(
        events: mpsc::UnboundedSender<Event>,
        raised: Arc<AtomicUsize>,
        lost: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            events,
            raised,
            lost,
        }
    }
}

impl EventSink for Sink {
    fn raise(&self, event: Event) {
        self.raised.fetch_add(1, Ordering::Relaxed);
        if self.events.send(event).is_err() {
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Node {
    pub(super) fn on_event(&self, event: Event) {
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
}
