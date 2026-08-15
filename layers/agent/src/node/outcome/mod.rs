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
    Lap { token: Frame, lap: Frame },
    /// The end of the chain and nobody is listening. Legitimate for work sent
    /// without a continuation, and reported so it is not mistaken for a drop.
    Unheard,
}

/// Decides a sequence's next move.
///
/// `carrier` is the frame this hop ran for; its chain says where in the order
/// this node sat, and its reply address says who asked.
pub fn next(carrier: &Frame, outcome: &Outcome) -> Next {
    if let Some(onward) = carrier.envelope.to_next_hop() {
        return Next::Hop(Frame {
            envelope: onward,
            body: carrier.body.clone(),
        });
    }
    // The last node is where generation lands, because logits exist only at
    // the end of the model. Everything below is therefore about reporting.
    let Some(reply) = carrier.envelope.to_reply() else {
        return Next::Unheard;
    };
    if outcome.is_finished() {
        return Next::Finish(Frame {
            envelope: reply,
            body: outcome
                .stop
                .clone()
                .unwrap_or_default()
                .into_bytes(),
        });
    }
    let Some(lap) = carrier.envelope.to_next_lap() else {
        // A chain that cannot lap has nowhere to continue, so an unfinished
        // sequence ends here rather than silently stalling.
        return Next::Finish(Frame {
            envelope: reply,
            body: b"chain cannot continue".to_vec(),
        });
    };
    Next::Lap {
        token: Frame {
            envelope: reply,
            body: outcome.text.clone().into_bytes(),
        },
        lap: Frame {
            envelope: lap,
            body: carrier.body.clone(),
        },
    }
}

impl Next {
    /// The frames this decision puts on the queue, in the order they must be
    /// enqueued. A token precedes its lap so a reader never sees the token of
    /// a later position before an earlier one.
    pub fn frames(self) -> Vec<Frame> {
        match self {
            Self::Hop(frame) | Self::Finish(frame) => vec![frame],
            Self::Lap { token, lap } => vec![token, lap],
            Self::Unheard => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
