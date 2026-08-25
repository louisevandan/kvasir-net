use super::*;

impl Worker {
    pub(super) fn load(&mut self, event: Event) -> Result<(), String> {
        let command: LoadCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid load payload: {error}"))?;
        if command.binary.is_empty()
            || command.plan.is_empty()
            || command.n_batch == 0
            || command.n_ubatch == 0
            || command.n_ubatch > command.n_batch
            || command.context_size == 0
            || command.sequence_capacity == 0
        {
            return Err(
                "load requires generation, binary, opaque plan, batch/ubatch, context and sequence capacity".into(),
            );
        }
        if command.load_generation == 0 {
            return Err("load generation must be non-zero".into());
        }
        let endpoint = SocketAddr::from_str(&command.endpoint)
            .map_err(|error| format!("invalid local stage endpoint: {error}"))?;
        let mut launch =
            ServerLaunch::new(&command.binary, endpoint, command.plan.as_bytes().to_vec());
        launch.args = command.args.iter().map(OsString::from).collect();
        if !command.args.iter().any(|value| value == "--port") {
            launch.args.extend([
                OsString::from("--port"),
                OsString::from(endpoint.port().to_string()),
            ]);
        }
        if !command.args.iter().any(|value| value == "--bind") {
            launch.args.extend([
                OsString::from("--bind"),
                OsString::from(endpoint.ip().to_string()),
            ]);
        }
        launch.environment = command
            .environment
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect();
        launch.ready_timeout = Duration::from_millis(command.ready_timeout_ms);
        launch.io_timeout = Duration::from_millis(command.io_timeout_ms);
        self.set_snapshot("loading");
        self.lifecycle
            .load(
                ProcessServerControl::new(launch),
                Duration::from_millis(command.ready_timeout_ms),
            )
            .map_err(|error| format!("stage load failed: {error:?}"))?;
        if !self.lifecycle.physical_batch_capable() {
            let _ = self.lifecycle.unload();
            return Err("stage server did not negotiate physical_batch=1".into());
        }
        self.state.batch_capacity = command.n_batch;
        self.state.physical_capacity = command.n_ubatch;
        self.state.context_size = command.context_size;
        self.state.sequence_capacity = command.sequence_capacity;
        self.state.free_sequences = (0..command.sequence_capacity).collect();
        self.state.load_generation = command.load_generation;
        self.state.next_speculative_id = 1;
        self.state.clear_verify_fence();
        self.set_snapshot("loaded");
        self.emit_json(
            &event,
            reply_target(&event),
            EventClass::Telemetry,
            LOADED_CONTENT_TYPE,
            &serde_json::json!({"state":"loaded","load_generation":command.load_generation,"n_batch":command.n_batch,"n_ubatch":command.n_ubatch}),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn unload(&mut self, event: Event) -> Result<(), String> {
        let command: UnloadCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid unload payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("unload load generation is stale".into());
        }
        self.set_snapshot("unloading");
        self.lifecycle
            .unload()
            .map_err(|error| format!("stage unload failed: {error:?}"))?;
        self.state.sessions.clear();
        self.state.requests.clear();
        self.state.pending.clear();
        self.state.free_sequences.clear();
        self.state.batch_capacity = 0;
        self.state.physical_capacity = 0;
        self.state.context_size = 0;
        self.state.sequence_capacity = 0;
        self.state.load_generation = 0;
        self.state.next_speculative_id = 1;
        self.state.clear_verify_fence();
        self.set_snapshot("unloaded");
        self.emit_json(
            &event,
            reply_target(&event),
            EventClass::Telemetry,
            UNLOADED_CONTENT_TYPE,
            &serde_json::json!({"state":"unloaded","load_generation":command.load_generation}),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn session(&mut self, event: Event) -> Result<(), String> {
        let command: SessionCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid session payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("session load generation is stale".into());
        }
        let first = node_endpoint(&command.first)?;
        let next = command.next.as_ref().map(node_endpoint).transpose()?;
        let id = command.session_id.clone();
        self.state.sessions.insert(
            id.clone(),
            PipelineSession {
                command,
                next,
                first,
            },
        );
        self.emit_json(
            &event,
            reply_target(&event),
            EventClass::Telemetry,
            SESSION_READY_CONTENT_TYPE,
            &serde_json::json!({"session_id":id,"state":"ready","load_generation":self.state.load_generation}),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }
}
