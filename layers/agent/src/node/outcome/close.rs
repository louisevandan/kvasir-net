//! Telling the rest of a chain that a sequence will not be hopped to again.
//!
//! `next` decides what happens to the node that just ran a hop: forward it,
//! finish it, lap it. Every one of those keeps every other node in the chain
//! informed for free, because the frame itself keeps travelling to them. Only
//! two outcomes do not: `Next::Finish` and `Next::Unheard` are both dead ends
//! at the *last* node of the chain, and neither one sends anything back to
//! the nodes before it. Those nodes may be holding a native sequence slot
//! open for exactly this request, on the strength of a *prediction*
//! (`intermediate_stage_should_release` in the staged adapter) that only
//! covers the length-terminal case. A sequence that stops early -- EOS well
//! short of its bound -- makes no prediction true, and nothing else was ever
//! going to arrive to correct it: the chain does not lap again, so those
//! nodes hear nothing, ever, and the slot leaks for the life of the process.
//! Measured: `Parallel 4, Requests 8` left exactly four sequences stuck this
//! way, one per never-admitted request.
//!
//! This is the fix, and it is an acknowledged contract. The last node
//! already knows the whole chain -- it travelled here in
//! `carrier.envelope.chain`, source-routed the way every hop is -- so it can
//! address every other link directly instead of waiting for one to ask. But
//! delivery of the close is not proof of delivery, and a frame this node
//! never manages to emit is not the only way one goes missing: the outbox
//! counter that watches emission (`Counts::outbox_lost`) says nothing about
//! a downstream queue silently full, a peer connection reset mid-flight, or
//! a process that restarted between accepting the bytes and acting on them.
//! `Node::begin_session_closes` (see `runner::pending_close`) is what turns
//! "sent" into "known to have landed": it keeps every close this function
//! builds pending, resends it on a bounded schedule until a matching
//! `SessionClosed` answers, and only then lets it go. A node that receives a
//! close only clears its own reservation *after* its adapter actually
//! confirms the release (`Event::Closed`, handled in `runner::events`) and
//! answers with `SessionClosed` naming this call's `close_id` back --
//! idempotently, so a resend that arrives after the first attempt already
//! landed gets the same answer again rather than a second reservation
//! release. `close_id`, not `sequence`, is what a late or duplicate answer
//! is fenced against, because `sequence` is a caller-chosen request identity
//! that a legitimate retry is free to reuse for an unrelated later session
//! (`tools/drive`'s `Admission::retry` does exactly that) -- see
//! `ToNode::SessionClose`'s own doc.
//!
//! Kept apart from `next` itself on purpose. `next`'s own doc is proud of
//! being provable without a socket, and every frame it already produces
//! (`Next::Hop`, `Next::Lap`, ...) is still exactly the one that pure
//! function decided. This is additional traffic the same decision implies,
//! computed from `next`'s answer rather than folded into it, so a reader
//! auditing routing correctness still has one function to read and a reader
//! auditing this leak still has one function to read.
//!
//! Gated on `Payload::supports_close`, checked first. A vocabulary that has
//! not opted in has no `lifecycle` arm reading a close body back as
//! `Work::Close`, and several `Payload` fixtures in this tree's own tests are
//! permissive enough to read *any* body as ordinary work -- a close sent to
//! one would be replayed as a brand-new hop, and if the receiving node is
//! itself a middle stage, the hop it produces forwards on, finishes, and
//! triggers another close, without end. A real two-agent test caught exactly
//! this the first time this shipped: `outer_seen.len()` reached 647 in under
//! a second. Sending nothing to a vocabulary that never asked for this is the
//! same "undisturbed unless opted in" rule `p4_mock::Profile::reserve_slots`
//! already uses one layer down.

use super::Next;
use crate::node::payload::Payload;
use p4_protocol::envelope::Chain;
use p4_protocol::frame::Frame;
use p4_protocol::{Envelope, QueueClass, Recipient};

