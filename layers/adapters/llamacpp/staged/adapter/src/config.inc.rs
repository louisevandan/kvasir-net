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
    /// Sequences this node has already run a hop for. `p4_adapter::Hop`
    /// carries no phase of its own -- an execution holding both a sequence
    /// beginning work and one continuing a decode lap is a fact about a
    /// backend, not about P4 -- so whether a sequence is beginning work
    /// *here* is this adapter's own question to answer, the same way
    /// `served` answers it from its open sessions and the mock answers it
    /// from its produced-token map. A sequence not in this set has not been
    /// through a hop at this node before; everything else has.
    active: Mutex<std::collections::HashSet<String>>,
}
