//! A concrete adapter with no backend.
//!
//! It implements the whole adapter interface against arithmetic, so a run
//! exercises P4 — framing, routing, admission, windows, cancellation,
//! deadlines — with nothing underneath that could be at fault. When something
//! goes wrong against this adapter, there is no GPU to blame.
//!
//! What it reproduces is the workload as measured, not a generic delay: a load
//! spread over stages, hop cost belonging to a chain position, prefill dearer
//! than a lap, and a window it reports rather than invents.

pub mod cache;
pub mod profile;
mod runtime;

use p4_adapter::{Distribution, Outcome, Phase};
use profile::{Fault, Profile};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// One sequence as it crossed the mock adapter boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceObservation {
    pub sequence: String,
    pub inbound_cut_set: Option<Vec<u8>>,
    pub position: u32,
    pub prompt: Option<String>,
    pub remaining: u32,
    pub options: String,
}

/// The load inputs the mock actually received. Keeping the capability
/// snapshot beside the opaque plan makes discovery/load pass-through
/// observable without interpreting either value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadObservation {
    pub artifact: String,
    pub plan: String,
    pub capability_snapshot_id: String,
    pub capability_expires_at: u64,
}

/// The input and output of one mock hop.
///
/// This is deliberately adapter-owned observation. It lets an integration
/// test compare the mock boundary with a real llama adapter without teaching
/// the P4 scheduler about backend details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HopObservation {
    pub hop_id: u64,
    pub phase: Phase,
    pub sequences: Vec<SequenceObservation>,
    pub outcomes: Vec<Outcome>,
}

/// The observable resident/durable state of one sequence's KV copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheState {
    pub resident: bool,
    pub persisted: bool,
    pub bytes: Option<u64>,
}

/// How far a sequence has got: this turn, and over its life.
///
/// Two numbers because they answer different questions. The turn decides
/// when this request stops; the lifetime is how much state there is, which is
/// what a persisted copy costs.
#[derive(Clone, Copy, Debug, Default)]
struct Progress {
    turn: u32,
    lifetime: u32,
    position: u32,
}

#[derive(Clone)]
pub(crate) struct CacheIdentity {
    pub deployment: String,
    pub stage_id: String,
    pub generation: u64,
    pub sequence: String,
}

impl CacheIdentity {
    pub(crate) fn matches(&self, cache: &p4_adapter::Cache) -> bool {
        self.deployment == cache.deployment
            && self.stage_id == cache.stage_id
            && self.generation == cache.generation
            && self.sequence == cache.sequence
    }
}

#[derive(Clone)]
pub(crate) enum PreparedCache {
    Persist {
        identity: CacheIdentity,
        bytes: u64,
        previous: Option<cache::DurableState>,
        resident: Option<Progress>,
    },
    Restore {
        identity: CacheIdentity,
        bytes: u64,
        position: u32,
        resident: Option<Progress>,
    },
    Discard {
        identity: CacheIdentity,
        previous: Option<cache::DurableState>,
    },
}

