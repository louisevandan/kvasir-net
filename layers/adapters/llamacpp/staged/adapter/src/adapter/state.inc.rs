#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransactionKind {
    Persist,
    Restore,
    Discard,
}

impl TransactionKind {
    fn from_action(action: &p4_adapter::CacheAction) -> Option<Self> {
        match action {
            p4_adapter::CacheAction::PreparePersist => Some(Self::Persist),
            p4_adapter::CacheAction::PrepareRestore => Some(Self::Restore),
            p4_adapter::CacheAction::PrepareDiscard => Some(Self::Discard),
            _ => None,
        }
    }

    fn action(self) -> p4_adapter::CacheAction {
        match self {
            Self::Persist => p4_adapter::CacheAction::Persist,
            Self::Restore => p4_adapter::CacheAction::Restore,
            Self::Discard => p4_adapter::CacheAction::Discard,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingCache {
    cache: p4_adapter::Cache,
    kind: TransactionKind,
}

fn same_cache_identity(left: &p4_adapter::Cache, right: &p4_adapter::Cache) -> bool {
    left.deployment == right.deployment
        && left.stage_id == right.stage_id
        && left.generation == right.generation
        && left.operation_id == right.operation_id
        && left.sequence == right.sequence
}

impl StagedAdapter {
    pub fn new(config: StagedConfig) -> Self {
        Self {
            config,
            lifecycle: Mutex::new(LlamaLifecycle::default()),
            generation: AtomicU64::new(0),
            telemetry: telemetry::RuntimeEvidence::default(),
            transactions: Mutex::new(HashMap::new()),
        }
    }

    pub fn state(&self) -> LoadState {
        self.lifecycle
            .lock()
            .expect("staged lifecycle lock")
            .state()
    }

    fn control(&self, plan: &str) -> ProcessServerControl {
        let endpoint = if self.config.endpoint.port() == 0 {
            TcpListener::bind((self.config.endpoint.ip(), 0))
                .and_then(|listener| listener.local_addr())
                .expect("staged adapter must reserve a local endpoint")
        } else {
            self.config.endpoint
        };
        let mut launch = ServerLaunch::new(
            self.config.binary.clone(),
            endpoint,
            plan.as_bytes().to_vec(),
        );
        launch.args = vec![
            "--port".into(),
            endpoint.port().to_string().into(),
            "--bind".into(),
            endpoint.ip().to_string().into(),
        ];
        if let Some(directory) = self.config.binary.parent() {
            let mut search = vec![directory.to_path_buf()];
            if let Some(root) = directory.parent() {
                let sibling_bin = root.join("bin").join("Release");
                if sibling_bin.is_dir() {
                    search.push(sibling_bin);
                }
            }
            if let Some(existing) = env::var_os("PATH") {
                search.extend(env::split_paths(&existing));
            }
            if let Ok(path) = env::join_paths(search) {
                launch.environment.push(("PATH".into(), path));
            }
        }
        launch.ready_timeout = self.config.ready_timeout;
        launch.io_timeout = self.config.io_timeout;
        launch.protocol_limits = self.config.protocol_limits;
        ProcessServerControl::new(launch)
    }

    fn failed(
        events: &dyn EventSink,
        deployment: String,
        sequence: Option<String>,
        hop_id: Option<u64>,
        detail: impl Into<String>,
    ) {
        events.raise(Event::Failed {
            deployment,
            sequence,
            hop_id,
            detail: detail.into(),
        });
    }
}
