//! What happens to a sequence after a hop ended.
//!
//! Three cases, and they are the whole routing behaviour of an inference:
//! hand on to the next node, finish, or start another lap. Pure — an outcome
//! and the frame it belonged to in, frames out — because this is where a
//! chain either closes correctly or leaks a route, and that must be provable
//! without a socket.
//!
//! Bodies stay opaque. The core never learns what a message means, so a token
//! carries its text as bytes and nothing here parses them.

pub mod close;

use crate::node::payload::Payload;
use p4_adapter::Outcome;
use p4_protocol::frame::Frame;

/// What a node does next with one sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Not the end of the chain: the same work travels on, body untouched.
    Hop(Frame),
    /// The end of the chain and the sequence is done. Terminal for its route.
    Finish(Frame),
    /// The end of the chain with tokens still wanted: report this one and
    /// start another lap from the first node.
    Lap { token: Frame, lap: Box<Frame> },
    /// A decode lap that advanced backend state but produced no externally
    /// visible text (the first KV-priming decode is the normal case).
    LapWithoutToken { lap: Frame },
    /// The end of the chain and nobody is listening. Legitimate for work sent
    /// without a continuation, and reported so it is not mistaken for a drop.
    Unheard,
}

/// Decides a sequence's next move.
///
/// `carrier` is the frame this hop ran for; its chain says where in the order
/// this node sat, and its reply address says who asked.
pub fn next(carrier: &Frame, outcome: &Outcome, report: &dyn Payload) -> Next {
    if let Some(onward) = carrier.envelope.to_next_hop() {
        return Next::Hop(Frame {
            envelope: onward,
            // Whatever the adapter produced is already inside the body the
            // payload seam builds. There is nothing to wrap: the state is a
            // field of the continuation rather than an envelope around it.
            body: report.continue_body(carrier, outcome),
        });
    }
    // The last node is where generation lands, because logits exist only at
    // the end of the model. Everything below is therefore about reporting.
    let Some(reply) = carrier.envelope.to_reply() else {
        return Next::Unheard;
    };
    if outcome.is_finished() {
        let mut envelope = reply;
        envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
        let observed = report
            .emitted(carrier)
            .saturating_add(u32::from(!outcome.text.is_empty()));
        let generated = terminal_generated(carrier, outcome, report, observed).unwrap_or(observed);
        return Next::Finish(Frame {
            envelope,
            body: report.finished(outcome.stop.as_deref().unwrap_or_default(), generated),
        });
    }
    // The caller asked for a number of tokens, and that number bounds the ring
    // whatever the backend says. A backend that never reports a stop would
    // otherwise lap forever, which is not a slow request but an unbounded one
    // — and the cost of it lands on every node of the chain at once.
    if let Some(requested) = report.sequence(carrier).map(|sequence| sequence.remaining)
        && requested > 0
    {
        // P4's own tally, and what this hop adds to it. The backend is not
        // asked how far along it is: the bound belongs to the request, so the
        // count that enforces it has to be one P4 can make on its own.
        let counted = report
            .emitted(carrier)
            .saturating_add(u32::from(!outcome.text.is_empty()));
        let final_token_body = (counted == requested)
            .then(|| {
                report.finished_with_token(
                    "length",
                    requested,
                    counted.saturating_sub(1),
                    &outcome.text,
                )
            })
            .flatten();
        let at_length = counted > requested
            || (counted == requested && (outcome.text.is_empty() || final_token_body.is_some()));
        if !at_length {
            // Continue below and start the next decode lap.
        } else {
            let mut envelope = reply;
            envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
            if let Some(body) = final_token_body {
                return Next::Finish(Frame { envelope, body });
            }
            return Next::Finish(Frame {
                envelope,
                body: report.finished("length", requested.min(counted)),
            });
        }
    }
    let Some(lap) = carrier.envelope.to_next_lap() else {
        // A chain that cannot lap has nowhere to continue, so an unfinished
        // sequence ends here rather than silently stalling.
        return Next::Finish(Frame {
            envelope: reply,
            body: report.failure("chain cannot continue"),
        });
    };
    // `event_seq` counts response events on the reply stream, not laps of the
    // chain. A lap that emits nothing must therefore carry the number
    // unchanged: advancing it there leaves a hole in the only numbering a
    // subscriber has, and a hole is indistinguishable from a frame the
    // transport lost — which is precisely the question `event_seq` exists to
    // answer. Empty laps are not rare enough to wave away: the first decode
    // primes the backend's state and produces no text at all, and a backend
    // holding back a partial multi-byte character produces more of them as
    // the answer runs.
    let mut lap = Frame {
        envelope: lap,
        // A decode lap restarts at stage 0. Each stage owns its KV shard, so
        // the previous tail's hidden-state cut-set belongs only to the
        // current lap's stage-to-stage handoff.
        body: report.continue_body(carrier, outcome),
    };
    if outcome.text.is_empty() {
        return Next::LapWithoutToken { lap };
    }
    let emitted_seq = carrier.envelope.event_seq.saturating_add(1);
    lap.envelope.event_seq = emitted_seq;
    Next::Lap {
        token: Frame {
            envelope: {
                let mut envelope = reply;
                envelope.event_seq = emitted_seq;
                envelope
            },
            // The index of this token in what P4 has streamed, which is
            // the only numbering the requester ever sees.
            body: report.token(&outcome.text, report.emitted(carrier)),
        },
        lap: Box::new(lap),
    }
}

/// A native terminal count is authoritative only for a validated length stop.
/// Clamp it to P4's request bound so malformed adapter metadata cannot claim
/// more output than the caller requested.
fn terminal_generated(
    carrier: &Frame,
    outcome: &Outcome,
    report: &dyn Payload,
    observed: u32,
) -> Option<u32> {
    (outcome.stop.as_deref() == Some("length"))
        .then(|| report.sequence(carrier).map(|sequence| sequence.remaining))
        .flatten()
        .zip(outcome.terminal_generated)
        .map(|(bound, generated)| generated.min(bound).max(observed.min(bound)))
}

impl Next {
    /// The frames this decision puts on the queue, in the order they must be
    /// enqueued. A token precedes its lap so a reader never sees the token of
    /// a later position before an earlier one.
    pub fn frames(self) -> Vec<Frame> {
        match self {
            Self::Hop(frame) | Self::Finish(frame) => vec![frame],
            Self::Lap { token, lap } => vec![token, *lap],
            Self::LapWithoutToken { lap } => vec![lap],
            Self::Unheard => Vec::new(),
        }
    }
}

#[cfg(test)]
mod terminal_tests;
#[cfg(test)]
mod tests;
