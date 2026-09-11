//! A bounded opportunity to combine ready decode requests. This does not grant
//! execution, KV, flight or storage credit, or bound a non-preemptive RPC.
use std::time::{Duration, Instant};

// Internal scheduling delay ceiling, not a measured optimum or token-latency
// promise. OS scheduling and native/control work can delay the next decision.
const DECODE_COALESCE_WAIT: Duration =
    Duration::from_millis(crate::v2::scheduler::pipeline::DECODE_COALESCE_WAIT_MS);

#[derive(Default)]
pub(super) struct DecodeCoalescer {
    waiting: Option<Waiting>,
    wake_at: Option<Instant>,
}

struct Waiting {
    load: u64,
    session: String,
    deadline: Instant,
}

impl DecodeCoalescer {
    /// Every drive attempt must re-authorize the timer. Hard blockers (full
    /// flight window, fenced native/effects, no runnable input) must not spin
    /// just because an older decode deadline has expired.
    pub fn disarm(&mut self) {
        self.wake_at = None;
    }

    pub fn clear(&mut self) {
        self.waiting = None;
        self.disarm();
    }

    pub fn wake_at(&self) -> Option<Instant> {
        self.wake_at
    }

    pub fn should_wait(&mut self, load: u64, session: &str, now: Instant) -> bool {
        if self
            .waiting
            .as_ref()
            .is_none_or(|w| w.load != load || w.session != session)
        {
            self.waiting = Some(Waiting {
                load,
                session: session.to_owned(),
                deadline: now + DECODE_COALESCE_WAIT,
            });
        }
        let deadline = self.waiting.as_ref().expect("initialized above").deadline;
        self.wake_at = (now < deadline).then_some(deadline);
        self.wake_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrivals_do_not_renew_decode_wait_and_hard_blockers_disarm_the_timer() {
        let now = Instant::now();
        let mut gate = DecodeCoalescer::default();
        assert!(gate.should_wait(1, "session", now));
        let deadline = gate.wake_at().unwrap();
        gate.disarm();
        assert!(gate.wake_at().is_none());
        assert!(gate.should_wait(1, "session", now + Duration::from_millis(1)));
        assert_eq!(
            gate.wake_at(),
            Some(deadline),
            "ingress must not restart the wait"
        );
        assert!(!gate.should_wait(1, "session", deadline));
        assert!(gate.wake_at().is_none());
        assert!(
            gate.should_wait(2, "session", deadline),
            "a new load must not inherit expiry"
        );
        gate.clear();
        assert!(gate.wake_at().is_none());
        assert!(gate.should_wait(2, "other-session", deadline));
    }
}
