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

use crate::status::StatusSnapshot;

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
        /// Transport route of the request to remove from a node queue.
        route: String,
        /// Immutable request identity used to fence a route reuse.
        request_id: String,
        stream_id: String,
        /// The return subscription that owns the request.
        return_channel: String,
        /// Ingress generation observed when the request was admitted.
        generation: u64,
    },
    /// What this agent is doing right now: its lanes, its traffic, and every
    /// node with the routes it is holding.
    ///
    /// Separate from `Inspect`, which is about the machine and does not change
    /// while the process runs. This changes constantly and is the only way
    /// OUTER can see where a request has got to.
    Status,
    /// Confirms receipt of response events for a logical OUTER channel.
    /// Events at or below `event_seq` for this stream become replayable no
    /// longer and may leave the agent's bounded delivery journal.
    Acknowledge {
        return_channel: String,
        stream_id: String,
        event_seq: u64,
    },
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
        /// This session's identity, minted here -- the very first hop-shaped
        /// request for it -- and carried unchanged by every `Continue` that
        /// follows (see that variant's own field) and by every
        /// `SessionClose` a chain's tail eventually builds for it. Distinct
        /// from `SessionClose`'s own `close_id`, which names one
        /// transmission rather than the session: see
        /// `p4_agent_core::node::payload::Payload::session_epoch` for why
        /// both are needed. A caller with nothing meaningful to mint (a
        /// fixture, a non-chained request) sends `0`; a real chained session
        /// must not reuse a value across two unrelated sessions that could
        /// ever share a sequence id.
        session_epoch: u64,
    },
    /// One session, carried a step further. What "a step" is, and what has to
    /// be remembered to take it, belong to the backend; this says only that
    /// there is more to do and hands back what the adapter last produced.
    Continue {
        remaining: u32,
        /// How many tokens P4 has already streamed for this request. Its own
        /// tally of its own output, so the bound holds whatever a backend
        /// does or does not report about itself.
        emitted: u32,
        options: String,
        /// Opaque adapter state. P4 moves it and never reads it.
        state: Vec<u8>,
        /// Echoed back exactly as `Execute` first minted it -- see that
        /// variant's own doc.
        session_epoch: u64,
    },
    /// Write one request's cached state somewhere durable and free the memory.
    ///
    /// One verb rather than two: persisting without freeing saves nothing, and
    /// freeing without persisting is what already happens when a request ends.
    /// Sent to every node of the chain, because each holds its own shard.
    Persist {
        sequence: String,
    },
    PreparePersist {
        sequence: String,
    },
    /// Bring it back, so the next hop continues where it left off.
    Restore {
        sequence: String,
    },
    PrepareRestore {
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
    PrepareDiscard {
        sequence: String,
    },
    Commit {
        sequence: String,
    },
    Abort {
        sequence: String,
    },
    /// Read the adapter-owned durable receipt for this operation without
    /// replaying a mutation. The request identity remains in the envelope.
    Reconcile {
        sequence: String,
    },
    /// A sequence's session ended at a node further along its chain; this
    /// node's own reservation for it, if it holds one, is no longer owed.
    ///
    /// Never sent by OUTER. P4's own core sends this to every node a
    /// finished (or orphaned) request's chain named besides the one that
    /// decided it was over, because that node is the only one to observe the
    /// backend's own stop and a hop never returns to the others to tell them.
    /// See `agent::node::outcome::close`.
    ///
    /// Acknowledged: the sender keeps this pending and resends it until a
    /// matching `SessionClosed` arrives or its own bounded retry gives up.
    /// `close_id` is the sender's own identity for *this* close occasion --
    /// not the sequence, which a caller may legitimately reuse for an
    /// unrelated later session (see `tools/drive`'s `Admission::retry`, which
    /// does exactly that) and so cannot by itself tell a late reply for the
    /// old session apart from one for the new. Idempotent: a receiver that
    /// has never heard of `sequence`, or has already closed it, answers with
    /// `SessionClosed` regardless.
    SessionClose {
        sequence: String,
        close_id: u64,
        /// The session `sequence` belonged to when this close was built --
        /// see `Execute::session_epoch`'s own doc. A receiver refuses to act
        /// on this close's behalf when its own currently held reservation
        /// for `sequence` names a *different* session, which is what keeps
        /// a resend that outlives its own session from touching whatever
        /// reused the same sequence id afterward.
        session_epoch: u64,
    },
    /// Acknowledges one `SessionClose`, naming back the same `sequence` and
    /// `close_id` so the original sender can retire exactly the pending
    /// entry this answers and no other -- see `SessionClose`'s own doc for
    /// why `sequence` alone cannot do that fencing. Sent only in answer to a
    /// `SessionClose` this node actually processed; never sent
    /// unprompted.
    SessionClosed {
        sequence: String,
        close_id: u64,
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
        /// Optional final token carried atomically with the terminal. This
        /// closes the max_tokens boundary without racing a separate token
        /// reply against the terminal lane.
        final_token: Option<(u32, String)>,
    },
    Failed {
        detail: String,
    },
    /// A cache instruction failed and carries the same identity as `Cached`.
    /// Generic failures intentionally remain identity-free for non-cache work.
    CacheFailed {
        deployment: String,
        stage_id: String,
        generation: u64,
        operation_id: String,
        sequence: String,
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
    /// Correlated, machine-readable monitoring state. The legacy `Status`
    /// string remains for peers that have not negotiated this variant.
    StatusSnapshot {
        snapshot: StatusSnapshot,
    },
    /// A cache instruction finished. `sequence` is the id the state now lives
    /// under — the new one after a fork — and `bytes` is what the durable copy
    /// occupies, which an operator persisting thousands of conversations needs
    /// and only the backend knows.
    Cached {
        deployment: String,
        stage_id: String,
        generation: u64,
        operation_id: String,
        sequence: String,
        bytes: u64,
        detail: String,
    },
    /// Adapter-owned receipt state used by coordinator recovery/reconciliation.
    CacheStatus {
        deployment: String,
        stage_id: String,
        generation: u64,
        operation_id: String,
        sequence: String,
        state: String,
        bytes: u64,
        detail: String,
    },
}
