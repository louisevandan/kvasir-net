use p4_adapter::node_adapter::*;
use p4_hf_adapter::{COMMAND, HfNodeAdapter};
use p4_protocol::event::{
    Endpoint, Envelope, Event, EventClass,
    lifecycle::{LifecycleOperation, LifecycleStatus, ResourceState},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
static IDS: AtomicU64 = AtomicU64::new(1);
static FIXTURE_STARTUP: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn pack(meta: &Value, body: &[u8]) -> Vec<u8> {
    let h = serde_json::to_vec(meta).unwrap();
    let mut b = (h.len() as u32).to_be_bytes().to_vec();
    b.extend(h);
    b.extend(body);
    b
}
fn unpack(bytes: &[u8]) -> Value {
    let n = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[4..4 + n]).unwrap()
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
struct Harness {
    adapter: HfNodeAdapter,
    tx: CompletionPublisher,
    rx: Arc<CompletionMailbox>,
    endpoint: Endpoint,
    outer: Endpoint,
    path: PathBuf,
}
impl Harness {
    fn new(mode: &str) -> Self {
        let id = IDS.fetch_add(1, Ordering::Relaxed);
        let address = "tcp://127.0.0.1:41999".parse().unwrap();
        let endpoint = Endpoint::node(address, "node", 1);
        let outer = Endpoint::outer(
            "tcp://127.0.0.1:41999".parse().unwrap(),
            format!("test{id}"),
            1,
        );
        let adapter = HfNodeAdapter::new(endpoint.clone(), 1, 1, 2, 4096).unwrap();
        let (tx, rx) = completion_mailbox_with_limits(4, 8, 32768).unwrap();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../../../target/hf/fixture-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let fixture = include_bytes!("../../tests/fixtures/bridge_worker/main.py");
        std::fs::write(path.join("entry.py"), fixture).unwrap();
        let bundle = json!({"protocol":2,"entry":"entry.py","files":{"entry.py":hash(fixture)}});
        std::fs::write(
            path.join("bundle.json"),
            serde_json::to_vec(&bundle).unwrap(),
        )
        .unwrap();
        let _ = mode;
        Self {
            adapter,
            tx,
            rx,
            endpoint,
            outer,
            path,
        }
    }
    fn input(&self, meta: Value, body: &[u8]) -> RetainedCompletion {
        self.input_from(meta, body, self.outer.clone())
    }
    fn agent(&self) -> Endpoint {
        match &self.endpoint {
            Endpoint::Node { agent, .. } => Endpoint::agent(agent.clone()),
            _ => unreachable!(),
        }
    }
    fn input_from(&self, meta: Value, body: &[u8], source: Endpoint) -> RetainedCompletion {
        let n = IDS.fetch_add(1, Ordering::Relaxed);
        let route = if let Endpoint::Outer(r) = &self.outer {
            r.clone()
        } else {
            unreachable!()
        };
        let event = Event {
            envelope: Envelope {
                protocol_version: 3,
                event_id: format!("input{n}"),
                correlation_id: format!("corr{n}"),
                causation_id: None,
                source,
                target: self.endpoint.clone(),
                return_route: Some(route),
                class: EventClass::Control,
                sequence: n,
                deadline_unix_ms: None,
                adapter_kind: Some("hf-transformers".into()),
                payload_content_type: COMMAND.into(),
            },
            payload: pack(&meta, body),
        };
        self.tx.try_publish_owned(event).unwrap();
        if let OwnedPoll::Event(c) = self.rx.try_take_owned() {
            c
        } else {
            panic!()
        }
    }
    fn send(&self, meta: Value, body: &[u8]) {
        self.offer(self.input(meta, body));
    }
    fn offer(&self, mut c: RetainedCompletion) {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            match self.adapter.try_offer_retained(c) {
                Ok(()) => return,
                Err(RetainedOfferError::Full(v)) => c = v,
                Err(e) => panic!("{e:?}"),
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn take(&self) -> RetainedCompletion {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(front) = self.adapter.peek_retained_completion() {
                if let OwnedPoll::Event(c) = self.adapter.try_take_retained_matching(&front) {
                    return c;
                }
            }
            assert!(Instant::now() < end, "state {}", self.adapter.snapshot());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn call(&self, meta: Value, body: &[u8]) -> Value {
        self.send(meta, body);
        unpack(&self.take().event().payload)
    }
    fn supervised(&self, meta: Value, body: &[u8]) -> RetainedCompletion {
        let input = self.input_from(meta, body, self.agent());
        let cause = input.event().envelope.event_id.clone();
        self.offer(input);
        let output = self.take();
        assert_eq!(output.event().envelope.target, self.agent());
        assert_eq!(
            output.event().envelope.causation_id.as_deref(),
            Some(cause.as_str())
        );
        output
    }
    fn load(&self, mode: &str) -> Value {
        self.call(self.load_meta(mode), &[])
    }
    fn load_meta(&self, mode: &str) -> Value {
        let _startup: Option<std::sync::MutexGuard<'static, ()>> = Some(
            FIXTURE_STARTUP
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        );
        let nodes = json!([{"agent":"tcp://127.0.0.1:41999","node":"node","generation":1}]);
        json!({"op":"load","generation":1,"nodes":nodes,"index":0,"launch":{
   "python":std::env::var("HF_TEST_PYTHON").unwrap_or("python".into()),"bundle":self.path.join("bundle.json"),
   "bundle_sha256":hash(&std::fs::read(self.path.join("bundle.json")).unwrap()),"config":{"mode":mode},
   "identity":{"generation":1,"index":0,"nodes":nodes},"frame_bytes":1024,"scratch_bytes":4096,"stderr_bytes":1024,"timeout_ms":300}})
    }
    fn job(&self, kind: &str, serial: u64, epoch: u64) -> Value {
        json!({"job":{"generation":1,"epoch":epoch,"serial":serial,"kind":kind,"request":"A","issue":0,"position":0},"receipts":[]})
    }
}

#[test]
fn node_load_lifecycle_hf_supervisor_preserves_outer_owner_and_typed_terminals() {
    let h = Harness::new("normal");
    let loaded = h.supervised(h.load_meta("normal"), &[]);
    assert_eq!(
        h.adapter
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

    // The supervisor receives lifecycle terminals, while inference continues
    // to use the original OUTER route captured from the LOAD request.
    assert_eq!(h.call(h.job("step", 1, 1), b"owned-by-outer")["ok"], true);
    let busy = h.supervised(h.job("unload", 2, 1), &[]);
    let busy_completion = h
        .adapter
        .decode_lifecycle_completion(LifecycleOperation::Unload, busy.event())
        .unwrap();
    assert_eq!(busy_completion.status, LifecycleStatus::Rejected);
    assert_eq!(busy_completion.resource_state, ResourceState::Present);
    drop(busy);

    assert_eq!(h.call(h.job("cancel", 2, 1), b"")["ok"], true);
    let unloaded = h.supervised(h.job("unload", 3, 1), &[]);
    assert_eq!(
        h.adapter
            .decode_lifecycle_completion(LifecycleOperation::Unload, unloaded.event())
            .unwrap()
            .resource_state,
        ResourceState::Absent
    );
}

#[test]
fn node_load_lifecycle_hf_failed_start_and_cleanup_failure_are_not_success() {
    let failed = Harness::new("ready_mismatch");
    let result = failed.supervised(failed.load_meta("ready_mismatch"), &[]);
    let completion = failed
        .adapter
        .decode_lifecycle_completion(LifecycleOperation::Load, result.event())
        .unwrap();
    assert_eq!(completion.status, LifecycleStatus::Failed);
    assert_eq!(completion.resource_state, ResourceState::Absent);
    assert!(completion.first_error.is_some());
    drop(result);

    let uncertain = Harness::new("unload_exit_error");
    let loaded = uncertain.supervised(uncertain.load_meta("unload_exit_error"), &[]);
    drop(loaded);
    let result = uncertain.supervised(uncertain.job("unload", 1, 1), &[]);
    let completion = uncertain
        .adapter
        .decode_lifecycle_completion(LifecycleOperation::Unload, result.event())
        .unwrap();
    assert_eq!(completion.status, LifecycleStatus::Failed);
    assert_eq!(completion.resource_state, ResourceState::Unknown);
    assert!(completion.cleanup_error.is_some());
}

#[test]
fn node_load_lifecycle_hf_supervised_abort_recovers_uncertain_worker_as_unload() {
    let h = Harness::new("death");
    drop(h.supervised(h.load_meta("death"), &[]));
    let failed = h.call(h.job("step", 1, 1), b"starts-native-effect");
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["uncertain"], "partial frame: EOF");

    let abort = json!({"job":{"generation":1,"epoch":0,"serial":0,"kind":"abort",
        "request":"","issue":0,"position":0},"receipts":[]});
    let recovered = h.supervised(abort, &[]);
    assert_eq!(recovered.event().envelope.target, h.agent());
    assert_eq!(
        h.adapter
            .decode_lifecycle_completion(LifecycleOperation::Unload, recovered.event())
            .unwrap(),
        AdapterLifecycleCompletion {
            operation: LifecycleOperation::Unload,
            status: LifecycleStatus::Succeeded,
            resource_state: ResourceState::Absent,
            first_error: None,
            cleanup_error: None,
        }
    );
}

#[test]
fn node_load_lifecycle_hf_full_completion_rejects_unload_before_native_effect() {
    let h = Harness::new("normal");
    drop(h.supervised(h.load_meta("normal"), &[]));
    h.send(h.job("cache", 1, 1), b"");
    let settled = Instant::now() + Duration::from_secs(3);
    while h
        .adapter
        .completion_storage_snapshot()
        .is_none_or(|storage| storage.queued_count != 1)
        || h.rx.storage_snapshot().retained_count != 0
    {
        assert!(Instant::now() < settled, "state {}", h.adapter.snapshot());
        std::thread::sleep(Duration::from_millis(1));
    }

    let unload = h.input_from(h.job("unload", 1, 1), &[], h.agent());
    h.offer(unload);
    let end = Instant::now() + Duration::from_secs(3);
    while h.adapter.snapshot() != "busy" {
        assert!(Instant::now() < end, "state {}", h.adapter.snapshot());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        h.rx.storage_snapshot().retained_count,
        1,
        "reservation saturation retains the unexecuted lifecycle input"
    );

    let held = h.take();
    let terminal = h.take();
    let completion = h
        .adapter
        .decode_lifecycle_completion(LifecycleOperation::Unload, terminal.event())
        .unwrap();
    assert_eq!(completion.status, LifecycleStatus::Rejected);
    assert_eq!(completion.resource_state, ResourceState::Present);
    assert_eq!(h.rx.storage_snapshot().retained_count, 0);

    drop(held);
    drop(terminal);
    let unloaded = h.supervised(h.job("unload", 1, 1), &[]);
    assert_eq!(
        h.adapter
            .decode_lifecycle_completion(LifecycleOperation::Unload, unloaded.event())
            .unwrap()
            .resource_state,
        ResourceState::Absent
    );
}

#[test]
fn node_load_lifecycle_hf_wrapper_crosses_python_frame_without_losing_agent_terminal() {
    let h = Harness::new("lifecycle_frame");
    drop(h.supervised(h.load_meta("lifecycle_frame"), &[]));

    let unloaded = h.supervised(h.job("unload", 1, 1), &[]);
    assert!(
        unloaded.event().payload.len() > 1024,
        "fixture must cross the negotiated Python frame only after adding the lifecycle wrapper"
    );
    assert_eq!(
        h.adapter
            .decode_lifecycle_completion(LifecycleOperation::Unload, unloaded.event())
            .unwrap(),
        AdapterLifecycleCompletion {
            operation: LifecycleOperation::Unload,
            status: LifecycleStatus::Succeeded,
            resource_state: ResourceState::Absent,
            first_error: None,
            cleanup_error: None,
        }
    );
}

#[test]
fn retained_pipe_lifecycle_epoch_and_stale_rejection() {
    let h = Harness::new("normal");
    assert_eq!(h.load("normal")["ok"], true);
    assert_eq!(
        h.call(h.job("step", 1, 1), b"payload")["receipts"][0]["report"]["calls"],
        1
    );
    assert_eq!(h.call(h.job("step", 1, 1), b"payload")["ok"], false);
    assert_eq!(h.call(h.job("unload", 2, 1), b"")["ok"], false);
    assert_eq!(h.call(h.job("cancel", 2, 1), b"")["ok"], true);
    assert_eq!(h.call(h.job("epoch", 3, 2), b"")["ok"], true);
    assert_eq!(h.call(h.job("step", 4, 1), b"stale")["ok"], false);
    assert_eq!(
        h.call(h.job("step", 4, 2), b"fresh")["receipts"][0]["report"]["calls"],
        2
    );
    assert_eq!(h.call(h.job("release", 5, 2), b"")["ok"], true);
    assert_eq!(h.call(h.job("unload", 6, 2), b"")["ok"], true);
    assert_eq!(
        h.adapter
            .completion_storage_snapshot()
            .unwrap()
            .retained_count,
        0
    );
}

#[test]
fn held_output_claims_bound_execution_and_matching_front() {
    let h = Harness::new("normal");
    assert_eq!(h.load("normal")["ok"], true);
    h.send(h.job("step", 1, 1), b"one");
    let held = h.take();
    h.send(h.job("step", 2, 1), b"two");
    let end = Instant::now() + Duration::from_secs(3);
    while h.adapter.peek_retained_completion().is_none() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    let front = h.adapter.peek_retained_completion().unwrap();
    assert_eq!(h.adapter.peek_retained_completion(), Some(front.clone()));
    assert_eq!(
        h.adapter
            .completion_storage_snapshot()
            .unwrap()
            .retained_count,
        2
    );
    let second = h.take();
    assert!(matches!(
        h.adapter.try_take_retained_matching(&front),
        OwnedPoll::Empty
    ));
    h.send(h.job("step", 3, 1), b"three");
    std::thread::sleep(Duration::from_millis(50));
    assert!(h.adapter.peek_retained_completion().is_none());
    let pending = h.adapter.retention_snapshot().unwrap().pending_requests;
    assert_eq!(pending.count, 1);
    assert!(pending.bytes > 0);
    drop(held);
    let third = h.take();
    assert_eq!(
        h.adapter.retention_snapshot().unwrap().pending_requests,
        AdapterRetainedStorage::default()
    );
    assert_eq!(
        unpack(&third.event().payload)["receipts"][0]["report"]["calls"],
        3
    );
    drop(second);
    drop(third);
    assert_eq!(h.call(h.job("cancel", 4, 1), b"")["ok"], true);
    assert_eq!(h.call(h.job("unload", 5, 1), b"")["ok"], true);
}

#[test]
fn pipe_faults_are_uncertain_and_facade_survives_for_abort() {
    for mode in [
        "death", "partial", "magic", "version", "reserved", "length", "identity", "hang",
    ] {
        let h = Harness::new(mode);
        assert_eq!(h.load(mode)["ok"], true, "{mode}");
        let reply = h.call(h.job("step", 1, 1), b"effect");
        assert_eq!(reply["ok"], false, "{mode}");
        assert!(!reply["uncertain"].is_null(), "{mode}: {reply}");
        if matches!(
            mode,
            "partial" | "magic" | "version" | "reserved" | "length" | "identity"
        ) {
            let native = h.adapter.retention_snapshot().unwrap().native_responses;
            assert!(native.count > 0 && native.bytes > 0, "{mode}: {native:?}");
        }
        assert_eq!(h.call(h.job("step", 2, 1), b"effect")["ok"], false);
        let mut abort = h.job("abort", 0, 0);
        abort["job"]["request"] = json!("");
        assert_eq!(h.call(abort, b"")["ok"], true);
    }
}

#[test]
fn incompatible_ready_and_bounded_stderr() {
    let h = Harness::new("ready_mismatch");
    assert_eq!(h.load("ready_mismatch")["ok"], false);
    let h = Harness::new("stderr");
    assert_eq!(h.load("stderr")["ok"], true);
    assert_eq!(h.call(h.job("unload", 1, 1), b"")["ok"], true);
}

#[test]
fn full_and_closed_return_exact_allocation_and_upstream_claim() {
    let h = Harness::new("normal");
    assert_eq!(h.load("normal")["ok"], true);
    h.send(h.job("step", 1, 1), b"first");
    let first = h.take();
    h.send(h.job("step", 2, 1), b"second");
    let second = h.take();
    h.send(h.job("step", 3, 1), b"inflight");
    std::thread::sleep(Duration::from_millis(40));
    h.send(h.job("step", 4, 1), b"queued");
    let value = h.input(h.job("step", 5, 1), b"original");
    let pointer = value.event().payload.as_ptr();
    let envelope = value.event().envelope.clone();
    let before = h.rx.storage_snapshot();
    let value = match h.adapter.try_offer_retained(value) {
        Err(RetainedOfferError::Full(v)) => v,
        other => panic!("{other:?}"),
    };
    assert_eq!(value.event().payload.as_ptr(), pointer);
    assert_eq!(value.event().envelope, envelope);
    assert_eq!(h.rx.storage_snapshot(), before);
    drop(value);
    let large = h.input(h.job("step", 6, 1), &vec![7; 5000]);
    let pointer = large.event().payload.as_ptr();
    let before = h.rx.storage_snapshot();
    let large = match h.adapter.try_offer_retained(large) {
        Err(RetainedOfferError::Closed(v)) => v,
        other => panic!("{other:?}"),
    };
    assert_eq!(large.event().payload.as_ptr(), pointer);
    assert_eq!(h.rx.storage_snapshot(), before);
    drop(large);
    drop(first);
    drop(second);
    drop(h.take());
    drop(h.take());
    assert_eq!(h.call(h.job("cancel", 5, 1), b"")["ok"], true);
    assert_eq!(h.call(h.job("unload", 6, 1), b"")["ok"], true);
}

#[test]
fn unload_and_epoch_refuse_held_output_without_worker_effects() {
    let h = Harness::new("normal");
    assert_eq!(h.load("normal")["ok"], true);
    h.send(h.job("cache", 1, 1), b"");
    let held = h.take();
    assert_eq!(h.call(h.job("unload", 1, 1), b"")["ok"], false);
    assert_eq!(h.call(h.job("epoch", 1, 2), b"")["ok"], false);
    assert_eq!(h.adapter.snapshot(), "busy");
    drop(held);
    assert_eq!(h.call(h.job("epoch", 1, 2), b"")["ok"], true);
    assert_eq!(h.call(h.job("unload", 2, 2), b"")["ok"], true);
}

#[test]
fn completion_poll_wakes_and_drop_interrupts_owned_hung_child() {
    use std::task::{Context, Poll, Wake, Waker};
    struct Signal(std::sync::mpsc::Sender<()>);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            let _ = self.0.send(());
        }
        fn wake_by_ref(self: &Arc<Self>) {
            let _ = self.0.send(());
        }
    }
    let h = Harness::new("hang");
    assert_eq!(h.load("hang")["ok"], true);
    let (tx, rx) = std::sync::mpsc::channel();
    let waker = Waker::from(Arc::new(Signal(tx)));
    let mut cx = Context::from_waker(&waker);
    assert!(matches!(
        h.adapter.poll_take_retained(&mut cx),
        Poll::Pending
    ));
    h.send(h.job("step", 1, 1), b"effect");
    rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let result = match h.adapter.poll_take_retained(&mut cx) {
        Poll::Ready(OwnedPoll::Event(c)) => c,
        other => panic!("{other:?}"),
    };
    assert_eq!(unpack(&result.event().payload)["ok"], false);
    drop(result);
    let start = Instant::now();
    drop(h);
    assert!(start.elapsed() < Duration::from_secs(12));
}

#[cfg(windows)]
#[test]
fn abort_reaps_launcher_and_grandchild_before_acknowledgement() {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    let h = Harness::new("tree");
    let loaded = h.load("tree");
    assert_eq!(loaded["ok"], true, "{loaded}");
    let pid = loaded["ready"]["report"]["child_pid"].as_u64().unwrap() as u32;
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!handle.is_null());
    let mut abort = h.job("abort", 0, 0);
    abort["job"]["request"] = json!("");
    assert_eq!(h.call(abort, b"")["ok"], true);
    assert_eq!(
        unsafe { WaitForSingleObject(handle, 0) },
        WAIT_OBJECT_0,
        "grandchild survived acknowledged abort"
    );
    unsafe {
        CloseHandle(handle);
    }
}

