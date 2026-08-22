//! Telling a node that a sequence it may be holding a slot for is over.
//!
//! Every other instruction in this module is driven from outside P4 -- OUTER
//! asks for a load, a hop, a cache mutation. This one is not: only the tail
//! of a chain ever observes a sequence actually finish (or that nobody is
//! listening for it any more), so only the tail's node can know when this is
//! owed, and it is P4's own core that sends it, not a caller. See
//! `agent::node::outcome::close` for who constructs one and when.
//!
//! It is deliberately its own shape rather than folded into `Cache`: a cache
//! mutation is about durable state a caller asked for, and this is about a
//! resident reservation nobody but this node's own admission bookkeeping
//! knows exists. Conflating the two would make a future cache verb have to
//! reason about ceiling accounting it has nothing to do with.

use crate::work::{DeploymentId, SequenceId};

/// One sequence, at one node, that will not be hopped to again.
///
/// Idempotent by construction: a node that has never heard of `sequence`, or
/// that has already closed it, has nothing left to release, and an adapter
/// implementing this verb must treat both as a plain success rather than an
/// error -- a redelivered close is expected, not exceptional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
    pub deployment: DeploymentId,
    /// The deployment generation this instruction was issued against, so a
    /// close for a deployment that has since been rebound is refused before
    /// it reaches the adapter rather than acting on the wrong generation's
    /// ledger. Mirrors `Cache::generation`.
    pub generation: u64,
    pub sequence: SequenceId,
    /// The session `sequence` belonged to when this close was built -- see
    /// `Sequence::session_epoch`'s own doc. An adapter that tracks residency
    /// per session, not merely per sequence id, compares this against
    /// whatever epoch it currently holds `sequence` under before releasing
    /// anything: a close naming an older epoch than the one now active must
    /// never cancel a newer session's reservation, and a close naming an
    /// epoch this adapter has already tombstoned is the ordinary idempotent
    /// no-op `Close`'s own doc describes. `0` is what a caller that has never
    /// minted a session identity sends, and an adapter that never
    /// distinguishes epochs may simply ignore this field the way it always
    /// has.
    pub session_epoch: u64,
}
