use super::*;

impl Worker {
    pub(super) fn load(&mut self, event: impl std::borrow::Borrow<Event>) -> Result<(), String> {
        let event = event.borrow();
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
        if command.load_generation == 0
            || command.load_generation <= self.state.last_load_generation
        {
            return Err("load generation must be fresh and non-zero".into());
        }
        let runtime_resources = self
            .runtime_resource_probe
            .as_ref()
            .ok_or("runtime edge and receipt resource probe is not configured")?
            .snapshot()?;
        let validated_profile = command.resource_profile.validate_preload(
            self.publisher.storage_snapshot(),
            runtime_resources,
        )?;
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
        with_host_load_gate(|| {
            self.lifecycle.load(
                Box::new(ProcessServerControl::new(launch)),
                Duration::from_millis(command.ready_timeout_ms),
            )
        })
        // Reaching READY includes process start, plan validation, model tensor
        // loading, context construction and capability negotiation. Do not
        // collapse every failure in that sequence into a model-load claim.
        .map_err(|error| format!("stage runtime initialization failed: {error:?}"))?;
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
        if let Err(detail) = command
            .resource_profile
            .validate_ready_bound(ready.max_physical_result_bytes)
        {
            let _ = self.lifecycle.unload();
            return Err(detail);
        }
        self.bind_loaded_identity(command.load_generation)?;
        self.state.batch_capacity = command.n_batch;
        self.state.physical_capacity = command.n_ubatch;
        self.state.max_physical_result_bytes = ready.max_physical_result_bytes;
        self.state.request_budget =
            super::super::request_budget::RequestBudget::new(validated_profile.request_limit);
        self.state.resource_profile = Some(command.resource_profile.clone());
        self.state.equal_sequence_ubatch = ready.equal_sequence_ubatch;
        self.state.max_atomic_sequences = ready.max_atomic_sequences;
        self.state.atomic_batch_exclusive = ready.atomic_batch_exclusive;
        self.state.context_size = command.context_size;
        self.state.sequence_capacity = command.sequence_capacity;
        self.state.free_sequences = (0..command.sequence_capacity).collect();
        self.state.load_generation = command.load_generation;
        self.state.next_speculative_id = 1;
        self.state.clear_verify_fence();
        self.state.clear_flights();
        // This is after the native BindLoad echo, not lazy authority minted
        // from an untrusted first physical capsule. Unload leaves it unbound.
        self.state.physical_receives =
            super::super::physical_receive::PhysicalReceiveLedger::new(command.load_generation)?;
        self.effects.clear();
        self.effects_fenced = false;
        self.set_snapshot("loaded");
        self.emit_json(
            &event,
            reply_target(&event)?,
            EventClass::Telemetry,
            LOADED_CONTENT_TYPE,
            &serde_json::json!({
                "state":"loaded",
                "load_generation":command.load_generation,
                "physical_identity_revision":ready.physical_identity_revision,
                "n_batch":ready.n_batch,
                "n_ubatch":ready.n_ubatch,
                "n_ctx":ready.n_ctx,
                "n_seq_max":ready.n_seq_max,
                "equal_sequence_ubatch":ready.equal_sequence_ubatch,
                "max_atomic_sequences":ready.max_atomic_sequences,
                "atomic_batch_exclusive":ready.atomic_batch_exclusive,
                "max_physical_result_bytes":ready.max_physical_result_bytes,
                "resource_profile":command.resource_profile,
                "upstream_commit":ready.upstream_commit,
                "patch_set":ready.patch_set,
                "backend_inventory":ready.backend_inventory,
                "stage_wire_abi":ready.stage_wire_abi,
                "per_sequence_context":command.context_size,
                "reserved_context":reserved_context
            }),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }

    // This is the production LOAD-to-native boundary, not a readiness-only
    // predicate. No physical slot can be admitted before its exact bind echo.
    fn bind_loaded_identity(&mut self, generation: u64) -> Result<(), String> {
        if generation == 0 || generation <= self.state.last_load_generation {
            return Err("load generation must be fresh and non-zero".into());
        }
        if self
            .lifecycle
            .ready_info()
            .is_none_or(|ready| ready.physical_identity_revision != 1)
        {
            let _ = self.lifecycle.unload();
            return Err("stage omitted physical_identity_revision=1".into());
        }
        // Burn the attempted generation even if the bind reply is lost.
        self.state.last_load_generation = generation;
        let bind = generation.to_le_bytes().to_vec();
        let acknowledgement =
            self.stage_request(Operation::BindLoad, Operation::BindLoad, bind.clone());
        if acknowledgement.as_ref() != Ok(&bind) {
            let _ = self.lifecycle.unload();
            return Err("native load identity binding failed".into());
        }
        Ok(())
    }

    pub(super) fn unload(&mut self, event: impl std::borrow::Borrow<Event>) -> Result<(), String> {
        let event = event.borrow();
        let command: UnloadCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid unload payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("unload load generation is stale".into());
        }
        // UNLOAD is not cancellation or an implicit cluster drain. Preserve
        // locally accepted work, including downstream KV with no head request
        // record. The worker serializes this preflight with native execution.
        self.require_idle_unload()?;
        self.set_snapshot("unloading");
        if let Err(error) = self.lifecycle.unload() {
            // Native cleanup may have partially happened. Unlike a busy
            // preflight rejection, this cannot resume the old loaded session
            // or acknowledge queued input. handle() reports and exits fenced.
            self.effects_fenced = true;
            return Err(format!("stage unload failed: {error:?}"));
        }
        self.state.sessions.clear();
        self.service_budget.clear();
        self.state.requests.clear();
        self.state.pending.clear();
        self.state.free_sequences.clear();
        self.state.batch_capacity = 0;
        self.state.physical_capacity = 0;
        self.state.max_physical_result_bytes = 0;
        self.state.request_budget = super::super::request_budget::RequestBudget::default();
        self.state.resource_profile = None;
        self.state.equal_sequence_ubatch = false;
        self.state.max_atomic_sequences = 0;
        self.state.atomic_batch_exclusive = false;
        self.state.context_size = 0;
        self.state.sequence_capacity = 0;
        self.state.load_generation = 0;
        // The ledger was scoped to that load generation; it has nothing left
        // to say about the next one.
        self.state.forget_session_keys();
        self.state.next_speculative_id = 1;
        self.state.clear_verify_fence();
        self.state.clear_flights();
        self.effects.clear();
        self.effects_fenced = false;
        self.set_snapshot("unloaded");
        self.emit_json(
            &event,
            reply_target(&event)?,
            EventClass::Telemetry,
            UNLOADED_CONTENT_TYPE,
            &serde_json::json!({"state":"unloaded","load_generation":command.load_generation}),
        )
        .map_err(|_| "completion queue is full".to_owned())
    }

