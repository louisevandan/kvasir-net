//! How much of each thing the agent will hold at once.
//!
//! Three axes, declared separately, because they were once one constant and
//! narrowing the release width silently narrowed connection admission and
//! queue depth with it. Nothing here derives one from another.

use p4_protocol::QueueClass;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Sockets this agent will hold open at once.
    pub connections: usize,
    /// Messages in flight through workers at once.
    pub in_flight: usize,
    /// Messages a lane will hold before it refuses.
    pub depth: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            connections: 1024,
            in_flight: 256,
            depth: 4096,
        }
    }
}

impl Budget {
    /// Refuses a nonsensical budget instead of clamping it, so a typo in a
    /// fleet run fails at start rather than changing what is measured.
    pub fn checked(self) -> Result<Self, String> {
        for (name, value) in [
            ("connections", self.connections),
            ("in_flight", self.in_flight),
            ("depth", self.depth),
        ] {
            if value == 0 {
                return Err(format!("{name} budget must not be zero"));
            }
        }
        Ok(self)
    }
}

/// Per-lane depth. Control must not be starved by inference, and a decode lap
/// that cannot enqueue stalls a request that is already holding KV, so the two
/// inference lanes are sized apart from each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lanes {
    pub control: usize,
    pub prefill: usize,
    pub decode: usize,
    pub response: usize,
}

impl Default for Lanes {
    fn default() -> Self {
        Self {
            control: 1024,
            prefill: 4096,
            // Decode is deliberately the deepest. A queued decode lap belongs
            // to a request that already occupies KV on every node of its
            // chain; refusing it wastes more than refusing an arrival does.
            decode: 8192,
            response: 4096,
        }
    }
}

impl Lanes {
    pub fn depth(&self, lane: QueueClass) -> usize {
        match lane {
            QueueClass::Control => self.control,
            QueueClass::Prefill => self.prefill,
            QueueClass::Decode => self.decode,
            QueueClass::Response => self.response,
        }
    }
}

#[cfg(test)]
mod tests;