/// The close instructions one outcome owes the rest of its chain, if any,
/// each paired with the `close_id` its own body carries.
///
/// `sequence` is the adapter-level identity (`Outcome::sequence`), which is
/// what every node's own ledger keys on -- not `carrier`'s route, which is a
/// transport key and may already have moved on by the time this arrives.
///
/// `next_close_id` is called once per frame produced, never shared across
/// two different target links: each earlier link acknowledges
/// independently, on its own schedule, and giving two links the same id
/// would let one's `SessionClosed` retire the other's still-pending entry.
///
/// Empty whenever `next` kept the chain going (`Hop`, `Lap`,
/// `LapWithoutToken`): those cases are not dead ends, so nobody downstream is
/// left holding anything. Empty too for a chain of one, which has nobody else
/// to tell.
pub fn session_close_frames(
    carrier: &Frame,
    next: &Next,
    sequence: &str,
    report: &dyn Payload,
    mut next_close_id: impl FnMut() -> u64,
) -> Vec<(u64, Frame)> {
    // See `Payload::supports_close`'s own doc: a vocabulary that has not
    // opted in has no `lifecycle` arm for this, and sending it one anyway
    // risks a permissive `sequence` reading it back as brand-new work.
    if !report.supports_close() {
        return Vec::new();
    }
    if !matches!(next, Next::Finish(_) | Next::Unheard) {
        return Vec::new();
    }
    let Some(chain) = carrier.envelope.chain.as_ref() else {
        return Vec::new();
    };
    let links = chain.links();
    // `next` only reaches `Finish`/`Unheard` from the last position (see its
    // own `to_next_hop` check), so every earlier link is someone else's, and
    // the last one is this node -- already handled, needs no message to
    // itself.
    let earlier = links.len().saturating_sub(1);
    // Read once, from the carrier every earlier link's close is built from,
    // rather than per-link: every earlier link is being told about the same
    // session ending, so every one of their closes must name the same
    // identity. See `Payload::session_epoch`'s own doc; `unwrap_or(0)` is the
    // "this vocabulary tracks nothing" case, matching the plain-text default
    // that `close` itself falls back to.
    let session_epoch = report.session_epoch(carrier).unwrap_or(0);
    (0..earlier)
        .map(|position| {
            let close_id = next_close_id();
            (
                close_id,
                Frame {
                    envelope: Envelope {
                        target: links[position].address.clone(),
                        recipient: Recipient::node(links[position].node.clone()),
                        lane: QueueClass::Control,
                        route: format!("close:{}", carrier.envelope.route),
                        request_id: carrier.envelope.request_id.clone(),
                        stream_id: carrier.envelope.stream_id.clone(),
                        // The wire's own rule: an envelope naming a chain must
                        // name its origin and return channel too
                        // (`envelope::wire::encode`). Carried from the
                        // original request only to satisfy that -- the
                        // acknowledgement this provokes answers straight at
                        // this node instead (`session_closed_frame`, built
                        // from this same chain), never at `origin_agent`.
                        origin_agent: carrier.envelope.origin_agent.clone(),
                        return_channel: carrier.envelope.return_channel.clone(),
                        ingress_generation: 0,
                        event_seq: 0,
                        // Not the original hop's deadline: that bound was a
                        // promise to the caller about visible progress, and
                        // this is neither -- it is best-effort internal
                        // cleanup with nothing waiting on it, so it must not
                        // expire and go unrefused just because the request it
                        // is cleaning up after already did. Retries are
                        // bounded separately, by `runner::pending_close`.
                        deadline_unix_ms: 0,
                        reply_to: None,
                        // Positioned at the target link, not left at the
                        // tail's own position, so `Payload::deployment` and
                        // the generation gate in `Node::refusal` read that
                        // node's own binding rather than the sender's.
                        chain: Some(Chain::at(links.to_vec(), position).expect(
                            "position is within the same links this chain already validated",
                        )),
                    },
                    body: report.close(sequence, close_id, session_epoch),
                },
            )
        })
        .collect()
}

/// The acknowledgement for one `SessionClose` this node just finished
/// processing, addressed straight back at whichever node sent it.
///
/// `carrier` is the `SessionClose` frame this node received -- still holding
/// the *whole* original chain, only repositioned to this node's own link
/// (see `session_close_frames`) -- so its last link is exactly the tail that
/// sent it, without this node ever having been told that address any other
/// way. `None` only when `carrier` somehow carries no chain at all, which
/// `Node::refusal` already refuses before a close reaches lifecycle
/// dispatch, so this is a defensive fallback rather than a path a real close
/// takes.
///
/// Deliberately built without `carrier`'s chain, origin agent or return
/// channel: those name OUTER's own path back to whoever asked the original
/// question, and this answers a different question, put by a peer node, not
/// by OUTER. Bypassing `Envelope::to_reply` (which would target
/// `origin_agent`) is what keeps this from being misdelivered to a caller
/// that asked nothing. The wire's inference-envelope rule
/// (`envelope::wire::encode`) does not apply here for the same reason it
/// does not apply to any other control frame with no chain: only a frame
/// that *names* a chain has to carry an origin and return channel with it.
pub fn session_closed_frame(
    carrier: &Frame,
    sequence: &str,
    close_id: u64,
    report: &dyn Payload,
) -> Option<Frame> {
    let chain = carrier.envelope.chain.as_ref()?;
    let closer = chain.links().last()?;
    Some(Frame {
        envelope: Envelope {
            target: closer.address.clone(),
            recipient: Recipient::node(closer.node.clone()),
            lane: QueueClass::Control,
            route: format!("closed:{}", carrier.envelope.route),
            request_id: carrier.envelope.request_id.clone(),
            stream_id: carrier.envelope.stream_id.clone(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: report.session_closed(sequence, close_id),
    })
}

#[cfg(test)]
mod tests;
