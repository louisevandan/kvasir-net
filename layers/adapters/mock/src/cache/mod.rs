//! The four cache verbs, against state this mock is holding.
//!
//! Apart from the rest because it answers a different question: not what a
//! backend costs, but what it does with a conversation asked to be put away and
//! brought back. Each verb is refused rather than guessed at when it makes no
//! sense — restoring something never persisted, or forking from nothing, is a
//! caller error, and inventing an empty cache for it would let a branch
//! continue from a conversation that does not exist.

use super::{CacheIdentity, Mock, PreparedCache, Progress, spin};
use std::fs;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn cache_identity(cache: &p4_adapter::Cache) -> CacheIdentity {
    CacheIdentity {
        deployment: cache.deployment.clone(),
        stage_id: cache.stage_id.clone(),
        generation: cache.generation,
        sequence: cache.sequence.clone(),
    }
}

pub(crate) mod journal;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DurableState {
    pub bytes: u64,
    pub position: u32,
}

mod manifest;
mod operations;
mod transaction;
