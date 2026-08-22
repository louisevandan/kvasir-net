//! One scheduler thread for every `Full` this agent is waiting out.
//!
//! Three things forced this out of the inline `tokio::spawn` it started as.
//!
//! It ran on whichever thread the client raised the rejection on, and a
//! real client raises from its own pump thread -- an OS thread with no
//! runtime attached, where `tokio::spawn` panics rather than failing. So
//! the retry never happened at all in any process whose client was real; a
//! scripted client hid it by answering from inside a test's runtime.
//!
//! It threw away the result of the retry's own `try_submit`. A local queue
//! that was still full meant the submission never went out, no further
//! `Full` ever arrived to schedule another attempt, the deadline was never
//! re-checked, and the route stayed for the life of the process.
//!
//! And a task per attempt is a task per attempt: under real saturation,
//! with every submission bouncing, that is unbounded concurrency created by
//! the very condition that says there is no room.
//!
//! One thread, one list of due times, each attempt re-checking the route
//! and the deadline as it fires. The list holds at most one entry per
//! in-flight submission, so it is bounded by the same thing the routing
//! table is.

use super::super::Agent;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

/// How long a `Full` waits before the same submission is offered again.
///
/// Not a backoff curve: P4 does not compute the backend's capacity, so it
/// has nothing to shape one from. The bound on retrying is the caller's own
/// deadline, checked on every attempt.
pub(crate) const FULL_RETRY_DELAY: Duration = Duration::from_millis(20);

/// How long the thread sleeps when it has nothing due, before looking again
/// at whether the agent it serves is still alive.
const IDLE_POLL: Duration = Duration::from_secs(1);

pub(crate) struct Retries {
    pending: Mutex<Vec<Attempt>>,
    due: Condvar,
}

struct Attempt {
    submission_id: String,
    at: Instant,
}

impl Retries {
    /// Starts the scheduler thread and returns the handle the agent keeps.
    ///
    /// `agent` is `Weak` for the same reason `AgentDeploymentSink`'s is: the
    /// agent owns this, so an owning reference back would be a cycle neither
    /// side breaks. It is also what lets the thread notice the agent is gone
    /// and stop.
    pub(crate) fn start(agent: Weak<Agent>) -> Arc<Self> {
        let retries = Arc::new(Self {
            pending: Mutex::new(Vec::new()),
            due: Condvar::new(),
        });
        let worker = Arc::clone(&retries);
        std::thread::Builder::new()
            .name("p4-relay-retry".into())
            .spawn(move || worker.run(agent))
            .expect("spawn the relay's retry thread");
        retries
    }

    /// Asks for `submission_id` to be offered again after `delay`.
    ///
    /// Rescheduling one that is already waiting moves it no earlier: a
    /// second `Full` for a submission already queued here is the same
    /// backpressure said twice, and letting it stack would turn a burst of
    /// rejections into a burst of attempts.
    pub(crate) fn schedule(&self, submission_id: String, delay: Duration) {
        let at = Instant::now() + delay;
        let mut pending = self.pending.lock().expect("retry queue lock");
        if pending
            .iter()
            .any(|attempt| attempt.submission_id == submission_id)
        {
            return;
        }
        pending.push(Attempt { submission_id, at });
        drop(pending);
        self.due.notify_one();
    }

    fn run(self: Arc<Self>, agent: Weak<Agent>) {
        loop {
            let Some(submission_id) = self.take_due() else {
                if agent.upgrade().is_none() {
                    return;
                }
                continue;
            };
            let Some(agent) = agent.upgrade() else {
                return;
            };
            // A failed attempt comes straight back into the queue. This is
            // the one path that keeps a submission alive when the local
            // client itself has no room: nothing else will produce another
            // `Full` for it, because nothing else ever put it on the wire.
            if agent.attempt_retry(&submission_id) {
                self.schedule(submission_id, FULL_RETRY_DELAY);
            }
        }
    }

    /// The next submission whose time has come, waiting for it if need be.
    /// Returns `None` when the wait timed out, which is the thread's chance
    /// to check whether the agent still exists.
    fn take_due(&self) -> Option<String> {
        let mut pending = self.pending.lock().expect("retry queue lock");
        loop {
            let now = Instant::now();
            let earliest = pending
                .iter()
                .enumerate()
                .min_by_key(|(_, attempt)| attempt.at)
                .map(|(index, attempt)| (index, attempt.at));
            match earliest {
                Some((index, at)) if at <= now => return Some(pending.remove(index).submission_id),
                Some((_, at)) => {
                    let (guard, _) = self
                        .due
                        .wait_timeout(pending, (at - now).min(IDLE_POLL))
                        .expect("retry wait");
                    pending = guard;
                }
                None => {
                    let (guard, timeout) = self
                        .due
                        .wait_timeout(pending, IDLE_POLL)
                        .expect("retry wait");
                    pending = guard;
                    if timeout.timed_out() {
                        return None;
                    }
                }
            }
        }
    }
}
