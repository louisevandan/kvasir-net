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
            || command.total_context_size == 0
            || command.sequence_capacity == 0
        {
            return Err(
                "load requires generation, binary, opaque plan, batch/ubatch, context and sequence capacity".into(),
            );
        }
        if command.load_generation == 0 {
            return Err("load generation must be non-zero".into());
        }
        let reserved_context = command
            .context_size
            .checked_mul(command.sequence_capacity as usize)
            .ok_or_else(|| "per-sequence context reservation overflows".to_owned())?;
        if reserved_context > command.total_context_size {
            return Err(format!(
                "total context {} cannot reserve {} sequences x {} tokens",
                command.total_context_size, command.sequence_capacity, command.context_size
            ));
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
        let ready = self
            .lifecycle
            .ready_info()
            .cloned()
            .ok_or_else(|| "loaded stage omitted readiness capabilities".to_owned())?;
        if let Err(detail) = validate_ready_capacities(&command, &ready) {
            let _ = self.lifecycle.unload();
            return Err(detail);
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
            &serde_json::json!({
                "state":"loaded",
                "load_generation":command.load_generation,
                "n_batch":ready.n_batch,
                "n_ubatch":ready.n_ubatch,
                "n_ctx":ready.n_ctx,
                "n_seq_max":ready.n_seq_max,
                "per_sequence_context":command.context_size,
                "reserved_context":reserved_context
            }),
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

fn validate_ready_capacities(
    command: &LoadCommand,
    ready: &crate::process::ReadyInfo,
) -> Result<(), String> {
    if ready.n_ctx < command.total_context_size
        || ready.n_batch < command.n_batch
        || ready.n_ubatch < command.n_ubatch
        || ready.n_seq_max < command.sequence_capacity
    {
        return Err(format!(
            "stage capacity is below the declared load contract: actual n_ctx={} n_batch={} n_ubatch={} n_seq_max={}; required n_ctx={} n_batch={} n_ubatch={} n_seq_max={}",
            ready.n_ctx,
            ready.n_batch,
            ready.n_ubatch,
            ready.n_seq_max,
            command.total_context_size,
            command.n_batch,
            command.n_ubatch,
            command.sequence_capacity
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> LoadCommand {
        LoadCommand {
            load_generation: 1,
            binary: "server".into(),
            endpoint: "127.0.0.1:1".into(),
            plan: "plan".into(),
            args: Vec::new(),
            environment: Vec::new(),
            n_batch: 512,
            n_ubatch: 64,
            context_size: 1_200,
            total_context_size: 12_000,
            sequence_capacity: 10,
            ready_timeout_ms: 1,
            io_timeout_ms: 1,
        }
    }

    fn ready() -> crate::process::ReadyInfo {
        crate::process::ReadyInfo {
            protocol_revision: 1,
            server_id: "ready".into(),
            transactions: false,
            physical_batch: true,
            n_ctx: 12_000,
            n_batch: 512,
            n_ubatch: 64,
            n_seq_max: 10,
        }
    }

    #[test]
    fn declared_parallel_context_fits_actual_llama_capacity() {
        assert_eq!(validate_ready_capacities(&command(), &ready()), Ok(()));
    }

    #[test]
    fn per_sequence_context_cannot_be_mistaken_for_total_llama_context() {
        let mut actual = ready();
        actual.n_ctx = 1_200;
        assert!(
            validate_ready_capacities(&command(), &actual)
                .unwrap_err()
                .contains("actual n_ctx=1200")
        );
    }
}
