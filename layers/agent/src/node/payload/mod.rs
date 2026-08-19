//! The one place a body stops being opaque.
//!
//! A hop needs a prompt, a position and how many tokens are still wanted, and
//! none of that is in the envelope — putting it there would make every relay
//! carry inference detail it has no use for. So the body is read here, by
//! something supplied from outside the core.
//!
//! The core therefore never learns a message catalog. Swapping what a body
//! means costs one implementation of this trait and touches nothing else.

use p4_adapter::{Outcome, Sequence, Work};
use p4_protocol::frame::Frame;

pub trait Payload: Send + Sync {
    /// Reads a queued frame into the sequence a hop will carry.
    ///
    /// `None` means this frame is not executable work — it is refused rather
    /// than guessed at, because a malformed body reaching a backend is how a
    /// protocol fault turns into a crash somewhere it cannot be traced.
    fn sequence(&self, frame: &Frame) -> Option<Sequence>;

    /// Re-encode the logical request for the next decode lap. Vocabulary
    /// owners supply position and remaining-token state while the core keeps
    /// the body opaque.
    fn continue_body(&self, carrier: &Frame, _outcome: &Outcome) -> Vec<u8> {
        carrier.body.clone()
    }

    /// Reads a frame as a load or an unload, if it is one.
    ///
    /// Lifecycle never batches: materialising a model is one instruction about
    /// the whole deployment, not a window of sequences. A node that finds one
    /// runs it alone.
    fn lifecycle(&self, _frame: &Frame) -> Option<Work> {
        None
    }

    /// The concurrency a load declares. Read here because the ceiling lives in
    /// the plan, which is opaque above the adapter.
    ///
    /// The node treats it as a ceiling and never derives one of its own; a
    /// declaration is the only thing that can raise it.
    fn ceiling(&self, _frame: &Frame) -> Option<usize> {
        None
    }

    /// Refuses lifecycle work before it reaches an adapter when its
    /// discovery evidence is no longer valid.
    fn lifecycle_error(&self, _frame: &Frame) -> Option<String> {
        None
    }

    /// The deployment this frame's work belongs to. Taken from the chain's
    /// current link, which already names it, so a body cannot disagree with
    /// the route it travelled.
    fn deployment(&self, frame: &Frame) -> Option<String> {
        Some(frame.envelope.chain.as_ref()?.current().binding.clone())
    }

    // The outbound half of the same seam. A node produces tokens, terminals
    // and progress, and the core must not decide how those are written any
    // more than it decides how a request is read — otherwise a deployment
    // could read its own bodies but not its own answers.
    //
    // The defaults are plain text: enough to run and to read in a log, and
    // replaced wholesale by a deployment that has a vocabulary.

    fn token(&self, text: &str, _index: u32) -> Vec<u8> {
        text.as_bytes().to_vec()
    }

    fn finished(&self, reason: &str, _generated: u32) -> Vec<u8> {
        reason.as_bytes().to_vec()
    }

    /// Optionally emits a terminal carrying the final token atomically. The
    /// default remains the legacy terminal-only vocabulary for adapters that
    /// do not own a structured reply codec.
    fn finished_with_token(
        &self,
        _reason: &str,
        _generated: u32,
        _index: u32,
        _text: &str,
    ) -> Option<Vec<u8>> {
        None
    }

    fn failure(&self, detail: &str) -> Vec<u8> {
        detail.as_bytes().to_vec()
    }

    /// Encodes a lifecycle failure. Cache failures may override this to carry
    /// the request identity needed by a multi-stage barrier; other failures
    /// retain the generic identity-free vocabulary.
    fn cache_failure(&self, _frame: &Frame, detail: &str) -> Vec<u8> {
        self.failure(detail)
    }

    fn progress(&self, stage: u32, percent: u32) -> Vec<u8> {
        format!("stage {stage} at {percent}%").into_bytes()
    }

    fn bound(&self, generation: u64) -> Vec<u8> {
        format!("loaded generation {generation}").into_bytes()
    }

    fn released(&self) -> Vec<u8> {
        b"unloaded".to_vec()
    }

    /// A cache instruction finished. `sequence` is the id the state now lives
    /// under, which is the new one after a fork.
    #[allow(clippy::too_many_arguments)]
    fn cached(
        &self,
        deployment: &str,
        stage_id: &str,
        generation: u64,
        operation_id: &str,
        sequence: &str,
        bytes: u64,
        detail: &str,
    ) -> Vec<u8> {
        format!(
            "cached deployment={deployment} stage={stage_id} generation={generation} operation={operation_id} {sequence} bytes={bytes} {detail}"
        )
        .into_bytes()
    }

    /// Encodes a read-only adapter receipt state. Deployments that use the
    /// structured P4 vocabulary override this; plain payloads remain usable.
    #[allow(clippy::too_many_arguments)]
    fn cache_status(
        &self,
        _deployment: &str,
        _stage_id: &str,
        _generation: u64,
        _operation_id: &str,
        _sequence: &str,
        state: &str,
        _bytes: u64,
        detail: &str,
    ) -> Vec<u8> {
        format!("cache status={state} {detail}").into_bytes()
    }
}
