//! The sending half of the `SessionClose`/`SessionClosed` contract: keeping
//! a close pending until it is answered, and giving up on a bound rather
//! than retrying forever.
//!
//! `outcome::close` decides *what* to send and *to whom*, as a pure function
//! of a hop's outcome; this decides *whether it landed*, which needs state
//! that outlives one call -- a retry schedule, an attempt count, and the
//! frame to resend. Kept apart for the same reason `outcome::close` is kept
//! apart from `outcome::next`: a reader auditing what a finished hop implies
//! still has one function to read, and a reader auditing whether that
//! implication actually reached its target has this one instead.
//!
//! Bounded by attempts and, because each attempt is spaced by a fixed
//! interval, by wall-clock too -- the two are not independent knobs here,
//! deliberately: a bound stated only in attempts with no fixed spacing could
//! still retry forever if the interval were ever made to grow without limit,
//! and a bound stated only in wall-clock time would keep sending arbitrarily
//! often within it. Fixing the interval and counting attempts against it
//! gives a single, small, testable worst case (five sends spread over
//! roughly a second) instead of two bounds that have to be reasoned about
//! together.
//!
//! Giving up removes the pending entry and counts it
//! (`Counts::session_close_abandoned`) rather than holding it forever or
//! dropping it silently. This node holds no reservation on the *other*
//! node's behalf -- `active_sequences` here is this node's own admission
//! ledger, untouched by a peer's close -- so abandoning does not itself leak
//! anything on this side. What it does is stop pretending delivery is still
//! being pursued, and say so where an operator already looks
//! (`node_counts`), which is the visible-degraded state a silent infinite
//! retry or a silent drop both fail to be.

use super::{Node, now_unix_ms};
use crate::node::outcome::{Next, close};
use p4_protocol::frame::Frame;
use std::sync::atomic::Ordering;

/// Retry spacing for a pending close. Fixed rather than backed off: the
/// traffic is a handful of bytes on a control lane nobody else is
/// contending for, so there is nothing here for a growing interval to
/// protect.
const SESSION_CLOSE_RETRY_INTERVAL_MS: u64 = 250;

/// How many times a close is sent, including the first attempt, before this
/// node gives up on hearing `SessionClosed` for it. Five attempts at the
/// fixed interval above is a worst case of roughly one second from first
/// send to abandonment -- long enough to absorb one lost frame and its
/// resend with room to spare, short enough that a test proving convergence
/// after a loss does not have to wait for it, and short enough that an
/// operator sees an abandonment count rise within the same run rather than
/// hours later.
const SESSION_CLOSE_MAX_ATTEMPTS: u32 = 5;

/// One `SessionClose` this node sent and is still waiting to hear
/// `SessionClosed` for.
pub(super) struct PendingClose {
    /// Echoed back by the acknowledgement so a `close_id` that collides with
    /// a stale or contaminated answer is refused rather than silently
    /// accepted -- see `Node::observe_session_closed`.
    pub(super) sequence: String,
    /// Resent byte-for-byte; the wire body is where `close_id` actually
    /// lives, so replaying the same frame is what makes a resend
    /// recognizable as the *same* close rather than a new one.
    pub(super) frame: Frame,
    pub(super) attempts: u32,
    pub(super) next_retry_unix_ms: u64,
}

impl Node {
    /// Sends every close one outcome's dead end owes the rest of its chain,
    /// and keeps each pending until it is acknowledged.
    ///
    /// `close_id` generation happens here, not in `outcome::close`, because
    /// only this node owns `next_close_id` -- the pure decision function
    /// takes a generator rather than the counter itself so it stays testable
    /// without a `Node` to construct.
    pub(super) async fn begin_session_closes(&self, carrier: &Frame, next: &Next, sequence: &str) {
        let closes =
            close::session_close_frames(carrier, next, sequence, self.payload.as_ref(), || {
                self.next_close_id.fetch_add(1, Ordering::Relaxed)
            });
        if closes.is_empty() {
            return;
        }
        let now = now_unix_ms();
        {
            let mut pending = self.pending_closes.lock().expect("pending close lock");
            for (close_id, frame) in &closes {
                pending.insert(
                    *close_id,
                    PendingClose {
                        sequence: sequence.to_owned(),
                        frame: frame.clone(),
                        attempts: 1,
                        next_retry_unix_ms: now + SESSION_CLOSE_RETRY_INTERVAL_MS,
                    },
                );
            }
        }
        for (_, frame) in closes {
            self.emit(frame).await;
        }
    }

    /// Resends every pending close whose retry time has passed, and
    /// abandons -- counted, not silent -- every one that has already used
    /// its last attempt. Called on every deadline-timer wakeup regardless of
    /// why the timer fired; a tick with nothing due does nothing.
    pub(super) async fn retry_pending_closes(&self) {
        let now = now_unix_ms();
        let mut due = Vec::new();
        let mut abandoned = 0usize;
        {
            let mut pending = self.pending_closes.lock().expect("pending close lock");
            pending.retain(|_, entry| {
                if entry.next_retry_unix_ms > now {
                    return true;
                }
                if entry.attempts >= SESSION_CLOSE_MAX_ATTEMPTS {
                    abandoned += 1;
                    return false;
                }
                entry.attempts += 1;
                entry.next_retry_unix_ms = now + SESSION_CLOSE_RETRY_INTERVAL_MS;
                due.push(entry.frame.clone());
                true
            });
        }
        if abandoned > 0 {
            self.counts
                .session_close_abandoned
                .fetch_add(abandoned, Ordering::Relaxed);
            if std::env::var_os("P4_AGENT_TRACE_CLOSE").is_some() {
                eprintln!("P4_AGENT_SESSION_CLOSE_ABANDONED count={abandoned}");
            }
        }
        for frame in due {
            self.emit(frame).await;
        }
    }

    /// Retires the pending entry a `SessionClosed` answers, if it still
    /// exists and actually matches.
    ///
    /// `close_id` alone identifies the entry -- see `PendingClose`'s own
    /// doc -- but `sequence` is still checked before removing anything: a
    /// `close_id` collision is not expected from this node's own
    /// `AtomicU64` counter, but an answer is untrusted input the moment it
    /// crosses the wire, and confirming the pairing costs nothing weighed
    /// against silently retiring the wrong entry. No match -- an unknown or
    /// already-cleared `close_id`, or one whose sequence disagrees -- is a
    /// stale or contaminated answer and is dropped, not treated as new
    /// information: this node has nothing else to do about a message it did
    /// not ask for.
    pub(super) async fn observe_session_closed(&self, sequence: &str, close_id: u64) {
        let matched = {
            let mut pending = self.pending_closes.lock().expect("pending close lock");
            let matches = pending
                .get(&close_id)
                .is_some_and(|entry| entry.sequence == sequence);
            if matches {
                pending.remove(&close_id);
            }
            matches
        };
        if matched {
            self.counts
                .session_closed_acked
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.counts
                .session_closed_stale
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}
