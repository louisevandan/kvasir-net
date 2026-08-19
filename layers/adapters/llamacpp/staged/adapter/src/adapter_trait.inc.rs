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
                    Ok(()) => events.raise(Event::Unloaded {
                        deployment: unload.deployment,
                    }),
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
        }
    }

    fn report(&self) -> String {
        format!("staged lifecycle={:?}\n{}", self.state(), self.telemetry.report())
    }
}
