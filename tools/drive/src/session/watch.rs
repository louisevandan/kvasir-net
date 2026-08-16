//! Watching the queues while the run is in flight.
//!
//! The interesting state only exists during a run and is gone by the time the
//! verdicts print, so a claim about queueing has to be observed while it is
//! happening. It is observed the way OUTER has to observe anything — by asking
//! over the same socket as everything else — rather than by reading the
//! agent's stdout, which is a thing you can only do sitting at the machine.
//!
//! What is being watched for: a node that dispatches no more than its declared
//! ceiling however deep its own queue gets, and a main queue that does not
//! become the place the backlog lives. Either number alone proves nothing —
//! a shallow main queue is also what you see when nothing has arrived, and a
//! deep node queue is also what you see when the agent has backed up with it.

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;

/// The highest each figure reached, which is the only part of a sample that
/// still means something once the run is over.
#[derive(Default, Clone)]
pub struct Peaks {
    /// Deepest a single node's own queue got — the backlog P4 was holding.
    pub node_depth: Arc<AtomicUsize>,
    /// Most a single node ever had inside the adapter at once. This is the
    /// declared ceiling being enforced, and it is enforced here rather than by
    /// the backend refusing work.
    pub running: Arc<AtomicUsize>,
    /// Deepest any of the agent's four lanes got. The main queue hands to the
    /// node and does not hold; if the backlog were living here instead, this
    /// is where it would show.
    pub lane: Arc<AtomicUsize>,
    /// How many samples were taken, so an empty watch cannot pass for a quiet
    /// one. Peaks of zero from zero samples say nothing at all.
    pub samples: Arc<AtomicUsize>,
}

impl Peaks {
    /// Folds one snapshot in. Unknown keys are ignored: this reads the fields
    /// it needs and does not require the format to stay otherwise fixed.
    pub fn observe(&self, snapshot: &str) {
        self.samples.fetch_add(1, SeqCst);
        for line in snapshot.lines() {
            if let Some(depth) = field(line, "depth=") {
                raise(&self.node_depth, depth);
            }
            if let Some(running) = field(line, "running=") {
                raise(&self.running, running);
            }
            for lane in ["control=", "prefill=", "decode=", "response="] {
                if let Some(value) = field(line, lane) {
                    raise(&self.lane, value);
                }
            }
        }
    }

    pub fn samples(&self) -> usize {
        self.samples.load(SeqCst)
    }
}

fn raise(slot: &AtomicUsize, value: usize) {
    slot.fetch_max(value, SeqCst);
}

/// Reads `key=<number>` out of a snapshot line.
fn field(line: &str, key: &str) -> Option<usize> {
    let rest = line.split(key).nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests;
