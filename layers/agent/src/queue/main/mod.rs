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
use p4_protocol::frame::Frame;
use p4_protocol::QueueClass;
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

pub struct Receiver {
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
        let lane = match frame.envelope.lane {
            QueueClass::Control => &self.control,
            QueueClass::Prefill => &self.prefill,
            QueueClass::Decode => &self.decode,
            QueueClass::Response => &self.response,
        };
        lane.try_send(frame).map_err(|error| Refused(match error {
            mpsc::error::TrySendError::Full(frame) => frame,
            mpsc::error::TrySendError::Closed(frame) => frame,
        }))
    }
}

impl Receiver {
    /// Takes the next message, preferring control so inference cannot starve
    /// the ability to create or delete a node, and decode over prefill for the
    /// same reason the window does — a lap belongs to a request already
    /// holding KV.
    ///
    /// Returns `None` only when every lane is closed.
    pub async fn take(&mut self) -> Option<Frame> {
        loop {
            tokio::select! {
                biased;
                Some(frame) = self.control.recv() => return Some(frame),
                Some(frame) = self.response.recv() => return Some(frame),
                Some(frame) = self.decode.recv() => return Some(frame),
                Some(frame) = self.prefill.recv() => return Some(frame),
                else => return None,
            }
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