pub struct Mock {
    profile: Profile,
    distribution: Distribution,
    /// Chain position this node sits at. Cost belongs to the position, so the
    /// adapter has to be told which one it is playing.
    position: usize,
    /// Whether this is the end of the chain. Only the end holds logits, so
    /// only the end produces a token or declares a sequence finished.
    terminal: bool,
    generation: AtomicU64,
    /// Every window width this adapter was given, so a test can prove the node
    /// batched rather than serialised.
    widths: Mutex<Vec<usize>>,
    /// Nanoseconds this node had nothing in the adapter between two hops.
    ///
    /// Only the gaps between hops, so the ramp before the first and the drain
    /// after the last are excluded: what is being asked is whether a stage
    /// rests while work is queued behind it, not whether a chain fills and
    /// empties. Busy over busy-plus-idle is the utilisation a card would show.
    idle: AtomicU64,
    /// When the last hop ended, for measuring that gap.
    rested: Mutex<Option<std::time::Instant>>,
    /// Nanoseconds spent inside a hop, so a chain can be asked the question the
    /// old runtime measured: stage compute over wall clock. Below one, the
    /// stages took turns; above it, they worked at the same time. It is the
    /// only number that tells pipelining from a queue.
    busy: AtomicU64,
    /// Hops in flight. Must never exceed one for a deployment: a node starts
    /// the next only when it sees the previous end.
    running: AtomicUsize,
    peak_running: AtomicUsize,
    /// Tokens produced so far, per sequence.
    ///
    /// A backend remembers this; the request does not carry it back down. The
    /// KV a sequence occupies lives here too, which is the same reason: state
    /// belongs to whoever is holding the sequence open.
    produced: Mutex<HashMap<String, Progress>>,
    /// Sequences whose state has been written somewhere durable, and how big
    /// each copy is.
    ///
    /// A real backend has a file or a store; what stands in for it here is a
    /// map, because the only thing above the boundary can observe is that the
    /// state survived being freed and came back. Sizes are derived from the
    /// progress the sequence had made, so a longer conversation persists to a
    /// larger copy — the proportion a caller reasons about.
    persisted: Mutex<HashMap<String, u64>>,
    /// Prepared cache mutations keyed by the transaction phase request ID.
    /// Preparation never changes resident state; commit/abort consumes it.
    pub(crate) prepared: Mutex<HashMap<String, PreparedCache>>,
    /// A committed cache keeps its pre-state until the transaction is known
    /// to have succeeded at every stage.  The service barrier can therefore
    /// compensate an earlier commit when a later stage fails.
    pub(crate) committed: Mutex<HashMap<String, PreparedCache>>,
    /// An aborted receipt makes duplicate Abort delivery idempotent across
    /// process restart while preventing a later Commit from being guessed.
    pub(crate) aborted: Mutex<HashMap<String, PreparedCache>>,
    /// Optional file-backed cache root used by integration tests and local
    /// mock runs that need restart evidence. `None` intentionally preserves
    /// the cheap process-local fixture behavior.
    cache_dir: Option<PathBuf>,
    /// Startup recovery errors are retained and surfaced on the first cache
    /// operation instead of silently dropping a malformed transaction file.
    cache_journal_error: Option<String>,
    /// The opaque plan and generation options the adapter actually received.
    /// These are observability evidence, never inputs to P4 scheduling.
    loaded: Mutex<Option<String>>,
    plans: Mutex<Vec<String>>,
    loads: Mutex<Vec<LoadObservation>>,
    options: Mutex<Vec<String>>,
    /// Exact hop inputs and outputs, in adapter call order.
    hops: Mutex<Vec<HopObservation>>,
}

mod engine;

/// The mock's own continuation format, which is the point of the exercise:
/// P4 hands `Sequence::state` back untouched, so what a position is and where
/// it lives is the adapter's to decide. A position and, when the mock is
/// pretending to be staged, its cut-set behind it.
pub(crate) fn encode_state(position: u32, cut_set: Option<Vec<u8>>) -> Vec<u8> {
    let mut bytes = position.to_le_bytes().to_vec();
    if let Some(cut_set) = cut_set {
        bytes.extend_from_slice(&cut_set);
    }
    bytes
}

/// The position a mock outcome carried, read back out of the mock's own
/// state. Tests used to read a P4 field; they read the adapter's format now,
/// which is the point.
pub fn decode_observed_position(state: Option<&Vec<u8>>) -> u32 {
    decode_state(state).0
}

pub(crate) fn decode_state(state: Option<&Vec<u8>>) -> (u32, Option<Vec<u8>>) {
    let Some(state) = state else {
        return (0, None);
    };
    let Some((head, rest)) = state.split_at_checked(4) else {
        return (0, None);
    };
    let position = u32::from_le_bytes(head.try_into().expect("four bytes"));
    (position, (!rest.is_empty()).then(|| rest.to_vec()))
}

fn mock_cut_set(sequence: &str, lifetime: u32) -> Vec<u8> {
    let mut bytes = b"p4-mock-cut-v1\0".to_vec();
    bytes.extend_from_slice(&(sequence.len() as u32).to_le_bytes());
    bytes.extend_from_slice(sequence.as_bytes());
    bytes.extend_from_slice(&lifetime.to_le_bytes());
    bytes
}

fn is_json_object(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('{') && value.ends_with('}')
}

/// Burns the declared time without a runtime.
///
/// The adapter interface is a procedure and must not require an async context
/// to honour a duration; a fleet run drives this from wherever the node runs.
fn spin(duration: std::time::Duration) {
    if duration.is_zero() {
        return;
    }
    std::thread::sleep(duration);
}

fn spin_until<F: Fn() -> bool>(duration: std::time::Duration, cancelled: F) -> bool {
    if duration.is_zero() {
        return cancelled();
    }
    let started = std::time::Instant::now();
    while started.elapsed() < duration {
        if cancelled() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1).min(duration));
    }
    cancelled()
}

#[cfg(test)]
mod tests;
