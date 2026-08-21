//! The node's observable reply boundary.
//!
//! It owns response numbering separately from scheduling because the same
//! reply can originate in an adapter completion, a local refusal, or a
//! lifecycle event. See `apps/p4/docs/testing.md#reply-sequence-contract`.

use super::Node;
use p4_adapter::Work;
use p4_protocol::frame::Frame;
use std::sync::atomic::Ordering;

impl Node {
    /// Builds one observable reply. A carrier records the last reply the route
    /// made visible, so its next reply is exactly one larger. Work that emits
    /// no reply keeps its carrier number unchanged.
    pub(super) fn response_frame(carrier: &Frame, body: Vec<u8>) -> Option<Frame> {
        let mut envelope = carrier.envelope.to_reply()?;
        envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
        Some(Frame { envelope, body })
    }

    /// Sends one reply and reports whether the route had a reply continuation.
    /// Callers with a retained lifecycle carrier use that result to preserve
    /// the next visible sequence for a later progress or terminal frame.
    pub(super) async fn reply(&self, carrier: &Frame, body: Vec<u8>) -> bool {
        let Some(frame) = Self::response_frame(carrier, body) else {
            return false;
        };
        self.emit(frame).await;
        true
    }

    /// Progress retains the lifecycle carrier until a later bound/released
    /// terminal arrives. Advance that retained carrier only after the progress
    /// reply was admitted, so both responses stay contiguous on one route.
    pub(super) async fn reply_lifecycle_progress(&self, stage: u32, percent: u32) {
        let carrier = self.lifecycle.lock().expect("lifecycle lock").clone();
        let Some(carrier) = carrier else {
            return;
        };
        if !self
            .reply(&carrier, self.payload.progress(stage, percent))
            .await
        {
            return;
        }
        let mut lifecycle = self.lifecycle.lock().expect("lifecycle lock");
        if let Some(active) = lifecycle.as_mut()
            && active.envelope.route == carrier.envelope.route
            && active.envelope.event_seq == carrier.envelope.event_seq
        {
            active.envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
        }
    }

    /// Hands a frame to this node's outbox.
    ///
    /// The outbox is drained by a task of its own, which waits for room on the
    /// agent queue. That waiting is backpressure and belongs somewhere — but
    /// not here: this is the same task that receives hop completions, and a
    /// node blocked mid-emit could not observe the hop it is waiting on. The
    /// outbox is the seam that keeps a full lane from becoming a stall.
    pub(super) async fn emit(&self, frame: Frame) {
        self.counts.emitted.fetch_add(1, Ordering::Relaxed);
        if *self.outbox_gate.lock().await {
            self.counts.outbox_lost.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut stop = self.outbox_stop.clone();
        let sent = tokio::select! {
            result = self.outbox.send(frame) => result.is_ok(),
            changed = stop.changed() => !(changed.is_ok() && *stop.borrow()),
        };
        if !sent {
            self.counts.outbox_lost.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(super) async fn reply_error(&self, carrier: &Frame, detail: &str) {
        let body = match self.payload.lifecycle(carrier) {
            Some(Work::Cache(_)) => self.payload.cache_failure(carrier, detail),
            _ => self.payload.failure(detail),
        };
        self.reply(carrier, body).await;
    }
}
