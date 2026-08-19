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
    // A P4CUT01 marker is a committed wire choice. If it is truncated or its
    // inner body is invalid, never treat the opaque carrier as an ordinary
    // Execute body and forward it to another stage. The service boundary
    // catches this too, but rejecting here prevents a malformed wrapper from
    // being copied or replaced during an intermediate HOP/LAP.
    if p4_adapter::is_continuation(&carrier.body)
        && p4_adapter::decode_continuation(&carrier.body).is_none()
    {
        let Some(mut envelope) = carrier.envelope.to_reply() else {
            return Next::Unheard;
        };
        envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
        return Next::Finish(Frame {
            envelope,
            body: report.failure("malformed P4CUT01 continuation"),
        });
    }
    if let Some(onward) = carrier.envelope.to_next_hop() {
        return Next::Hop(Frame {
            envelope: onward,
            body: outcome.outbound_cut_set.as_deref().map_or_else(
                || report.continue_body(carrier, outcome),
                |cut_set| {
                    let original = report.continue_body(carrier, outcome);
                    let original = p4_adapter::decode_continuation(&original)
                        .map(|(_, body)| body)
                        .unwrap_or(original);
                    p4_adapter::encode_continuation(cut_set, &original)
                },
            ),
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
        return Next::Finish(Frame {
            envelope,
            body: report.finished(
                outcome.stop.as_deref().unwrap_or_default(),
                outcome.position,
            ),
        });
    }
    // The caller asked for a number of tokens, and that number bounds the ring
    // whatever the backend says. A backend that never reports a stop would
    // otherwise lap forever, which is not a slow request but an unbounded one
    // — and the cost of it lands on every node of the chain at once.
    if let Some(requested) = report.sequence(carrier).map(|sequence| sequence.remaining)
        && requested > 0
    {
        let final_token_body = (outcome.position == requested)
            .then(|| {
                report.finished_with_token(
                    "length",
                    requested,
                    outcome.position.saturating_sub(1),
                    &outcome.text,
                )
            })
            .flatten();
        let at_length = outcome.position > requested
            || (outcome.position == requested
                && (outcome.text.is_empty() || final_token_body.is_some()));
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
                body: report.finished("length", requested.min(outcome.position)),
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
    let lap = Frame {
        envelope: {
            let mut envelope = lap;
            envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
            envelope
        },
        // A decode lap restarts at stage 0. Each stage owns its KV shard, so
        // the previous tail's hidden-state cut-set belongs only to the
        // current lap's stage-to-stage handoff.
        body: report.continue_body(carrier, outcome),
    };
    if outcome.text.is_empty() {
        Next::LapWithoutToken { lap }
    } else {
        Next::Lap {
            token: Frame {
                envelope: {
                    let mut envelope = reply;
                    envelope.event_seq = carrier.envelope.event_seq.saturating_add(1);
                    envelope
                },
                body: report.token(&outcome.text, outcome.position.saturating_sub(1)),
            },
            lap: Box::new(lap),
        }
    }
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
mod tests;
