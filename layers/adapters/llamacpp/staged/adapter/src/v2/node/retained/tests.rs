use super::*;
use crate::v2::resource_profile::{ResourceStorageSnapshot, RuntimeResourceSnapshot};
use crate::v2::{LOAD_CONTENT_TYPE, LoadCommand, UNLOAD_CONTENT_TYPE, UnloadCommand};
use p4_adapter::node_adapter::{
    AdapterLifecycleCompletion, CompletionPublisher, retained_event_bytes,
};
use p4_protocol::Address;
use p4_protocol::event::lifecycle::{LifecycleOperation, LifecycleStatus, ResourceState};
use p4_protocol::event::{Envelope, EventClass, OuterEndpoint};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn input(name: &str) -> Event {
    let mut event = crate::v2::tests::request_state(vec![7]).template.clone();
    event.envelope.event_id = format!("owned-{name}");
    event.envelope.correlation_id = name.into();
    event.envelope.payload_content_type = "application/unsupported-owned-probe".into();
    event.payload.reserve_exact(8192);
    event.payload.extend([0, 128, 255, 7]);
    event
}
fn owned(
    publisher: &CompletionPublisher,
    mailbox: &CompletionMailbox,
    event: Event,
) -> RetainedCompletion {
    let claim = publisher
        .try_reserve(1, retained_event_bytes(&event).unwrap())
        .unwrap();
    publisher.publish_reserved(event, claim).unwrap();
    match mailbox.try_take_owned() {
        OwnedPoll::Event(event) => event,
        p => panic!("{p:?}"),
    }
}
fn until(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(
            Instant::now() < end,
            "actual owned worker did not reach the expected hold"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn offer(adapter: &RetainedLlamaNodeAdapter, completion: RetainedCompletion) {
    let mut pending = Some(completion);
    until(
        || match adapter.try_offer_retained(pending.take().unwrap()) {
            Ok(()) => true,
            Err(RetainedOfferError::Full(event)) => {
                pending = Some(event);
                false
            }
            other => panic!("{other:?}"),
        },
    );
}

struct LifecycleStage {
    shutdown_error: bool,
}

impl crate::process::ServerControl for LifecycleStage {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn wait_ready(&mut self, _: Instant) -> Result<Option<crate::process::ReadyInfo>, String> {
        Ok(Some(crate::process::ReadyInfo {
            physical_identity_revision: 1,
            protocol_revision: 1,
            server_id: "lifecycle-stage".into(),
            transactions: false,
            physical_batch: true,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 1,
            atomic_batch_exclusive: false,
            n_ctx: 512,
            n_batch: 64,
            n_ubatch: 64,
            n_seq_max: 1,
            physical_result_payload_bytes: 0,
            physical_result_tensor_count: 0,
            max_physical_result_bytes: 33_554_432,
            upstream_commit: "fixture-upstream".into(),
            patch_set: "fixture-patch-set".into(),
            backend_inventory: "fixture-backend".into(),
            stage_wire_abi: "fixture-stage-wire".into(),
        }))
    }
    fn request(&mut self, request: crate::Frame) -> Result<crate::Frame, String> {
        if request.header.operation != crate::Operation::BindLoad {
            return Err("unexpected lifecycle fixture operation".into());
        }
        crate::Frame::new(crate::Operation::BindLoad, request.body).map_err(|e| e.to_string())
    }
    fn shutdown(&mut self) -> Result<(), String> {
        if self.shutdown_error {
            Err("fixture cleanup failed".into())
        } else {
            Ok(())
        }
    }
}

fn lifecycle_adapter(
    endpoint: Endpoint,
    shutdown_error: bool,
    fill_completion_queue: bool,
) -> RetainedLlamaNodeAdapter {
    let queue_capacity = if fill_completion_queue { 1 } else { 4 };
    let retained_bytes = if fill_completion_queue {
        128 << 20
    } else {
        64 << 20
    };
    let (publisher, mailbox) =
        completion_mailbox_with_limits(queue_capacity, 8, retained_bytes).unwrap();
    if fill_completion_queue {
        publisher
            .try_publish_owned(input("lifecycle-filler"))
            .unwrap();
    }
    let (sender, receiver) = mpsc::sync_channel(4);
    let snapshot = Arc::new(Mutex::new("empty".into()));
    let shutdown = Arc::new(AtomicBool::new(false));
    let probe = RuntimeResourceProbe::new(|| {
        let store = ResourceStorageSnapshot {
            count_limit: 8,
            retained_count: 0,
            byte_limit: 64 << 20,
            retained_bytes: 0,
        };
        Ok(RuntimeResourceSnapshot {
            edge: store,
            receipt: store,
        })
    });
    let worker = Worker::new(
        endpoint,
        receiver,
        publisher,
        snapshot.clone(),
        shutdown.clone(),
    )
    .with_runtime_resource_probe(probe)
    .with_server_factory(Arc::new(move |_| {
        Box::new(LifecycleStage { shutdown_error })
    }));
    RetainedLlamaNodeAdapter::spawn_worker(sender, mailbox, snapshot, shutdown, worker)
}

fn lifecycle_event(endpoint: Endpoint, operation: LifecycleOperation, generation: u64) -> Event {
    let agent = match &endpoint {
        Endpoint::Node { agent, .. } => agent.clone(),
        _ => unreachable!(),
    };
    let payload = match operation {
        LifecycleOperation::Load => serde_json::to_vec(&LoadCommand {
            load_generation: generation,
            binary: "fixture".into(),
            endpoint: "127.0.0.1:43190".into(),
            plan: "model=test".into(),
            args: Vec::new(),
            environment: Vec::new(),
            n_batch: 64,
            n_ubatch: 64,
            context_size: 512,
            total_context_size: 512,
            sequence_capacity: 1,
            resource_profile: crate::v2::resource_profile::fixture_resource_profile(),
            ready_timeout_ms: 100,
            io_timeout_ms: 100,
        })
        .unwrap(),
        LifecycleOperation::Unload => serde_json::to_vec(&UnloadCommand {
            load_generation: generation,
        })
        .unwrap(),
    };
    Event {
        envelope: Envelope {
            protocol_version: 3,
            event_id: format!("llama-lifecycle-{operation:?}-{generation}"),
            correlation_id: format!("llama-lifecycle-correlation-{generation}"),
            causation_id: None,
            source: Endpoint::agent(agent.clone()),
            target: endpoint,
            return_route: Some(OuterEndpoint {
                ingress_agent: agent,
                channel: "llama-lifecycle-test".into(),
                connection_generation: 1,
            }),
            class: EventClass::Control,
            sequence: generation,
            deadline_unix_ms: None,
            adapter_kind: Some("llamacpp".into()),
            payload_content_type: match operation {
                LifecycleOperation::Load => LOAD_CONTENT_TYPE,
                LifecycleOperation::Unload => UNLOAD_CONTENT_TYPE,
            }
            .into(),
        },
        payload,
    }
}

fn take(adapter: &RetainedLlamaNodeAdapter) -> RetainedCompletion {
    let mut output = None;
    until(|| {
        let Some(front) = adapter.peek_retained_completion() else {
            return false;
        };
        match adapter.try_take_retained_matching(&front) {
            OwnedPoll::Event(value) => {
                output = Some(value);
                true
            }
            OwnedPoll::Empty => false,
            OwnedPoll::Closed => panic!("llama lifecycle mailbox closed"),
        }
    });
    output.unwrap()
}

#[test]
fn node_load_lifecycle_llama_worker_targets_agent_and_decodes_typed_terminals() {
    let address = Address::tcp("127.0.0.1", 43189);
    let endpoint = Endpoint::node(address.clone(), "llama-lifecycle", 1);
    let adapter = lifecycle_adapter(endpoint.clone(), false, false);
    let (source_publisher, source) = completion_mailbox_with_limits(4, 8, 64 << 20).unwrap();

    let load = lifecycle_event(endpoint.clone(), LifecycleOperation::Load, 1);
    let load_id = load.envelope.event_id.clone();
    offer(&adapter, owned(&source_publisher, &source, load));
    let loaded = take(&adapter);
    assert_eq!(
        loaded.event().envelope.target,
        Endpoint::agent(address.clone())
    );
    assert_eq!(
        loaded.event().envelope.causation_id.as_deref(),
        Some(load_id.as_str())
    );
    assert_eq!(
        adapter
            .decode_lifecycle_completion(LifecycleOperation::Load, loaded.event())
            .unwrap(),
        AdapterLifecycleCompletion {
            operation: LifecycleOperation::Load,
            status: LifecycleStatus::Succeeded,
            resource_state: ResourceState::Present,
            first_error: None,
            cleanup_error: None,
        }
    );
    drop(loaded);

    let unload = lifecycle_event(endpoint, LifecycleOperation::Unload, 1);
    offer(&adapter, owned(&source_publisher, &source, unload));
    let unloaded = take(&adapter);
    assert_eq!(
        adapter
            .decode_lifecycle_completion(LifecycleOperation::Unload, unloaded.event())
            .unwrap()
            .resource_state,
        ResourceState::Absent
    );
    drop(unloaded);
    until(|| {
        adapter
            .retention_snapshot()
            .is_some_and(|value| value.pending_requests.count == 0)
    });
    assert_eq!(source.storage_snapshot().retained_count, 0);
}

#[test]
fn node_load_lifecycle_llama_cleanup_failure_is_failed_unknown() {
    let address = Address::tcp("127.0.0.1", 43188);
    let endpoint = Endpoint::node(address, "llama-cleanup-failure", 1);
    let adapter = lifecycle_adapter(endpoint.clone(), true, false);
    let (source_publisher, source) = completion_mailbox_with_limits(4, 8, 64 << 20).unwrap();
    offer(
        &adapter,
        owned(
            &source_publisher,
            &source,
            lifecycle_event(endpoint.clone(), LifecycleOperation::Load, 1),
        ),
    );
    drop(take(&adapter));
    offer(
        &adapter,
        owned(
            &source_publisher,
            &source,
            lifecycle_event(endpoint, LifecycleOperation::Unload, 1),
        ),
    );
    let failed = take(&adapter);
    let completion = adapter
        .decode_lifecycle_completion(LifecycleOperation::Unload, failed.event())
        .unwrap();
    assert_eq!(completion.status, LifecycleStatus::Failed);
    assert_eq!(completion.resource_state, ResourceState::Unknown);
    assert!(completion.cleanup_error.is_some());
}

#[test]
fn node_load_lifecycle_llama_full_completion_retires_input_before_terminal() {
    let address = Address::tcp("127.0.0.1", 43187);
    let endpoint = Endpoint::node(address, "llama-full-completion", 1);
    let adapter = lifecycle_adapter(endpoint.clone(), false, true);
    let (source_publisher, source) = completion_mailbox_with_limits(4, 8, 64 << 20).unwrap();
    offer(
        &adapter,
        owned(
            &source_publisher,
            &source,
            lifecycle_event(endpoint, LifecycleOperation::Load, 1),
        ),
    );
    until(|| adapter.snapshot() == "completion_queue_full:waiting");
    assert_eq!(
        source.storage_snapshot().retained_count,
        0,
        "supervised lifecycle input must retire before terminal publication"
    );
    let filler = take(&adapter);
    assert_eq!(filler.event().envelope.event_id, "owned-lifecycle-filler");
    drop(filler);
    let loaded = take(&adapter);
    assert_eq!(
        adapter
            .decode_lifecycle_completion(LifecycleOperation::Load, loaded.event())
            .unwrap()
            .resource_state,
        ResourceState::Present
    );
}

#[test]
fn owned_worker_stop_retains_active_obstructing_and_queued_original_inputs() {
    let (publisher, source) = completion_mailbox_with_limits(1, 8, 1 << 20).unwrap();
    let first = input("first");
    let adapter =
        RetainedLlamaNodeAdapter::new(first.envelope.target.clone(), 1, 1, 8, 1 << 20).unwrap();
    offer(&adapter, owned(&publisher, &source, first));
    until(|| {
        adapter.inner.mailbox.storage_snapshot().queued_count == 1
            && source.storage_snapshot().retained_count == 0
    });
    assert!(
        matches!(adapter.inner.mailbox.try_take(), Poll::Empty),
        "raw consumer cannot strip an owned completion claim"
    );
    let active = input("active");
    let active_pointer = active.payload.as_ptr();
    offer(&adapter, owned(&publisher, &source, active));
    until(|| adapter.snapshot() == "completion_queue_full:waiting");
    assert_eq!(source.storage_snapshot().retained_count, 1);
    let mut bad_ack = input("bad-ack");
    bad_ack.envelope.payload_content_type = crate::v2::RELEASED_CONTENT_TYPE.into();
    let ack_pointer = bad_ack.payload.as_ptr();
    offer(&adapter, owned(&publisher, &source, bad_ack));
    let obstructing = input("obstructing");
    let obstructing_pointer = obstructing.payload.as_ptr();
    offer(&adapter, owned(&publisher, &source, obstructing));
    let queued = input("queued");
    let queued_pointer = queued.payload.as_ptr();
    // The second successful std-channel offer proves the first non-ACK left
    // that channel while native/recursive command service remained blocked.
    offer(&adapter, owned(&publisher, &source, queued));
    assert_eq!(source.storage_snapshot().retained_count, 4);
    adapter.inner.shutting_down.store(true, Ordering::Release);
    let thread = adapter.inner.worker.lock().unwrap().take().unwrap();
    thread.join().unwrap();
    assert!(adapter.stopped.load(Ordering::Acquire));
    assert_eq!(
        source.storage_snapshot().retained_count,
        4,
        "worker exit is not input retirement"
    );
    {
        let remainder = adapter._remainder.lock().unwrap();
        let remainder = remainder.as_ref().unwrap();
        assert_eq!(
            remainder
                .failed_input
                .as_ref()
                .unwrap()
                .event()
                .payload
                .as_ptr(),
            active_pointer
        );
        assert_eq!(
            remainder
                .held_input
                .as_ref()
                .unwrap()
                .event()
                .payload
                .as_ptr(),
            obstructing_pointer
        );
        assert_eq!(
            remainder
                .deferred_ack_error
                .as_ref()
                .unwrap()
                .0
                .event()
                .payload
                .as_ptr(),
            ack_pointer
        );
        let queued = remainder.receiver.try_recv().unwrap();
        assert_eq!(queued.event().payload.as_ptr(), queued_pointer);
        assert!(
            remainder.effect_count() > 0,
            "the exact failed publication remains owned"
        );
        assert_eq!(remainder.state.requests.len(), 0);
        drop(queued);
    }
    assert_eq!(source.storage_snapshot().retained_count, 3);
    let retry = owned(&publisher, &source, input("after-stop"));
    let pointer = retry.event().payload.as_ptr();
    let Err(RetainedOfferError::Closed(retry)) = adapter.try_offer_retained(retry) else {
        panic!("stopped worker accepted more input");
    };
    assert_eq!(retry.event().payload.as_ptr(), pointer);
    retry.retire();
    drop(adapter); // Explicit local abandonment retires values and claims together.
    assert_eq!(source.storage_snapshot().retained_count, 0);
    assert_eq!(source.storage_snapshot().retained_bytes, 0);
}
