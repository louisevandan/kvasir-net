impl Adapter for StagedAdapter {
    fn inspect_model(&self, artifact: &str) -> Result<String, String> {
        // Discovery must be possible before a stage server exists. The
        // backend-neutral GGUF inspector reads the same artifact that Load
        // later forwards in the opaque plan; it does not allocate a model or
        // start a child process.
        p4_adapter::model::inspect_artifact(artifact)
    }

    fn distribution(&self) -> Distribution {
        Distribution::Staged
    }

    fn reserves_sequence_slots(&self) -> bool {
        true
    }

    fn start(&self, work: Work, events: &dyn EventSink) {
        match work {
            // `Work::Load` and `Work::Unload` both hold `self.lifecycle`
            // across `events.raise(..)` below, and `EventSink::raise` is
            // documented as possibly blocking (`event/sink/mod.rs`), not
            // guaranteed to return immediately. That is safe today only
            // because of a fact about the caller, not about this code: the
            // node's event-draining loop never needs to reacquire an
            // adapter-internal lock, and it dispatches new work
            // fire-and-forget rather than calling back into this adapter
            // synchronously from inside event handling. If a future change
            // made handling an event call back into this adapter -- for
            // instance to decide the next hop from inside the same stack
            // frame that raised the event -- that call would try to lock
            // `self.lifecycle` again while this frame still holds it, and
            // this becomes a deadlock rather than a slow path.
            Work::Load(load) => {
                let mut lifecycle = self.lifecycle.lock().expect("staged lifecycle lock");
                match lifecycle.load(self.control(&load.plan), self.config.ready_timeout) {
                    Ok(()) => {
                        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
                        events.raise(Event::Loaded {
                            deployment: load.deployment,
                            generation,
                            allocations: Vec::new(),
                        });
                    }
                    Err(error) => {
                        eprintln!(
                            "STAGED_LOAD_FAILED deployment={} detail={error:?}",
                            load.deployment
                        );
                        Self::failed(
                            events,
                            load.deployment,
                            None,
                            None,
                            format!("staged load failed: {error:?}"),
                        )
                    }
                }
            }
            Work::Unload(unload) => {
                let mut lifecycle = self.lifecycle.lock().expect("staged lifecycle lock");
                match lifecycle.unload() {
                    Ok(()) => {
                        // `LlamaLifecycle::unload()` only succeeds from
                        // `LoadState::Loaded` and moves to the terminal
                        // `Unloaded` (`lifecycle/mod.rs`); `load()` only
                        // succeeds from `Empty`. So this same
                        // `LlamaLifecycle` instance can never be loaded
                        // again, and a node's `Arc<dyn Adapter>` is built
                        // once at `CreateNode` and never swapped -- there is
                        // no code path today that reuses this `sequences`
                        // ledger for a second deployment. This clear exists
                        // for the reload-in-place path that does not exist
                        // yet: the day one instance can be loaded a second
                        // time, it must not inherit this node's residency or
                        // its tombstones from whatever ran here before it.
                        self.sequences
                            .lock()
                            .expect("staged sequence ledger lock")
                            .clear();
                        events.raise(Event::Unloaded {
                            deployment: unload.deployment,
                        })
                    }
                    Err(error) => Self::failed(
                        events,
                        unload.deployment,
                        None,
                        None,
                        format!("staged unload failed: {error:?}"),
                    ),
                }
            }
            Work::Hop(hop) => self.hop(hop, events),
            Work::Cache(cache) => self.cache(cache, events),
            Work::Close(close) => self.close(close, events),
        }
    }

    fn report(&self) -> String {
        format!(
            "staged lifecycle={:?}\nP4_STAGED_TOMBSTONE_REJECTED_V1 count={}\nP4_STAGED_TOMBSTONE_EVICTED_V1 count={}\n{}",
            self.state(),
            self.tombstone_rejections.load(Ordering::Relaxed),
            self.tombstone_evictions.load(Ordering::Relaxed),
            self.telemetry.report()
        )
    }
}
