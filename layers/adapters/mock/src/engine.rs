use super::*;

impl Mock {
    /// A stage that is not the end of its chain. It advances its layer range
    /// and produces no token, because logits exist only at the end.
    pub fn staged(position: usize, profile: Profile) -> Self {
        Self::new(Distribution::Staged, position, false, profile)
    }

    pub fn staged_with_cache_dir(
        position: usize,
        profile: Profile,
        path: impl AsRef<Path>,
    ) -> Self {
        Self::new_with_cache(
            Distribution::Staged,
            position,
            false,
            profile,
            Some(path.as_ref().to_path_buf()),
        )
    }

    /// The last stage of a chain. This is where generation lands, so this is
    /// the only stage that counts tokens and decides a sequence is finished.
    pub fn terminal(position: usize, profile: Profile) -> Self {
        Self::new(Distribution::Staged, position, true, profile)
    }

    pub fn terminal_with_cache_dir(
        position: usize,
        profile: Profile,
        path: impl AsRef<Path>,
    ) -> Self {
        Self::new_with_cache(
            Distribution::Staged,
            position,
            true,
            profile,
            Some(path.as_ref().to_path_buf()),
        )
    }

    /// A backend that spreads a model itself, the way vLLM and SGLang do. Its
    /// chain is one node long, so it is both the leading and the last stage.
    pub fn internal(profile: Profile) -> Self {
        Self::new(Distribution::Internal, 0, true, profile)
    }

    fn new(distribution: Distribution, position: usize, terminal: bool, profile: Profile) -> Self {
        Self::new_with_cache(distribution, position, terminal, profile, None)
    }

    fn new_with_cache(
        distribution: Distribution,
        position: usize,
        terminal: bool,
        profile: Profile,
        cache_dir: Option<PathBuf>,
    ) -> Self {
        let (prepared, committed, aborted, cache_journal_error) =
            cache::journal::load(cache_dir.as_deref());
        Self {
            profile,
            distribution,
            position,
            terminal,
            generation: AtomicU64::new(0),
            widths: Mutex::new(Vec::new()),
            busy: AtomicU64::new(0),
            idle: AtomicU64::new(0),
            rested: Mutex::new(None),
            running: AtomicUsize::new(0),
            peak_running: AtomicUsize::new(0),
            produced: Mutex::new(HashMap::new()),
            persisted: Mutex::new(HashMap::new()),
            prepared: Mutex::new(prepared),
            committed: Mutex::new(committed),
            aborted: Mutex::new(aborted),
            cache_dir,
            cache_journal_error,
            loaded: Mutex::new(None),
            plans: Mutex::new(Vec::new()),
            loads: Mutex::new(Vec::new()),
            options: Mutex::new(Vec::new()),
            hops: Mutex::new(Vec::new()),
        }
    }

    /// File-backed mock variant for restart-safe cache tests. The production
    /// adapter remains responsible for its own durable format and may refuse
    /// the operation when its backend surface cannot provide one.
    pub fn internal_with_cache_dir(profile: Profile, path: impl AsRef<Path>) -> Self {
        Self::new_with_cache(
            Distribution::Internal,
            0,
            true,
            profile,
            Some(path.as_ref().to_path_buf()),
        )
    }

    pub(crate) fn cache_path(&self, sequence: &str) -> Option<PathBuf> {
        self.cache_dir.as_ref().map(|root| {
            let name: String = sequence
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            root.join(format!("{name}.kv"))
        })
    }

    /// How long this adapter spent inside hops.
    ///
    /// The numerator of the question a chain has to answer: stage compute over
    /// wall clock. One stage can never exceed the wall; a chain of three that
    /// overlaps properly approaches three times it, and a chain that takes
    /// turns stays at one however much work is queued behind it.
    pub fn busy(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.busy.load(Ordering::Relaxed))
    }

    /// How long this adapter had nothing to do between hops.
    ///
    /// Beside `busy`, this is the utilisation of one stage: a chain that never
    /// rests has an idle near zero however long its queue is.
    pub fn idle(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.idle.load(Ordering::Relaxed))
    }

    /// Widths of every hop this adapter ran.
    pub fn widths(&self) -> Vec<usize> {
        self.widths.lock().expect("width log lock").clone()
    }

    /// The most hops this adapter ever had in flight at once. Anything above
    /// one means a node started work beside work.
    pub fn peak_concurrent_hops(&self) -> usize {
        self.peak_running.load(Ordering::SeqCst)
    }
}
#[path = "execution.rs"]
mod execution;

impl Mock {
    /// What this adapter has written down, for a test that wants to check the
    /// state really left memory rather than being copied beside it.
    pub fn persisted(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .persisted
            .lock()
            .expect("persisted")
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    /// Sequences currently resident.
    pub fn resident(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .produced
            .lock()
            .expect("produced")
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    pub fn plans(&self) -> Vec<String> {
        self.plans.lock().expect("plan log lock").clone()
    }

    pub fn load_observations(&self) -> Vec<LoadObservation> {
        self.loads.lock().expect("load log lock").clone()
    }

    pub fn options_seen(&self) -> Vec<String> {
        self.options.lock().expect("options log lock").clone()
    }

    /// Exact adapter-boundary observations, including multi-sequence order.
    pub fn hop_observations(&self) -> Vec<HopObservation> {
        self.hops.lock().expect("hop observation lock").clone()
    }

    /// Resident and durable state for one sequence.
    pub fn cache_state(&self, sequence: &str) -> CacheState {
        let resident = self
            .produced
            .lock()
            .expect("produced")
            .contains_key(sequence);
        let bytes = self
            .persisted
            .lock()
            .expect("persisted")
            .get(sequence)
            .copied()
            .or_else(|| {
                self.read_durable(sequence, None)
                    .ok()
                    .flatten()
                    .map(|state| state.bytes)
            });
        CacheState {
            resident,
            persisted: bytes.is_some(),
            bytes,
        }
    }
}
