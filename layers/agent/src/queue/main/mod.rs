//! The agent's one main queue.
//!
//! Four lanes, each bounded on its own, drained by a dispatcher that prefers
//! control over everything and decode over prefill. A worker's job is short by
//! construction — forward, or move to a node queue — so the queue empties at
//! the speed of a decision rather than the speed of the work.
//!
//! Depth and in-flight width are separate: depth is how much this agent will
//! remember, in-flight is how much it will be doing at once. Conflating them
//! is the recorded defect this file exists to avoid repeating.

use super::lane::{Budget, Lanes};
use p4_protocol::QueueClass;
use p4_protocol::frame::Frame;
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};

/// Refused because the lane it belongs to is full. The caller answers with an
/// error rather than waiting, because waiting here is what turns a bounded
/// queue back into an unbounded one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub Frame);

pub struct Sender {
    control: mpsc::Sender<Frame>,
    prefill: mpsc::Sender<Frame>,
    decode: mpsc::Sender<Frame>,
    response: mpsc::Sender<Frame>,
}

impl Clone for Sender {
    fn clone(&self) -> Self {
        Self {
            control: self.control.clone(),
            prefill: self.prefill.clone(),
            decode: self.decode.clone(),
            response: self.response.clone(),
        }
    }
}

/// How often the lane preference gives way to a fair pass.
const FAIR_EVERY: u64 = 16;

pub struct Receiver {
    /// Counts takes so the preference can be dropped periodically.
    taken: u64,
    control: mpsc::Receiver<Frame>,
    prefill: mpsc::Receiver<Frame>,
    decode: mpsc::Receiver<Frame>,
    response: mpsc::Receiver<Frame>,
}

/// Bounds how many messages are being handled at once, independently of how
/// many are remembered.
pub type InFlight = Arc<Semaphore>;

pub fn channel(lanes: Lanes, budget: Budget) -> (Sender, Receiver, InFlight) {
    let (control_tx, control) = mpsc::channel(lanes.control);
    let (prefill_tx, prefill) = mpsc::channel(lanes.prefill);
    let (decode_tx, decode) = mpsc::channel(lanes.decode);
    let (response_tx, response) = mpsc::channel(lanes.response);
    (
        Sender {
            control: control_tx,
            prefill: prefill_tx,
            decode: decode_tx,
            response: response_tx,
        },
        Receiver {
            taken: 0,
            control,
            prefill,
            decode,
            response,
        },
        Arc::new(Semaphore::new(budget.in_flight)),
    )
}

impl Sender {
    /// Enqueues without waiting. A socket receiver calls this and nothing
    /// else, so a full lane must come back as a refusal rather than block the
    /// reader and stall every other route on that connection.
    pub fn offer(&self, frame: Frame) -> Result<(), Refused> {
        self.lane(frame.envelope.lane)
            .try_send(frame)
            .map_err(|error| {
                Refused(match error {
                    mpsc::error::TrySendError::Full(frame) => frame,
                    mpsc::error::TrySendError::Closed(frame) => frame,
                })
            })
    }

    /// Enqueues, waiting for room if there is none.
    ///
    /// For callers that should be slowed rather than refused. A node producing
    /// tokens is one: it outruns the agent only by being fast, and holding it
    /// at its next hop is backpressure, where dropping its output is loss. A
    /// worker must never call this — waiting in a worker is what turns a
    /// bounded queue back into an unbounded one.
    pub async fn send(&self, frame: Frame) -> Result<(), Refused> {
        let lane = self.lane(frame.envelope.lane).clone();
        lane.send(frame).await.map_err(|error| Refused(error.0))
    }

    fn lane(&self, class: QueueClass) -> &mpsc::Sender<Frame> {
        match class {
            QueueClass::Control => &self.control,
            QueueClass::Prefill => &self.prefill,
            QueueClass::Decode => &self.decode,
            QueueClass::Response => &self.response,
        }
    }
}

impl Receiver {
    /// Takes the next message, preferring control so inference cannot starve
    /// the ability to create or delete a node, and decode over prefill for the
    /// same reason the window does — a lap belongs to a request already
    /// holding KV.
    ///
    /// The preference is bounded. Strict priority is not a preference but a
    /// veto: a busy lane that is never empty means the ones under it are never
    /// polled at all, and a lap that is never dispatched is a request that
    /// never finishes. So every `FAIR_EVERY` frames the order is dropped and
    /// all four lanes compete, which costs nothing when the upper lanes are
    /// quiet and guarantees progress when they are not.
    ///
    /// Returns `None` only when every lane is closed.
    pub async fn take(&mut self) -> Option<Frame> {
        self.taken = self.taken.wrapping_add(1);
        if self.taken % FAIR_EVERY == 0 {
            return self.fair().await;
        }
        tokio::select! {
            biased;
            Some(frame) = self.control.recv() => Some(frame),
            Some(frame) = self.response.recv() => Some(frame),
            Some(frame) = self.decode.recv() => Some(frame),
            Some(frame) = self.prefill.recv() => Some(frame),
            else => None,
        }
    }

    async fn fair(&mut self) -> Option<Frame> {
        tokio::select! {
            Some(frame) = self.control.recv() => Some(frame),
            Some(frame) = self.response.recv() => Some(frame),
            Some(frame) = self.decode.recv() => Some(frame),
            Some(frame) = self.prefill.recv() => Some(frame),
            else => None,
        }
    }

    /// What each lane is holding. The agent-side half of the pair that tells a
    /// slowdown in P4 from one below it: shallow here beside a deep node queue
    /// puts the cause under the adapter.
    pub fn depth(&self) -> Depth {
        Depth {
            control: self.control.len(),
            prefill: self.prefill.len(),
            decode: self.decode.len(),
            response: self.response.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Depth {
    pub control: usize,
    pub prefill: usize,
    pub decode: usize,
    pub response: usize,
}

impl Depth {
    pub fn total(&self) -> usize {
        self.control + self.prefill + self.decode + self.response
    }
}

#[cfg(test)]
mod tests;

impl Sender {
    /// Lane depth, read from the sending side.
    ///
    /// The dispatcher owns the receiver, so anything watching the queue needs
    /// its own way in. This is the agent-side half of the pair that says which
    /// side of the adapter boundary is slow.
    pub fn depth(&self) -> Depth {
        let used = |lane: &mpsc::Sender<Frame>| lane.max_capacity() - lane.capacity();
        Depth {
            control: used(&self.control),
            prefill: used(&self.prefill),
            decode: used(&self.decode),
            response: used(&self.response),
        }
    }
}
