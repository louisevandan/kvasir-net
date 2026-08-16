//! What a bad link does to bytes, stated as numbers rather than behaviour.
//!
//! Everything here is deterministic. A seeded generator would make two runs of
//! the same scenario differ, and the whole point of the mock fleet is that a
//! difference between two runs is a difference in P4. Jitter therefore comes
//! from counting chunks, not from chance — the sequence is irregular, and it
//! is the same irregular sequence every time.

use std::time::Duration;

/// A link's declared badness.
///
/// The default is a perfect link, so a scenario names only what it is testing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Impairment {
    /// Added one-way latency, paid once per chunk in each direction.
    pub delay: Duration,
    /// How far the delay swings either side of `delay`.
    pub jitter: Duration,
    /// Bytes per second this link will carry, or `None` for unmetered.
    pub rate: Option<u64>,
    /// A pause of this long, every `stall_every` chunks. Zero disables it.
    pub stall: Duration,
    pub stall_every: u32,
}

impl Impairment {
    /// A link with latency and nothing else — the WAN case.
    pub fn latency(delay: Duration, jitter: Duration) -> Self {
        Self {
            delay,
            jitter,
            ..Self::default()
        }
    }

    /// A narrow link. Latency is low; there is simply not much of it.
    pub fn bandwidth(bytes_per_second: u64) -> Self {
        Self {
            rate: Some(bytes_per_second),
            ..Self::default()
        }
    }

    /// A link that seizes up periodically, which is what a saturated switch or
    /// a retransmit storm looks like from above.
    pub fn stalling(every: u32, stall: Duration) -> Self {
        Self {
            stall,
            stall_every: every,
            ..Self::default()
        }
    }

    /// How long chunk number `n` of `bytes` bytes should be held.
    ///
    /// Latency and serialisation are added because they are different things:
    /// a satellite link is slow to start and quick to drain, a narrow one the
    /// other way round, and a chain feels them differently.
    pub fn hold(&self, sequence: u64, bytes: usize) -> Duration {
        let mut held = self.delay + self.jitter_at(sequence);
        if let Some(rate) = self.rate.filter(|rate| *rate > 0) {
            held += Duration::from_nanos((bytes as u64).saturating_mul(1_000_000_000) / rate);
        }
        if self.stall_every > 0
            && self.stall > Duration::ZERO
            && sequence.is_multiple_of(u64::from(self.stall_every))
        {
            held += self.stall;
        }
        held
    }

    /// The swing for a given chunk, spread across the jitter window without a
    /// generator: consecutive chunks land at different points, and the same
    /// chunk always lands at the same one.
    fn jitter_at(&self, sequence: u64) -> Duration {
        if self.jitter.is_zero() {
            return Duration::ZERO;
        }
        // A cheap integer scramble, so neighbouring chunks are not neighbouring
        // delays — an ordered ramp would be a pattern the queue could ride.
        let scrambled = sequence
            .wrapping_mul(6_364_136_223_846_793_005)
            .rotate_left(17);
        let step = (scrambled % 1_000) as u32;
        self.jitter.mul_f64(f64::from(step) / 1_000.0)
    }

    /// Whether this link does anything at all, so a perfect one can skip the
    /// timer entirely rather than sleeping for zero.
    pub fn is_perfect(&self) -> bool {
        self.delay.is_zero()
            && self.jitter.is_zero()
            && self.rate.is_none()
            && (self.stall.is_zero() || self.stall_every == 0)
    }
}

#[cfg(test)]
mod tests;
