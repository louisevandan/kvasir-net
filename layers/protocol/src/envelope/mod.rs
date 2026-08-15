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
            reply_to: None,
            chain: None,
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests;
