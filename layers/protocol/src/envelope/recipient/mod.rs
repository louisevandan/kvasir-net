//! Who consumes a message once its address turned out to be ours.
//!
//! Exactly two, and that is a claim about the system rather than a
//! simplification: an agent owns one kind of internal entity, the node. There
//! is no controller. If a third variant ever seems necessary, something has
//! acquired state it was not supposed to have.
//!
//! Should not move.

pub type NodeId = String;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Recipient {
    /// Node creation and deletion, inference intake, hardware inspection.
    /// Facts about the machine and the registry the agent owns.
    Agent,
    /// Model load and unload, prefill and decode. Work belonging to something
    /// that was materialised.
    Node(NodeId),
}

impl Recipient {
    pub fn node(id: impl Into<NodeId>) -> Self {
        Self::Node(id.into())
    }

    /// True when delivery ends at the agent itself rather than being handed to
    /// a node queue. The distinction decides whether a worker finishes the
    /// work or merely moves it.
    pub fn is_agent(&self) -> bool {
        matches!(self, Self::Agent)
    }

    pub fn node_id(&self) -> Option<&str> {
        match self {
            Self::Agent => None,
            Self::Node(id) => Some(id),
        }
    }
}

#[cfg(test)]
mod tests;
