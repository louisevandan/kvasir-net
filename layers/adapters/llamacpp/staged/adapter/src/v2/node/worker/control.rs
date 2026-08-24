use super::*;

impl Worker {
    pub(super) fn load(&mut self, event: Event) -> Result<(), String> {
        let command: LoadCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid load payload: {error}"))?;
        if command.binary.is_empty()
            || command.plan.is_empty()
            || command.n_batch == 0
            || command.context_size == 0
            || command.sequence_capacity == 0
        {
            return Err(
                "load requires binary, opaque plan, n_batch, context and sequence capacity".into(),
            );
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
        self.state.context_size = command.context_size;
        self.state.sequence_capacity = command.sequence_capacity;
        self.state.free_sequences = (0..command.sequence_capacity).collect();
        self.set_snapshot("loaded");
        self.emit_json(
            &event,
            reply_target(&event),
            EventClass::Telemetry,
            "application/vnd.p4.llamacpp.loaded-v2+json",
            &serde_json::json!({"state":"loaded","n_batch":command.n_batch}),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn unload(&mut self, event: Event) -> Result<(), String> {
        self.set_snapshot("unloading");
        self.lifecycle
            .unload()
            .map_err(|error| format!("stage unload failed: {error:?}"))?;
        self.state.sessions.clear();
        self.state.requests.clear();
        self.state.pending.clear();
        self.state.free_sequences.clear();
        self.state.batch_capacity = 0;
        self.state.context_size = 0;
        self.state.sequence_capacity = 0;
        self.set_snapshot("unloaded");
        self.emit_json(
            &event,
            reply_target(&event),
            EventClass::Telemetry,
            "application/vnd.p4.llamacpp.unloaded-v2+json",
            &serde_json::json!({"state":"unloaded"}),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn session(&mut self, event: Event) -> Result<(), String> {
        let command: SessionCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid session payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
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
            "application/vnd.p4.llamacpp.session-ready-v2+json",
            &serde_json::json!({"session_id":id,"state":"ready"}),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }
}
