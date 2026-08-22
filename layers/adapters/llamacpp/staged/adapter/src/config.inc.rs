use process::{ProcessServerControl, ServerLaunch};

/// Construction-time facts owned by this concrete adapter. OUTER still owns
/// the opaque plan; this only identifies the executable and local endpoint.
#[derive(Clone, Debug)]
pub struct StagedConfig {
    pub binary: PathBuf,
    pub endpoint: SocketAddr,
    pub ready_timeout: Duration,
    pub io_timeout: Duration,
    pub protocol_limits: ProtocolLimits,
    /// Identity and layer range used to validate a persisted KV snapshot.
    /// These facts are supplied by the load planner; the adapter never parses
    /// OUTER's opaque plan to invent them.
    pub model_identity: Option<String>,
    pub stage_begin: i32,
    pub stage_end: i32,
}

impl StagedConfig {
    pub fn new(binary: impl Into<PathBuf>, endpoint: SocketAddr) -> Self {
        let ready_timeout = env::var("P4_STAGED_READY_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(120));
        let io_timeout = env::var("P4_STAGED_IO_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(60));
        Self {
            binary: binary.into(),
            endpoint,
            // Real GGUF loading and a first CPU/GPU HOP can exceed the
            // control-plane defaults, especially for the large models this
            // adapter is intended to split. These are inactivity deadlines,
            // not promises that every request takes this long.
            ready_timeout,
            io_timeout,
            protocol_limits: ProtocolLimits::default(),
            model_identity: None,
            stage_begin: 0,
            stage_end: 0,
        }
    }

    pub fn with_kv_metadata(
        mut self,
        model_identity: impl Into<String>,
        stage_begin: i32,
        stage_end: i32,
    ) -> Self {
        self.model_identity = Some(model_identity.into());
        self.stage_begin = stage_begin;
        self.stage_end = stage_end;
        self
    }
}

/// How many tombstones `released` remembers before the oldest is evicted to
/// make room for a new one.
///
/// This number has no measurement behind it. Nothing here was profiled
/// against real session churn or derived from a deployment's actual
/// concurrency; it is a guess at "comfortably more than any node has
/// plausibly seen recently."
///
/// An evicted `(sequence, epoch)` pair leaves this detector's reach:
/// `reject_released_sequences` can only refuse what `released` still
/// remembers, so once eviction has happened a redelivered hop for that exact
/// session is indistinguishable from brand-new work and is silently
/// reclassified as a fresh Prefill -- `tests_hop.inc.rs` pins exactly this.
///
/// That is a known limitation, not a hidden regression, once what a
/// post-release arrival *is* is taken into account: it is definitionally a
/// bug, because this node has already told its peers the sequence is gone.
/// Before this tombstone existed at all, every one of those crashed the
/// backend (`llama_decode failed with status -3`). Past eviction, this
/// adapter returns to exactly that pre-fix behaviour -- it does not get any
/// worse than it already was. The tombstone is a **detector**, not a safety
/// mechanism: it turns a confusing backend crash into a clear refusal for as
/// long as it remembers, and raising this cap only buys a longer detection
/// window, never a correctness guarantee the current one is missing.
///
/// The cap counts `(sequence, epoch)` pairs, not distinct sequence ids: a
/// sequence id reused across several sessions (`tools/drive`'s
/// `Admission::retry` does this on purpose) now tombstones one entry per
/// session that has actually ended here, not one entry that would otherwise
/// need clearing before the id could ever be admitted again. That is more
/// entries per id than before this cap's unit changed, not fewer, so the
/// same 4096 guess buys a shorter window per id under heavy reuse than it
/// used to -- a fact for the same operator this counter already serves
/// (`tombstone_evictions`), not a silent change of what the cap means.
const RELEASED_TOMBSTONE_CAP: usize = 4096;

/// Sequences this node has already run a hop for, and the sessions it has
/// released, under one lock.
///
/// `p4_adapter::Hop` carries no phase of its own -- an execution holding
/// both a sequence beginning work and one continuing a decode lap is a fact
/// about a backend, not about P4 -- so whether a sequence is beginning work
/// *here* is this adapter's own question to answer, the same way `served`
/// answers it from its open sessions and the mock answers it from its
/// produced-token map. A sequence not in `active`, or present under a
/// *different* `session_epoch` than the one a hop now names, has not been
/// through a hop at this node under that session before; everything else
/// has. The epoch is what lets a reused sequence id be told apart from a
/// redelivery of the session that used to hold it: `active` maps a sequence
/// to the one epoch currently reserving it, not merely to a presence flag,
/// so a hop naming a fresh epoch for an id this node still shows active
/// under an older one is read as new work rather than folded into whatever
/// that older session was doing.
///
/// But `release_sequence` in `hop_execute.inc.rs` is a *prediction* built from
/// remaining length and stage position, not an observation of the backend
/// actually being done with a sequence: the tail keeps its slot for the
/// length-terminal hop, and an intermediate stage releases one hop early. If
/// that prediction is ever wrong, or a hop is redelivered after a genuine
/// release, a bare `active` set cannot tell the difference from brand-new
/// work -- absence means both "never seen" and "already finished". Losing
/// that distinction sent a finished sequence back through the backend as a
/// fresh Prefill and produced `llama_decode failed with status -3`.
/// `released` is the fix: a `(sequence, epoch)` pair leaves `active` and
/// enters `released` in the same critical section, so the two can never
/// drift apart, and a hop that names an already-released session is
/// rejected instead of misclassified -- while a hop for the *same sequence
/// id* under a new epoch never matches that tombstone at all, because the
/// tombstone names the epoch that ended, not the id alone.
struct SequenceLedger {
    active: std::collections::HashMap<String, u64>,
    released: std::collections::HashSet<(String, u64)>,
    /// Insertion order for `released`, so it can be evicted oldest-first
    /// once it reaches `RELEASED_TOMBSTONE_CAP`.
    released_order: std::collections::VecDeque<(String, u64)>,
}

impl SequenceLedger {
    fn new() -> Self {
        Self {
            active: std::collections::HashMap::new(),
            released: std::collections::HashSet::new(),
            released_order: std::collections::VecDeque::new(),
        }
    }

    /// Moves `(sequence, epoch)` from `active` to `released`, bounding
    /// `released` to `RELEASED_TOMBSTONE_CAP` by evicting the oldest
    /// tombstone first. Returns whether this call evicted one, so a caller
    /// can count it.
    ///
    /// `active`'s entry for `sequence` is removed only when it still names
    /// this exact `epoch` -- a release naming an epoch that has since been
    /// superseded by a newer session's reservation for the same sequence id
    /// must never cancel that newer reservation. This is what keeps a
    /// belated close (or a hop-driven release) for an old session from
    /// touching a session that reused its sequence id afterward.
    fn release(&mut self, sequence: &str, epoch: u64) -> bool {
        if self.active.get(sequence) == Some(&epoch) {
            self.active.remove(sequence);
        }
        let mut evicted = false;
        let key = (sequence.to_owned(), epoch);
        if self.released.insert(key.clone()) {
            self.released_order.push_back(key);
            if self.released_order.len() > RELEASED_TOMBSTONE_CAP
                && let Some(oldest) = self.released_order.pop_front()
            {
                self.released.remove(&oldest);
                evicted = true;
            }
        }
        evicted
    }

    /// Forgets every sequence this node has ever seen. A genuinely new
    /// deployment must not inherit either the residency or the tombstones
    /// of whatever this node ran before it.
    fn clear(&mut self) {
        self.active.clear();
        self.released.clear();
        self.released_order.clear();
    }
}

/// Minimal P4 adapter surface backed by one concrete stage server per load.
///
/// Load/unload and the three durable KV verbs are backed by the owned server
/// process. Fork remains explicit failure because the wire protocol has no
/// copy/rename operation and silently treating it as persist would be wrong.
pub struct StagedAdapter {
    config: StagedConfig,
    lifecycle: Mutex<LlamaLifecycle<ProcessServerControl>>,
    generation: AtomicU64,
    telemetry: telemetry::RuntimeEvidence,
    /// Compatibility fence used only when the negotiated stage server does
    /// not advertise native transaction receipts. Native-capable servers
    /// persist their receipt in the stage-owned journal.
    transactions: Mutex<HashMap<String, PendingCache>>,
    sequences: Mutex<SequenceLedger>,
    /// How many hops this adapter has refused because every sequence they
    /// named -- or some of them -- was already released at this node. A
    /// nonzero count here is the signal that a `release_sequence` prediction
    /// was wrong (or a peer redelivered), which otherwise looks like an
    /// unrelated `llama_decode` failure with nothing pointing back at this
    /// adapter.
    tombstone_rejections: AtomicU64,
    /// How many tombstones have fallen out of `released`'s FIFO cap. Each
    /// one is a sequence this adapter can no longer tell apart from
    /// brand-new work if its hop is ever redelivered -- see
    /// `RELEASED_TOMBSTONE_CAP`. A count here that climbs while
    /// `tombstone_rejections` does not is the signal that the cap is too
    /// small for this deployment's actual churn.
    tombstone_evictions: AtomicU64,
}
