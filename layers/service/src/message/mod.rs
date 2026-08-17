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
    /// Ask one concrete adapter for the model facts it can discover locally.
    /// The artifact reference and returned profile remain opaque to P4.
    InspectModel {
        artifact: String,
        adapter: String,
    },
    /// Stops one request. Work already inside a backend runs to its hop
    /// boundary — there is no way to interrupt a hop — so this means the next
    /// one never starts.
    Cancel {
        route: String,
    },
    /// What this agent is doing right now: its lanes, its traffic, and every
    /// node with the routes it is holding.
    ///
    /// Separate from `Inspect`, which is about the machine and does not change
    /// while the process runs. This changes constantly and is the only way
    /// OUTER can see where a request has got to.
    Status,
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
        /// The discovery snapshot used by OUTER to compose this load. Empty
        /// is reserved for legacy/mock tests; production plans must bind it.
        capability_snapshot_id: String,
        /// Unix milliseconds at which the discovery snapshot stops being valid.
        /// Zero is the legacy/mock sentinel and is rejected only when an id is
        /// present.
        capability_expires_at: u64,
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
    /// Write one request's cached state somewhere durable and free the memory.
    ///
    /// One verb rather than two: persisting without freeing saves nothing, and
    /// freeing without persisting is what already happens when a request ends.
    /// Sent to every node of the chain, because each holds its own shard.
    Persist {
        sequence: String,
    },
    /// Bring it back, so the next hop continues where it left off.
    Restore {
        sequence: String,
    },
    /// Copy it under a new id, leaving the original as it was.
    ///
    /// The branch: two continuations of one conversation, neither able to
    /// disturb the other.
    Fork {
        sequence: String,
        into: String,
    },
    /// Delete the durable copy. State nothing ever deletes is a disk filling
    /// up on a schedule nobody set.
    Discard {
        sequence: String,
    },
}

/// Sent back to whoever asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Accepted {
        detail: String,
    },
    Progress {
        stage: u32,
        percent: u32,
    },
    Bound {
        generation: u64,
    },
    Released,
    Token {
        index: u32,
        text: String,
    },
    Done {
        reason: String,
        generated: u32,
    },
    Failed {
        detail: String,
    },
    Machine {
        snapshot: String,
    },
    /// Model metadata and tensor/profile facts discovered by an adapter.
    /// P4 carries the text but does not interpret its backend vocabulary.
    Model {
        artifact: String,
        adapter: String,
        profile: String,
        capability_snapshot_id: String,
        generated_at: u64,
        expires_at: u64,
    },
    /// What the agent is doing, as of the moment it was asked.
    Status {
        snapshot: String,
    },
    /// A cache instruction finished. `sequence` is the id the state now lives
    /// under — the new one after a fork — and `bytes` is what the durable copy
    /// occupies, which an operator persisting thousands of conversations needs
    /// and only the backend knows.
    Cached {
        sequence: String,
        bytes: u64,
        detail: String,
    },
}
