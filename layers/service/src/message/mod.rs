//! What goes in a body.
//!
//! The core never reads this — it routes on the envelope alone. So this is
//! vocabulary rather than protocol, and it changes on its own schedule
//! without touching how a frame travels.
//!
//! Kept small on purpose. Every field here is one a relay has to carry and a
//! node has to parse, so anything a backend can be told in its own opaque plan
//! stays out.

pub mod wire;

#[cfg(test)]
mod tests;

/// Sent to an agent. These are the three things an agent owns: its node
/// registry, taking in an inference, and facts about its machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToAgent {
    /// Creates the id. Nothing is materialised until a load arrives.
    CreateNode {
        node: String,
        /// Which registered adapter backs it. Attaching a new backend is
        /// registering a name here and nothing else.
        adapter: String,
    },
    DeleteNode {
        node: String,
    },
    /// Facts about the machine, for whoever is composing placements.
    Inspect,
}

/// Sent to a node. Materialise, release, or run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToNode {
    Load {
        /// Opaque below the adapter. Carries placement and whatever else the
        /// backend needs.
        plan: String,
        artifact: String,
        /// Concurrency this deployment admits. A ceiling, never derived.
        ceiling: u32,
    },
    Unload,
    /// One sequence's work. Batching is the node's decision, so this describes
    /// one request and never a window.
    Execute {
        prompt: String,
        max_tokens: u32,
        /// Opaque sampling options, passed through whole.
        options: String,
    },
}

/// Sent back to whoever asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Accepted { detail: String },
    Progress { stage: u32, percent: u32 },
    Bound { generation: u64 },
    Released,
    Token { index: u32, text: String },
    Done { reason: String, generated: u32 },
    Failed { detail: String },
    Machine { snapshot: String },
}
