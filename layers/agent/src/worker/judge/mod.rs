//! The two decisions a worker makes, and nothing else.
//!
//! Pure: an envelope and this agent's own address in, a verdict out. No I/O,
//! no state, no body. That is deliberate — this is the hottest code in the
//! system and the easiest to get subtly wrong, so it is the code that must be
//! testable without standing anything up.

use p4_protocol::{Address, Envelope, Recipient};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not ours. Forward the frame whole, without decoding its body. Whether
    /// the address belongs to another agent or to OUTER makes no difference
    /// here, which is what collapses two of the three message kinds into one
    /// path.
    Forward(Address),
    /// Ours, and the agent itself consumes it: node creation and deletion,
    /// inference intake, hardware inspection.
    Agent,
    /// Ours, and a node consumes it. The worker moves it to that node's queue
    /// and is released; it does not wait for the node to act.
    Node(String),
}

/// Decides where an envelope goes. Called once per message, per hop.
pub fn judge(envelope: &Envelope, own: &Address) -> Verdict {
    if !envelope.is_mine(own) {
        return Verdict::Forward(envelope.target.clone());
    }
    match &envelope.recipient {
        Recipient::Agent => Verdict::Agent,
        Recipient::Node(id) => Verdict::Node(id.clone()),
    }
}

#[cfg(test)]
mod tests;
