//! The one place a body stops being opaque.
//!
//! A hop needs a prompt, a position and how many tokens are still wanted, and
//! none of that is in the envelope — putting it there would make every relay
//! carry inference detail it has no use for. So the body is read here, by
//! something supplied from outside the core.
//!
//! The core therefore never learns a message catalog. Swapping what a body
//! means costs one implementation of this trait and touches nothing else.

use p4_adapter::deployment::Submit;
use p4_adapter::{Outcome, Sequence, Work};
use p4_protocol::frame::Frame;

pub trait Payload: Send + Sync {
    /// Reads a queued frame into the sequence a hop will carry.
    ///
    /// `None` means this frame is not executable work — it is refused rather
    /// than guessed at, because a malformed body reaching a backend is how a
    /// protocol fault turns into a crash somewhere it cannot be traced.
    fn sequence(&self, frame: &Frame) -> Option<Sequence>;

    /// Reads a frame as the first hop-shaped request of a submission --
    /// fresh work, not a continuation, a lifecycle instruction, or an
    /// acknowledgement -- for a broker relay that diverts it to a registered
    /// deployment client instead of composing a hop.
    ///
    /// The default is `None` for every vocabulary that has not opted in,
    /// exactly like every other method here; a relay that finds `None` falls
    /// back to the existing hop path unchanged. An override is expected to
    /// build the same identity `sequence` would use for this frame (a resend
    /// under one id must remain one submission -- see `Submit`'s own doc),
    /// so `None` here whenever `sequence` would return a `prompt` of `None`
    /// too: a continuation is never a new submission.
    fn submission(&self, _frame: &Frame) -> Option<Submit> {
        None
    }

    /// Re-encode the logical request for the next decode lap. Vocabulary
    /// owners supply position and remaining-token state while the core keeps
    /// the body opaque.
    fn continue_body(&self, carrier: &Frame, _outcome: &Outcome) -> Vec<u8> {
        carrier.body.clone()
    }

