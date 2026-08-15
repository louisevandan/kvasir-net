//! The ordered nodes a request travels, carried by the request itself.
//!
//! This is source routing: a node reads the chain to learn where to send the
//! work next, so nothing between the nodes has to hold routing state. It is
//! also why the chain lives in the envelope rather than in a registry — a
//! relay that had to look up the next hop would need state that outlives the
//! request.
//!
//! The whole chain travels, not only the remainder. It costs frame size and
//! buys the two things worth having: a node can report where it sat in the
//! order, and a failed request can be retried without reconstructing what it
//! was supposed to visit.
//!
//! A chain of one is valid and is how a backend that spreads a model
//! internally participates. Moves with routing policy.

use super::address::Address;
use super::recipient::NodeId;
use crate::ProtocolError;

/// One executable target, named completely.
///
/// All four parts are needed. Without the generation a request can reach a
/// node that has since been rebound and execute against the wrong
/// materialisation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub address: Address,
    pub node: NodeId,
    pub binding: String,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    links: Vec<Link>,
    position: usize,
}

impl Chain {
    /// An empty chain is refused here rather than discovered at the first hop.
    pub fn new(links: Vec<Link>) -> Result<Self, ProtocolError> {
        if links.is_empty() {
            return Err(ProtocolError::new("a chain must name at least one node"));
        }
        Ok(Self { links, position: 0 })
    }

    pub fn at(links: Vec<Link>, position: usize) -> Result<Self, ProtocolError> {
        let chain = Self::new(links)?;
        if position >= chain.links.len() {
            return Err(ProtocolError::new("chain position is past its end"));
        }
        Ok(Self { position, ..chain })
    }

    pub fn links(&self) -> &[Link] {
        &self.links
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn len(&self) -> usize {
        self.links.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn current(&self) -> &Link {
        &self.links[self.position]
    }

    /// Where this request goes after the current node, if anywhere.
    pub fn peek_next(&self) -> Option<&Link> {
        self.links.get(self.position + 1)
    }

    /// The last node is where generation lands, because logits exist only at
    /// the end of the model.
    pub fn is_last(&self) -> bool {
        self.position + 1 == self.links.len()
    }

    pub fn is_first(&self) -> bool {
        self.position == 0
    }

    /// Advances to the next hop. Returns `None` at the end rather than
    /// wrapping — a decode lap restarts the chain explicitly, so that the
    /// difference between finishing a pass and continuing generation stays
    /// visible.
    pub fn advance(&self) -> Option<Self> {
        self.peek_next().is_some().then(|| Self {
            links: self.links.clone(),
            position: self.position + 1,
        })
    }

    /// Starts another lap. One lap of the ring produces one token per
    /// sequence, so this is what a decode step does after the last node.
    pub fn restart(&self) -> Self {
        Self {
            links: self.links.clone(),
            position: 0,
        }
    }
}

#[cfg(test)]
mod tests;