#[test]
fn exact_input_byte_boundary_and_one_byte_over_have_distinct_dispositions() {
    let h = Harness::new("normal");
    assert_eq!(h.load("normal")["ok"], true);
    let base = h.input(h.job("cache", 1, 1), b"");
    let mut event = base.event().clone();
    drop(base);
    event.payload = Vec::new();
    let overhead = retained_event_bytes(&event).unwrap() + COMPLETION_ENTRY_OVERHEAD_BYTES;
    event.payload = vec![0; 4096 - overhead];
    h.tx.try_publish_owned(event.clone()).unwrap();
    let exact = match h.rx.try_take_owned() {
        OwnedPoll::Event(c) => c,
        _ => panic!(),
    };
    assert_eq!(exact.retained_bytes(), 4096);
    assert!(h.adapter.try_offer_retained(exact).is_ok());
    assert_eq!(unpack(&h.take().event().payload)["ok"], false);
    event.payload = vec![0; 4097 - overhead];
    h.tx.try_publish_owned(event).unwrap();
    let excess = match h.rx.try_take_owned() {
        OwnedPoll::Event(c) => c,
        _ => panic!(),
    };
    assert_eq!(excess.retained_bytes(), 4097);
    let pointer = excess.event().payload.as_ptr();
    let before = h.rx.storage_snapshot();
    let excess = match h.adapter.try_offer_retained(excess) {
        Err(RetainedOfferError::Closed(c)) => c,
        other => panic!("{other:?}"),
    };
    assert_eq!(excess.event().payload.as_ptr(), pointer);
    assert_eq!(h.rx.storage_snapshot(), before);
    drop(excess);
    assert_eq!(h.call(h.job("unload", 1, 1), b"")["ok"], true);
}

#[test]
fn missing_return_context_is_refused_before_worker_admission() {
    let h = Harness::new("normal");
    let original = h.input(json!({"op":"load"}), &[]);
    let mut bad = original.event().clone();
    drop(original);
    bad.envelope.return_route = None;
    let pointer = bad.payload.as_ptr();
    h.tx.try_publish_owned(bad).unwrap();
    let OwnedPoll::Event(held) = h.rx.try_take_owned() else {
        panic!();
    };
    let charge = h.rx.storage_snapshot().retained_bytes;
    let before = h.adapter.snapshot();
    let Err(RetainedOfferError::Closed(returned)) = h.adapter.try_offer_retained(held) else {
        panic!("missing context admitted");
    };
    assert_eq!(returned.event().payload.as_ptr(), pointer);
    assert_eq!(h.rx.storage_snapshot().retained_bytes, charge);
    assert_eq!(h.adapter.snapshot(), before);
    assert!(h.adapter.peek_retained_completion().is_none());
    drop(returned);
    assert_eq!(h.rx.storage_snapshot().retained_bytes, 0);
}
