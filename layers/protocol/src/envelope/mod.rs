//! What a worker reads to decide what to do, without decoding a body.
//!
//! Two decisions live here, in order. Is the target address mine — if not the
//! message is forwarded whole, and whether it is going to another agent or
//! back to OUTER makes no difference at all. If it is mine, does the agent
//! consume it or does a node.
//!
//! Everything those decisions need is in the envelope, which is what lets a
//! socket receiver do nothing but enqueue and a relay stay stateless. The body
//! stays encoded until something that actually handles it looks.

pub mod address;
pub mod chain;
pub mod recipient;
pub(crate) mod wire;

pub use address::{Address, Scheme};
pub use chain::{Chain, Link};
pub use recipient::{NodeId, Recipient};

use crate::QueueClass;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    /// Where this goes. The first decision compares it against our own.
    pub target: Address,
    /// Who consumes it once the address matched.
    pub recipient: Recipient,
    /// Which lane it waits in. Carried rather than derived so lane selection
    /// does not need the body.
    pub lane: QueueClass,
    /// Transport correlation for one exchange.
    pub route: String,
    /// Absolute; zero disables. Checked at hop boundaries, since there is no
    /// way to interrupt work already handed to a backend.
    pub deadline_unix_ms: u64,
    /// Where a response goes. Present because a requester registers a paired
    /// handler rather than waiting, so the reply needs somewhere to be sent.
    pub reply_to: Option<Address>,
    /// The ordered nodes an inference travels. Absent on control messages,
    /// which address one node and stop.
    pub chain: Option<Chain>,
}

impl Envelope {
    /// Whether this agent consumes the message or forwards it untouched.
    ///
    /// The comparison is on the address alone. There is no agent id to look
    /// up, and nothing about the body is consulted.
    pub fn is_mine(&self, own: &Address) -> bool {
        &self.target == own
    }

    /// Retargets a message at the next node in its chain.
    ///
    /// Returns `None` at the end of the chain. Used by a node that finished
    /// its hop and has somewhere to hand the work on to; the reply address and
    /// the deadline ride along unchanged, because they belong to the request
    /// rather than to a hop.
    pub fn to_next_hop(&self) -> Option<Self> {
        let advanced = self.chain.as_ref()?.advance()?;
        let link = advanced.current();
        Some(Self {
            target: link.address.clone(),
            recipient: Recipient::node(link.node.clone()),
            chain: Some(advanced),
            ..self.clone()
        })
    }

    /// Retargets at the chain's first node for another decode lap.
    pub fn to_next_lap(&self) -> Option<Self> {
        let restarted = self.chain.as_ref()?.restart();
        let link = restarted.current();
        Some(Self {
            target: link.address.clone(),
            recipient: Recipient::node(link.node.clone()),
            lane: QueueClass::Decode,
            chain: Some(restarted),
            ..self.clone()
        })
    }

    /// Addresses a reply at whoever asked. `None` when nobody is listening,
    /// which is legitimate for a message sent without a continuation.
    pub fn to_reply(&self) -> Option<Self> {
        let reply_to = self.reply_to.clone()?;
        Some(Self {
            target: reply_to,
            recipient: Recipient::Agent,
            lane: QueueClass::Response,
            // Nothing more is expected of this frame, so nobody should reply
            // to a reply.
            reply_to: None,
            // The chain stays. It was thrown away here, and with it the only
            // record of how this answer got to where it is: an agent that
            // cannot reach the caller had nothing to fall back on, because the
            // path it arrived by was gone. Every link in it is an address that
            // demonstrably reached this machine — it sent the work — so the
            // chain read backwards is a route home that needs no routing table
            // and no agent knowing the shape of the fleet.
            //
            // It costs about two hundred bytes on a frame that carries one
            // token. Measured against a link running at two to seven per cent
            // while generating, that is not a trade worth the alternative,
            // which is a monitoring path that exists only when the caller
            // happens to be directly reachable.
            ..self.clone()
        })
    }

    /// Somewhere else to hand this frame when its target cannot be reached.
    ///
    /// The chain's first link. That is the stage the caller sent the work to,
    /// so it is an agent the caller was connected to — which is the whole
    /// question when a reply cannot be delivered: not "where is the caller",
    /// which nothing here can answer, but "who was talking to it".
    ///
    /// A remote node's agent is often not the one OUTER holds a socket to. It
    /// answers to the address in `reply_to` and, across a subnet or a NAT or
    /// simply a caller that has since moved, that address may be one it cannot
    /// open. The first link is not a guess: it demonstrably reached this
    /// machine, because it is where the work came from.
    ///
    /// `None` when there is no chain, when the chain's first link is the target
    /// already — relaying to the address that just failed is not a fallback —
    /// or when it is this agent itself, which would be a frame handed back to
    /// the thing that could not send it.
    pub fn relay_home(&self, own: &Address) -> Option<Address> {
        let first = self.chain.as_ref()?.links().first()?.address.clone();
        (first != self.target && &first != own).then_some(first)
    }
}

#[cfg(test)]
mod tests;