    /// How many tokens P4 has already streamed for this request.
    ///
    /// The bound on a request is P4's contract with whoever asked, so P4
    /// counts its own output rather than reading a backend's idea of how far
    /// along it is. A vocabulary that does not carry the tally reports none
    /// and gets the old unbounded behaviour.
    fn emitted(&self, _carrier: &Frame) -> u32 {
        0
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

    /// Encodes an instruction telling another node of the same chain that
    /// `sequence` will not be hopped to again, so any slot it reserved for it
    /// is owed back. Not a reply: the core constructs and sends this itself,
    /// straight at that node, when a chain's tail decides a sequence is
    /// finished (or that nobody is left to hear it) -- see
    /// `node::outcome::close`. `close_id` is the sender's own identity for
    /// this close occasion, echoed back by `SessionClosed` so the sender can
    /// fence a late or duplicate answer against the right pending entry --
    /// see that trait method's own doc for why `sequence` alone cannot do
    /// this.
    ///
    /// `session_epoch` is a different fence for a different confusion:
    /// `close_id` identifies this *transmission*, so a resend of the exact
    /// same close still carries the one `close_id` it was first given, but
    /// nothing about `close_id` says anything about which *session* held
    /// `sequence` when this close was built. A sequence id can legitimately
    /// belong to a second, unrelated session later -- `tools/drive`'s
    /// `Admission::retry` reuses one on purpose -- so a receiver that only
    /// checked `close_id` could still apply a genuinely old close to a
    /// reservation a brand new session now holds. `session_epoch` is minted
    /// once, at that session's own first admission, carried unchanged
    /// through every hop and lap for it (see `session_epoch`'s own doc), and
    /// is what the receiver compares against its *own currently held*
    /// reservation before ever calling its adapter -- see
    /// `Node::session_epoch_conflict` and `Node::stale_session_close`. The
    /// default is plain text, matching every other opaque-vocabulary default
    /// here. Only reachable when `supports_close` says so.
    fn close(&self, sequence: &str, close_id: u64, session_epoch: u64) -> Vec<u8> {
        format!("{sequence}:{close_id}:{session_epoch}").into_bytes()
    }

    /// Encodes the acknowledgement a node sends back after `close`'s
    /// instruction actually finished at its own adapter -- never
    /// optimistically, never before. Answers straight at the node that sent
    /// `close`, not through the OUTER reply path, because the sender's own
    /// pending-close bookkeeping is what is waiting on it, not a caller. The
    /// default mirrors `close`'s own plain-text shape.
    fn session_closed(&self, sequence: &str, close_id: u64) -> Vec<u8> {
        format!("{sequence}:{close_id}").into_bytes()
    }

    /// Reads a frame's body back as the `(sequence, close_id)` a `close`
    /// call produced, if this vocabulary recognizes one. Used by the node
    /// that receives a close, to recover `close_id` for the acknowledgement
    /// once its own adapter actually finishes the work -- not from `close`'s
    /// caller, which has moved on by then.
    fn close_identity(&self, _frame: &Frame) -> Option<(String, u64)> {
        None
    }

    /// Reads a frame's body back as the `(sequence, close_id)` a
    /// `session_closed` call produced, if this vocabulary recognizes one.
    ///
    /// Checked first, before any other frame handling, on every frame a node
    /// receives: an acknowledgement is P4's own internal bookkeeping traffic
    /// answering a pending close, never work to schedule, and reading it as
    /// one would either fence it correctly here or -- for a vocabulary
    /// permissive enough to read arbitrary bytes as a valid request, which
    /// several small test fixtures in this tree are -- schedule it as a
    /// brand-new hop. Default `None`, matching every other opaque-vocabulary
    /// default here: a vocabulary that never sends `SessionClosed` never
    /// needs to recognize one either.
    fn session_closed_ack(&self, _frame: &Frame) -> Option<(String, u64)> {
        None
    }

    /// The session identity this frame carries, if this vocabulary tracks
    /// one: minted once, by whoever sends the very first `Execute` (or
    /// equivalent hop-shaped request) for a session, and then carried
    /// unchanged through every later lap or hop of it -- a `Continue`-shaped
    /// frame must echo back exactly the value its own predecessor carried,
    /// the way it already echoes `options`. Also read from a `SessionClose`,
    /// where it names which session the close believes it is naming, as
    /// opposed to `close_identity`'s `close_id`, which names only the
    /// transmission.
    ///
    /// `None` for ordinary work that carries no identity (a lifecycle frame,
    /// or a vocabulary that has never opted into this at all) and, on
    /// purpose, the default for every vocabulary that does not override it:
    /// `Node::session_epoch_conflict` and `Node::stale_session_close` both
    /// treat `None` as "nothing to fence", so a fixture that has never heard
    /// of this keeps exactly its old admission behaviour rather than
    /// acquiring a new refusal path it never asked for.
    fn session_epoch(&self, _frame: &Frame) -> Option<u64> {
        None
    }

    /// Whether `lifecycle` actually recognizes a body this vocabulary's own
    /// `close` produces, and reads it back as `Work::Close`.
    ///
    /// Default `false`, and load-bearing rather than a formality: a
    /// vocabulary that has not opted in has, by definition, no `lifecycle`
    /// arm for it, so a close frame arriving there falls through to
    /// `sequence`. A vocabulary permissive enough to read arbitrary bytes as
    /// a valid request -- which several small test fixtures in this tree are
    /// -- would then schedule the close as a brand-new hop against whatever
    /// adapter that node has, and if that node is itself a middle stage the
    /// hop it produces forwards on and finishes again, closing the same
    /// chain again, without end. `node::outcome::close` checks this before
    /// building anything, so a vocabulary that never mentions `Work::Close`
    /// in its own `lifecycle` simply never receives one -- exactly its
    /// current behaviour, undisturbed.
    fn supports_close(&self) -> bool {
        false
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