    pub(super) fn session(&mut self, event: impl std::borrow::Borrow<Event>) -> Result<(), String> {
        let event = event.borrow();
        let command: SessionCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid session payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        self.service_budget.validate_session(&command.session_id)?;
        if command.load_generation != self.state.load_generation {
            return Err("session load generation is stale".into());
        }
        let stages = command
            .stages
            .iter()
            .map(node_endpoint)
            .collect::<Result<Vec<_>, _>>()?;
        if stages[command.stage_index] != self.endpoint || event.envelope.target != self.endpoint {
            return Err("session local index does not name this worker endpoint".into());
        }
        let first = stages[0].clone();
        let last = stages.last().expect("validated pipeline").clone();
        let next = stages.get(command.stage_index + 1).cloned();
        let previous = command
            .stage_index
            .checked_sub(1)
            .map(|index| stages[index].clone());
        let id = command.session_id.clone();
        if let Some(existing) = self.state.sessions.get(&id) {
            if existing.command != command {
                return Err("a live pipeline session cannot change its owner or route".into());
            }
        }
        let response = self.prepare_json_emission(
            &event,
            reply_target(&event)?,
            EventClass::Telemetry,
            SESSION_READY_CONTENT_TYPE,
            &serde_json::json!({"session_id":id,"state":"ready","load_generation":self.state.load_generation}),
        )?;
        // The body and widest future envelope are prevalidated without
        // assigning an ID. No handler/native work/yield separates authority
        // commit from appending that same response behind the existing FIFO.
        self.state.sessions.insert(
            id,
            PipelineSession {
                command,
                next,
                first,
                previous,
                last,
            },
        );
        self.publish_prepared_emission(response)
            .map_err(|_| "prepared session ready event could not be published".to_owned())
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
        || ready.max_atomic_sequences == 0
        || ready.max_physical_result_bytes == 0
    {
        return Err(format!(
            "stage capacity is below the declared load contract: actual n_ctx={} n_batch={} n_ubatch={} n_seq_max={} max_atomic_sequences={}; required n_ctx={} n_batch={} n_ubatch={} n_seq_max={}",
            ready.n_ctx,
            ready.n_batch,
            ready.n_ubatch,
            ready.n_seq_max,
            ready.max_atomic_sequences,
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

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
            resource_profile: crate::v2::resource_profile::fixture_resource_profile(),
            ready_timeout_ms: 1,
            io_timeout_ms: 1,
        }
    }

    fn ready() -> crate::process::ReadyInfo {
        crate::process::ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: 1,
            server_id: "ready".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 10,
            atomic_batch_exclusive: false,
            n_ctx: 12_000,
            n_batch: 512,
            n_ubatch: 64,
            n_seq_max: 10,
            physical_result_payload_bytes: 0,
            physical_result_tensor_count: 0,
            max_physical_result_bytes: 33_554_432,
            upstream_commit: "fixture-upstream".into(),
            patch_set: "fixture-patch-set".into(),
            backend_inventory: "fixture-backend".into(),
            stage_wire_abi: "unknown".into(),
        }
    }

    fn load_event(command: &LoadCommand) -> Event {
        let mut event = crate::v2::tests::request_state(vec![7]).template.clone();
        event.envelope.payload_content_type = LOAD_CONTENT_TYPE.into();
        event.payload = serde_json::to_vec(command).unwrap();
        event
    }

    fn runtime_probe(edge_count: usize) -> crate::v2::RuntimeResourceProbe {
        crate::v2::RuntimeResourceProbe::new(move || {
            Ok(crate::v2::RuntimeResourceSnapshot {
                edge: crate::v2::ResourceStorageSnapshot {
                    count_limit: edge_count,
                    retained_count: 0,
                    byte_limit: 64 << 20,
                    retained_bytes: 0,
                },
                receipt: crate::v2::ResourceStorageSnapshot {
                    count_limit: 1,
                    retained_count: 0,
                    byte_limit: 1 << 20,
                    retained_bytes: 0,
                },
            })
        })
    }

    fn preload_worker() -> Worker {
        let (_sender, receiver) = mpsc::channel();
        let (publisher, _mailbox) =
            p4_adapter::node_adapter::completion_mailbox_with_limits(1, 1, 64 << 20)
                .unwrap();
        Worker::new(
            Endpoint::node(Address::tcp("127.0.0.1", 43001), "preload", 1),
            receiver,
            publisher,
            Arc::new(Mutex::new(String::new())),
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[test]
    fn actual_load_rejects_missing_or_short_runtime_resources_before_native_start() {
        let event = load_event(&command());
        let mut missing = preload_worker();
        let before = super::super::release_tests::snapshot(&missing);
        assert_eq!(
            missing.load(&event).unwrap_err(),
            "runtime edge and receipt resource probe is not configured"
        );
        assert!(!missing.lifecycle.has_server());
        assert_eq!(super::super::release_tests::snapshot(&missing), before);
        assert!(missing.effects.is_empty());

        let mut short = preload_worker().with_runtime_resource_probe(runtime_probe(0));
        let before = super::super::release_tests::snapshot(&short);
        assert!(short.load(&event).unwrap_err().contains("edge retained count is insufficient"));
        assert!(!short.lifecycle.has_server());
        assert_eq!(super::super::release_tests::snapshot(&short), before);
        assert!(short.effects.is_empty());
    }

    #[test]
    fn declared_parallel_context_fits_actual_llama_capacity() {
        assert_eq!(validate_ready_capacities(&command(), &ready()), Ok(()));
    }

    #[test]
    fn zero_physical_result_bound_cannot_complete_load() {
        let mut actual = ready();
        actual.max_physical_result_bytes = 0;
        assert!(
            validate_ready_capacities(&command(), &actual)
                .unwrap_err()
                .contains("stage capacity is below")
        );
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

    #[derive(Clone, Copy)]
    enum BindReply {
        Echo,
        OtherGeneration,
        Lost,
    }

    struct BindingStage {
        revision: u16,
        reply: BindReply,
        calls: Arc<Mutex<Vec<(Operation, Vec<u8>)>>>,
        shutdowns: Arc<AtomicUsize>,
    }

    impl crate::process::ServerControl for BindingStage {
        fn start(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn wait_ready(&mut self, _: Instant) -> Result<Option<crate::process::ReadyInfo>, String> {
            let mut capabilities = ready();
            capabilities.physical_identity_revision = self.revision;
            Ok(Some(capabilities))
        }
        fn request(&mut self, request: Frame) -> Result<Frame, String> {
            self.calls
                .lock()
                .unwrap()
                .push((request.header.operation, request.body.clone()));
            if request.header.operation != Operation::BindLoad {
                return Err("unexpected pre-bind mutation".into());
            }
            match self.reply {
                BindReply::Lost => Err("bound but reply lost".into()),
                BindReply::Echo => {
                    Frame::new(Operation::BindLoad, request.body).map_err(|e| e.to_string())
                }
                BindReply::OtherGeneration => {
                    Frame::new(Operation::BindLoad, 99u64.to_le_bytes().to_vec())
                        .map_err(|e| e.to_string())
                }
            }
        }
        fn shutdown(&mut self) -> Result<(), String> {
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn binding_worker(
        revision: u16,
        reply: BindReply,
    ) -> (
        Worker,
        Arc<Mutex<Vec<(Operation, Vec<u8>)>>>,
        Arc<AtomicUsize>,
    ) {
        let (_sender, receiver) = mpsc::channel();
        let (publisher, _mailbox) = p4_adapter::node_adapter::completion_mailbox(8);
        let mut worker = Worker::new(
            Endpoint::node(Address::tcp("127.0.0.1", 43001), "bind", 1),
            receiver,
            publisher,
            Arc::new(Mutex::new(String::new())),
            Arc::new(AtomicBool::new(false)),
        );
        let calls = Arc::new(Mutex::new(Vec::new()));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        worker
            .lifecycle
            .load(
                Box::new(BindingStage {
                    revision,
                    reply,
                    calls: Arc::clone(&calls),
                    shutdowns: Arc::clone(&shutdowns),
                }),
                Duration::from_millis(10),
            )
            .unwrap();
        (worker, calls, shutdowns)
    }

    #[test]
    fn product_load_binds_exact_identity_before_any_slot_admission() {
        let (mut worker, calls, shutdowns) = binding_worker(1, BindReply::Echo);
        worker.bind_loaded_identity(7).unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            vec![(Operation::BindLoad, 7u64.to_le_bytes().to_vec())]
        );
        assert_eq!(worker.state.last_load_generation, 7);
        assert_eq!(
            worker.state.load_generation, 0,
            "LOAD installs capacity only after this boundary"
        );
        assert!(worker.state.free_sequences.is_empty());
        assert_eq!(shutdowns.load(Ordering::SeqCst), 0);
        for stale in [0, 6, 7] {
            assert!(worker.bind_loaded_identity(stale).is_err());
        }
        assert_eq!(
            calls.lock().unwrap().len(),
            1,
            "stale binds never reach native"
        );
    }

    #[test]
    fn absent_or_unknown_identity_capability_cannot_use_physical_batch_support() {
        for revision in [0, 2] {
            let (mut worker, calls, shutdowns) = binding_worker(revision, BindReply::Echo);
            assert!(worker.lifecycle.physical_batch_capable());
            assert!(
                worker
                    .bind_loaded_identity(7)
                    .unwrap_err()
                    .contains("physical_identity_revision")
            );
            assert!(calls.lock().unwrap().is_empty());
            assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(worker.state.load_generation, 0);
        }
    }

    #[test]
    fn changed_or_lost_bind_reply_closes_native_and_burns_the_attempted_generation() {
        for reply in [BindReply::OtherGeneration, BindReply::Lost] {
            let (mut worker, calls, shutdowns) = binding_worker(1, reply);
            assert!(
                worker
                    .bind_loaded_identity(7)
                    .unwrap_err()
                    .contains("binding failed")
            );
            assert_eq!(calls.lock().unwrap().len(), 1);
            assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(worker.state.last_load_generation, 7);
            assert_eq!(worker.state.load_generation, 0);
            assert!(worker.state.free_sequences.is_empty());
            assert!(!worker.lifecycle.has_server());
            assert!(worker.bind_loaded_identity(7).is_err());
            assert_eq!(calls.lock().unwrap().len(), 1);
        }
    }

    #[test]
    fn concrete_node_loads_are_serialized_across_one_agent() {
        let entered = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let start = Arc::new(Barrier::new(3));
        let workers = (0..2)
            .map(|_| {
                let entered = Arc::clone(&entered);
                let peak = Arc::clone(&peak);
                let start = Arc::clone(&start);
                thread::spawn(move || {
                    start.wait();
                    with_host_load_gate(|| {
                        let current = entered.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(current, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(10));
                        entered.fetch_sub(1, Ordering::SeqCst);
                    });
                })
            })
            .collect::<Vec<_>>();
        start.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }
}
