//! The one place a body stops being opaque.
//!
//! A hop needs a prompt, a position and how many tokens are still wanted, and
//! none of that is in the envelope — putting it there would make every relay
//! carry inference detail it has no use for. So the body is read here, by
//! something supplied from outside the core.
//!
//! The core therefore never learns a message catalog. Swapping what a body
//! means costs one implementation of this trait and touches nothing else.

use p4_adapter::{Sequence, Work};
use p4_protocol::frame::Frame;

pub trait Payload: Send + Sync {
    /// Reads a queued frame into the sequence a hop will carry.
    ///
    /// `None` means this frame is not executable work — it is refused rather
    /// than guessed at, because a malformed body reaching a backend is how a
    /// protocol fault turns into a crash somewhere it cannot be traced.
    fn sequence(&self, frame: &Frame) -> Option<Sequence>;

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

    /// The deployment this frame's work belongs to. Taken from the chain's
    /// current link, which already names it, so a body cannot disagree with
    /// the route it travelled.
    fn deployment(&self, frame: &Frame) -> Option<String> {
        Some(frame.envelope.chain.as_ref()?.current().binding.clone())
    }
}
