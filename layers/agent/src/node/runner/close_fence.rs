//! Fencing a reservation against a session that does not own it.
//!
//! `active_sequences` keys a reservation by sequence id alone, and a sequence
//! id is not a session identity: a caller may legitimately reuse one for an
//! unrelated later session (`tools/drive`'s `Admission::retry` does exactly
//! that after a terminal), and `close_id` -- minted once, when a close
//! occasion is first built, and then carried unchanged on every resend of it
//! (`pending_close.rs` replays the identical frame byte-for-byte, so a resend
//! never mints a new one) -- only ever identifies *that occasion*, never the
//! session it was originally about. Two defects follow from trusting either
//! alone:
//!
//! - A `SessionClose` for session A whose `SessionClosed` answer is lost
//!   gets retried. If A's close already landed and a session B has since
//!   reserved the same sequence id, the retried close still names a
//!   sequence B now holds -- and without a session identity to compare, a
//!   receiver has no way to tell B's reservation apart from A's.
//! - A native close that fails leaves a reservation deliberately in place
//!   (see `events::fail_session_close`) rather than freed, specifically so a
//!   *different* session cannot be admitted onto the same sequence id while
//!   the backend's own release is still unconfirmed.
//!
//! `Payload::session_epoch` is the identity that closes both gaps: minted
//! once, at a session's own first admission, and carried unchanged through
//! every hop and lap of it -- including into the `SessionClose` a chain's
//! tail eventually builds (`outcome::close::session_close_frames` reads it
//! from the same carrier `close_id` is generated for). `active_sequences`
//! stores it alongside each reservation, and the two functions below are the
//! only places it is ever compared.

use super::Node;
use crate::node::outcome::close;
use p4_protocol::frame::Frame;

impl Node {
    /// A native close that failed: the adapter's own release attempt did
    /// not succeed, so the backend's slot is unconfirmed rather than known
    /// free. Handled apart from an ordinary hop failure (see
    /// `events::on_event`'s `Event::Failed` arm, which dispatches here
    /// before its own generic path runs) for two reasons: `Work::Close`
    /// never populates `in_flight` -- it runs alone the way every other
    /// lifecycle instruction does, so the generic path's sequence-removal
    /// logic has nothing of this failure's to find there -- and, more to
    /// the point, the two failures mean opposite things for
    /// `active_sequences`. An ordinary hop failure really did give the slot
    /// back; this one specifically did not.
    ///
    /// `sequence` is left exactly as it was: neither freed, which would let
    /// a new admission race a slot the backend may still hold (the defect
    /// this closes), nor moved to some separate quarantine set, because the
    /// reservation staying present, unchanged, is already everything both
    /// `session_epoch_conflict` (to refuse a same-key admission) and an
    /// operator reading `node_counts`'s `reserved` (to see this node stuck)
    /// need. No `SessionClosed` is built or sent -- the sender's own bounded
    /// retry (`pending_close`) does not know anything went wrong here, and
    /// resends the identical `SessionClose` on its own schedule, which is
    /// this sequence's only path back to a confirmed release. Silence here,
    /// not an error reply, is what leaves that path open: the carrier
    /// (`SessionClose`) has no caller waiting on a reply in the first
    /// place, only a peer node's own pending-close bookkeeping, which a
    /// missing acknowledgement already tells everything an error body
    /// could.
    pub(super) async fn fail_session_close(&self, close: p4_adapter::Close, detail: String) {
        self.lifecycle.lock().expect("lifecycle lock").take();
        self.clear_event_fence();
        self.queue.finished();
        if std::env::var_os("P4_AGENT_TRACE_CLOSE").is_some() {
            eprintln!(
                "P4_AGENT_SESSION_CLOSE_FAILED deployment={} sequence={} detail={}",
                close.deployment, close.sequence, detail
            );
        }
        self.drain().await;
    }

    /// Whether `frame` is a `SessionClose` this node can answer immediately,
    /// without ever touching `active_sequences` or its adapter.
    ///
    /// True only when this node currently holds a reservation for the
    /// close's own sequence *and* that reservation's `session_epoch`
    /// disagrees with the one the close names. That is deliberately
    /// narrower than "no reservation matches": a sequence this node holds
    /// nothing for at all is the ordinary idempotent case `Work::Close`
    /// already handles once it reaches the adapter (see `Close`'s own doc),
    /// and every vocabulary that has never opted into `session_epoch`
    /// (default `None`) reports no conflict here either, leaving it exactly
    /// where it always was: dispatched to the adapter, which is still free
    /// to treat it as a no-op.
    ///
    /// When true, the close is answered on the spot: the sender's own
    /// pending-close bookkeeping gets precisely the acknowledgement it is
    /// waiting for (naming the same `close_id` it sent), while the
    /// reservation a newer session holds is left completely alone --
    /// neither released nor even inspected by the adapter.
    pub(super) async fn stale_session_close(&self, frame: &Frame) -> bool {
        let Some((sequence, close_id)) = self.payload.close_identity(frame) else {
            return false;
        };
        let Some(incoming_epoch) = self.payload.session_epoch(frame) else {
            return false;
        };
        let held_epoch = self
            .active_sequences
            .lock()
            .expect("active sequence lock")
            .get(&sequence)
            .copied();
        match held_epoch {
            Some(held) if held != incoming_epoch => {}
            _ => return false,
        }
        if let Some(ack) =
            close::session_closed_frame(frame, &sequence, close_id, self.payload.as_ref())
        {
            self.emit(ack).await;
        }
        true
    }

    /// Refuses ordinary hop-shaped work (`refusal`'s own caller has already
    /// returned for every lifecycle-shaped frame by the time this runs)
    /// naming a sequence this node already holds under a *different*
    /// `session_epoch`.
    ///
    /// Ordinarily this never fires: a reservation is removed from
    /// `active_sequences` in the same tick it is no longer owed, well
    /// before any other session could be admitted under the same sequence
    /// id. The one case that leaves a stale entry behind on purpose is a
    /// native close that failed to confirm its own release
    /// (`events::fail_session_close`) -- and that is exactly the case this
    /// exists to make visible as an explicit refusal rather than a silent
    /// race over a slot the backend may still hold.
    pub(super) fn session_epoch_conflict(&self, frame: &Frame) -> Option<String> {
        let sequence = self.payload.sequence(frame)?;
        let incoming_epoch = self.payload.session_epoch(frame)?;
        let held_epoch = *self
            .active_sequences
            .lock()
            .expect("active sequence lock")
            .get(&sequence.sequence)?;
        (held_epoch != incoming_epoch).then(|| {
            format!(
                "sequence {} reservation is held by a different session and has not been confirmed released",
                sequence.sequence
            )
        })
    }
}
