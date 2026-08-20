//! The declared behaviour of a backend that does not exist.
//!
//! Shaped by what the real workload was measured doing, not by a generic
//! delay: a load spread over stages, a prefill whose cost belongs to a chain
//! position rather than to a device, a decode lap, and a window.
//!
//! Nothing is computed and nothing is random. The same profile produces the
//! same timings on every machine, so a difference between two fleet runs is a
//! difference in P4.

use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// How long the whole load takes, reported across `stages`.
    pub load: Duration,
    /// How many pieces the model is spread over. A staged backend reports one
    /// progress stream per piece; an internal one reports a single stage
    /// because its pieces are not separately addressable.
    pub stages: u32,
    /// What the first node of a chain costs per hop.
    ///
    /// Measurement put this well above the trailing cost and found it belonged
    /// to the position rather than to the card sitting in it, so the mock
    /// charges by position too.
    pub leading_hop: Duration,
    /// What every later node costs per hop.
    pub trailing_hop: Duration,
    /// Extra cost of a prefill hop over a decode hop at the same position.
    /// Prefill is the long phase; a lap is comparatively cheap.
    pub prefill_extra: Duration,
    /// Bytes this deployment claims per stage, so a report has a shape worth
    /// reading and a stage that reserves far more than its share is visible.
    pub reserved_per_stage: u64,
    /// Every Nth lap of the terminal stage advances state and reports no text.
    /// Zero never does it.
    ///
    /// Not a fault. A backend holding back the first bytes of a multi-byte
    /// character has decoded perfectly well and simply has nothing printable
    /// to hand over yet, and the first decode of a request primes state
    /// without producing anything at all. Both are ordinary, and both are laps
    /// that are not response events — which is the distinction P4's reply
    /// numbering has to keep, so a mock has to be able to state it.
    pub mute_every: u32,
    pub fault: Fault,
}

/// Failures asked for on purpose, each one a terminal state that is otherwise
/// hard to reach without breaking something real.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    None,
    /// The load never succeeds.
    Load,
    /// Every hop fails.
    Hop,
    /// The adapter never answers, so the deadline has to.
    Silence,
    /// Ignores cooperative cancellation briefly and emits a terminal event
    /// later. This makes the node's timed-out-but-still-occupied state
    /// testable without a real backend.
    Stubborn,
    /// Preparation succeeds but commit fails. This is the failure needed to
    /// exercise compensation after an earlier stage has already committed.
    CacheCommit,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            load: Duration::ZERO,
            stages: 1,
            leading_hop: Duration::ZERO,
            trailing_hop: Duration::ZERO,
            prefill_extra: Duration::ZERO,
            reserved_per_stage: 0,
            mute_every: 0,
            fault: Fault::None,
        }
    }
}

impl Profile {
    /// A profile in the proportions measurement found, scaled so a test can
    /// run in milliseconds what a GPU took a minute to do. The ratios are what
    /// matter: leading well above trailing, prefill well above a lap.
    pub fn measured_shape(scale: Duration) -> Self {
        Self {
            load: scale * 4,
            stages: 2,
            leading_hop: scale * 2,
            trailing_hop: scale,
            prefill_extra: scale * 3,
            reserved_per_stage: 188 * 1024 * 1024,
            mute_every: 0,
            fault: Fault::None,
        }
    }

    /// What one hop costs at this position in a chain.
    pub fn hop_cost(&self, position: usize, prefill: bool) -> Duration {
        let base = if position == 0 {
            self.leading_hop
        } else {
            self.trailing_hop
        };
        if prefill {
            base + self.prefill_extra
        } else {
            base
        }
    }

    /// How long one stage of the load takes.
    pub fn stage_cost(&self) -> Duration {
        self.load / self.stages.max(1)
    }
}

#[cfg(test)]
mod tests;
