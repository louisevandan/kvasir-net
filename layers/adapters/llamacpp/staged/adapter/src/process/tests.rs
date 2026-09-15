use super::*;
use std::env;
use std::net::TcpListener;

#[derive(Default)]
struct Fake {
    ready_after: usize,
    polls: usize,
    crash_on_ready: bool,
    shutdown_error: bool,
}

impl ServerControl for Fake {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn wait_ready(&mut self, _deadline: Instant) -> Result<Option<ReadyInfo>, String> {
        self.polls += 1;
        if self.crash_on_ready {
            return Err("child exited".into());
        }
        if self.polls > self.ready_after {
            Ok(Some(ReadyInfo {
                protocol_revision: 1,
                physical_identity_revision: 1,
                server_id: "fake".into(),
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
                stage_wire_abi: "unknown".into(),
            }))
        } else {
            Ok(None)
        }
    }
    fn shutdown(&mut self) -> Result<(), String> {
        if self.shutdown_error {
            Err("shutdown failed".into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn unload_is_terminal_and_clears_ready_info() {
    let mut process = ServerProcess::new(Fake::default());
    process
        .start_and_wait_ready(Duration::from_millis(10))
        .unwrap();
    process.unload().unwrap();
    assert_eq!(process.state(), ProcessState::Exited);
    assert!(process.state().is_terminal());
    assert!(process.ready_info().is_none());
    assert_eq!(
        process.unload(),
        Err(ProcessError::InvalidState(ProcessState::Exited))
    );
}

#[test]
fn ready_crash_is_terminal() {
    let mut process = ServerProcess::new(Fake {
        crash_on_ready: true,
        ..Fake::default()
    });
    assert!(matches!(
        process.start_and_wait_ready(Duration::from_millis(10)),
        Err(ProcessError::ReadyFailed(_))
    ));
    assert_eq!(process.state(), ProcessState::Crashed);
    assert!(process.state().is_terminal());
}

#[test]
fn ready_timeout_is_terminal() {
    let mut process = ServerProcess::new(Fake {
        ready_after: usize::MAX,
        ..Fake::default()
    });
    assert!(matches!(
        process.start_and_wait_ready(Duration::ZERO),
        Err(ProcessError::ReadyTimeout(_))
    ));
    assert_eq!(process.state(), ProcessState::TimedOut);
    assert!(process.state().is_terminal());
}

#[test]
fn hello_contract_errors_retain_the_actual_server_identity() {
    let mut body = PROTOCOL_REVISION.to_le_bytes().to_vec();
    body.extend_from_slice(b"READY;physical_batch=1");
    let error = core::decode_hello(&body).expect_err("n_ctx is required");
    assert!(error.contains("HELLO omits n_ctx"), "{error}");
    assert!(
        error.contains("server_id=\"READY;physical_batch=1\""),
        "{error}"
    );
    assert!(error.contains("hello_id_bytes=22"), "{error}");
}

#[test]
fn stage_socket_identity_rejects_a_tcp_self_connection() {
    let endpoint: SocketAddr = "127.0.0.1:52104".parse().expect("valid endpoint");
    assert!(core::is_self_connection(endpoint, endpoint));
    assert!(!core::is_self_connection(
        "127.0.0.1:61234".parse().expect("valid local endpoint"),
        endpoint,
    ));
}

fn wait_for_concrete_ready(control: &mut ProcessServerControl, timeout: Duration) -> ReadyInfo {
    let deadline = Instant::now() + timeout;
    loop {
        match control.wait_ready(deadline).expect("hello succeeds") {
            Some(ready) => return ready,
            None if Instant::now() >= deadline => {
                panic!("server did not become ready within {timeout:?}")
            }
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
}

#[test]
fn concrete_process_control_sends_plan_hello_and_unload_then_reaps_child() {
    if env::var_os("STAGED_ADAPTER_CHILD_SERVER").is_some() {
        child_server();
        return;
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test endpoint");
    let endpoint = listener.local_addr().expect("test endpoint address");
    drop(listener);

    let binary = env::current_exe().expect("test executable");
    let mut launch = ServerLaunch::new(binary, endpoint, b"model=test".to_vec());
    launch.args = vec![
        "--exact".into(),
        "process::tests::concrete_process_control_sends_plan_hello_and_unload_then_reaps_child"
            .into(),
    ];
    launch
        .environment
        .push(("STAGED_ADAPTER_CHILD_SERVER".into(), "1".into()));
    launch.environment.push((
        "STAGED_ADAPTER_CHILD_ENDPOINT".into(),
        endpoint.to_string().into(),
    ));
    launch.ready_timeout = Duration::from_secs(5);
    launch.io_timeout = Duration::from_secs(2);
    let mut control = ProcessServerControl::new(launch);
    control.start().expect("child starts");
    let pid = control.pid().expect("child pid");
    let ready = wait_for_concrete_ready(&mut control, Duration::from_secs(5));
    assert_eq!(ready.protocol_revision, PROTOCOL_REVISION);
    assert!(ready.server_id.starts_with("test-child;"));
    assert_eq!((ready.n_ctx, ready.n_batch, ready.n_ubatch), (512, 64, 64));
    assert_eq!(ready.n_seq_max, 1);
    control.shutdown().expect("unload reaps child");
    assert!(control.pid().is_none());

    // A second shutdown is safe through Drop and does not resurrect the
    // process or leave the plan pipe open.
    drop(control);
    assert!(pid > 0);
}

#[test]
fn concrete_process_control_round_trips_all_kv_operations() {
    if env::var_os("STAGED_ADAPTER_CHILD_KV_SERVER").is_some() {
        child_server();
        return;
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test endpoint");
    let endpoint = listener.local_addr().expect("test endpoint address");
    drop(listener);
    let binary = env::current_exe().expect("test executable");
    let mut launch = ServerLaunch::new(binary, endpoint, b"model=test".to_vec());
    launch.args = vec![
        "--exact".into(),
        "process::tests::concrete_process_control_round_trips_all_kv_operations".into(),
    ];
    launch
        .environment
        .push(("STAGED_ADAPTER_CHILD_KV_SERVER".into(), "1".into()));
    launch.environment.push((
        "STAGED_ADAPTER_CHILD_ENDPOINT".into(),
        endpoint.to_string().into(),
    ));
    launch.ready_timeout = Duration::from_secs(5);
    launch.io_timeout = Duration::from_secs(2);
    let mut control = ProcessServerControl::new(launch);
    control.start().expect("child starts");
    wait_for_concrete_ready(&mut control, Duration::from_secs(5));

    for (operation, expected) in [
        (Operation::KvSave, 101u64),
        (Operation::KvRestore, 202u64),
        (Operation::KvDrop, 0u64),
    ] {
        let body = KvPayload {
            sequence_id: "seq-7".into(),
            cache_key: "request-7".into(),
            model_identity: "model.gguf".into(),
            stage_begin: 0,
            stage_end: 4,
            flags: 0,
            expected_checksum: String::new(),
            operation_id: "operation-1".into(),
        }
        .encode(ProtocolLimits::default())
        .expect("KV request encodes");
        let response = control
            .request(Frame::new(operation, body).expect("KV frame"))
            .expect("KV response");
        assert_eq!(response.header.operation, Operation::KvResult);
        let result =
            KvResult::decode(&response.body, ProtocolLimits::default()).expect("KV result decodes");
        assert_eq!(result.sequence_id, "seq-7");
        assert_eq!(result.cache_key, "request-7");
        assert_eq!(result.bytes, expected);
    }
    control.shutdown().expect("unload reaps child");
}

#[test]
fn concrete_process_control_round_trips_multiple_hop_results() {
    if env::var_os("STAGED_ADAPTER_CHILD_HOP_SERVER").is_some() {
        child_server();
        return;
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve test endpoint");
    let endpoint = listener.local_addr().expect("test endpoint address");
    drop(listener);
    let binary = env::current_exe().expect("current test binary");
    let mut launch = ServerLaunch::new(binary, endpoint, b"model=test".to_vec());
    launch.args = vec![
        "--exact".into(),
        "process::tests::concrete_process_control_round_trips_multiple_hop_results".into(),
    ];
    launch
        .environment
        .push(("STAGED_ADAPTER_CHILD_HOP_SERVER".into(), "1".into()));
    launch.environment.push((
        "STAGED_ADAPTER_CHILD_ENDPOINT".into(),
        endpoint.to_string().into(),
    ));
    launch.ready_timeout = Duration::from_secs(5);
    launch.io_timeout = Duration::from_secs(2);
    let mut control = ProcessServerControl::new(launch);
    control.start().expect("child starts");
    wait_for_concrete_ready(&mut control, Duration::from_secs(5));

    let body = crate::HopPayload {
        phase: crate::HopPhase::Prefill,
        sequences: ["seq-a", "seq-b"]
            .into_iter()
            .map(|sequence_id| SequencePayload {
                sequence_id: sequence_id.into(),
                descriptors: Vec::new(),
                payloads: Vec::new(),
                n_tokens: None,
                prompt: None,
                initial_tokens: None,
                options: String::new(),
                position: Some(0),
                outcome: None,
            })
            .collect(),
        legacy: false,
    }
    .encode(ProtocolLimits::default())
    .expect("HOP payload encodes");
    let response = control
        .request(Frame::new(Operation::Hop, body).expect("HOP frame"))
        .expect("HOP response");
    assert_eq!(response.header.operation, Operation::HopResult);
    let result = crate::HopPayload::decode(&response.body, ProtocolLimits::default())
        .expect("HOP_RESULT decodes");
    assert_eq!(
        result
            .sequences
            .iter()
            .map(|sequence| sequence.sequence_id.as_str())
            .collect::<Vec<_>>(),
        vec!["seq-a", "seq-b"]
    );
    control.shutdown().expect("unload reaps child");
}

fn child_server() {
    let endpoint: SocketAddr = env::var("STAGED_ADAPTER_CHILD_ENDPOINT")
        .expect("child endpoint")
        .parse()
        .expect("valid child endpoint");
    let mut stdin = std::io::stdin().lock();
    let mut prefix = [0u8; 4];
    std::io::Read::read_exact(&mut stdin, &mut prefix).expect("plan prefix");
    let length = u32::from_le_bytes(prefix) as usize;
    let mut plan = vec![0u8; length];
    std::io::Read::read_exact(&mut stdin, &mut plan).expect("plan body");
    assert_eq!(plan, b"model=test");

    let listener = TcpListener::bind(endpoint).expect("child binds endpoint");
    let (mut stream, _) = listener.accept().expect("adapter connects");
    let hello = Frame::read_from(&mut stream, ProtocolLimits::default()).expect("HELLO");
    assert_eq!(hello.header.operation, Operation::Hello);
    let response = Frame::new(Operation::Hello, {
        let mut body = PROTOCOL_REVISION.to_le_bytes().to_vec();
        body.extend_from_slice(
            b"test-child;physical_batch=1;equal_sequence_ubatch=0;max_atomic_sequences=1;atomic_batch_exclusive=0;n_ctx=512;n_batch=64;n_ubatch=64;n_seq_max=1;physical_result_payload_bytes=0;physical_result_tensor_count=0;max_physical_result_bytes=33554432",
        );
        body
    })
    .expect("HELLO response");
    response
        .write_to(&mut stream, ProtocolLimits::default())
        .expect("HELLO response write");
    if env::var_os("STAGED_ADAPTER_CHILD_KV_SERVER").is_some() {
        for expected in [Operation::KvSave, Operation::KvRestore, Operation::KvDrop] {
            let request =
                Frame::read_from(&mut stream, ProtocolLimits::default()).expect("KV request");
            assert_eq!(request.header.operation, expected);
            let payload =
                KvPayload::decode(&request.body, ProtocolLimits::default()).expect("KV payload");
            let bytes = match expected {
                Operation::KvSave => 101,
                Operation::KvRestore => 202,
                Operation::KvDrop => 0,
                _ => unreachable!(),
            };
            let result = KvResult {
                sequence_id: payload.sequence_id,
                cache_key: payload.cache_key,
                bytes,
                checksum: "a".repeat(64),
            };
            Frame::new(
                Operation::KvResult,
                result.encode(ProtocolLimits::default()).expect("KV result"),
            )
            .expect("KV response frame")
            .write_to(&mut stream, ProtocolLimits::default())
            .expect("KV response write");
        }
    }
    if env::var_os("STAGED_ADAPTER_CHILD_HOP_SERVER").is_some() {
        let request =
            Frame::read_from(&mut stream, ProtocolLimits::default()).expect("HOP request");
        assert_eq!(request.header.operation, Operation::Hop);
        let payload = crate::HopPayload::decode(&request.body, ProtocolLimits::default())
            .expect("HOP payload");
        assert_eq!(payload.sequences.len(), 2);
        assert_eq!(payload.sequences[0].sequence_id, "seq-a");
        assert_eq!(payload.sequences[1].sequence_id, "seq-b");
        Frame::new(
            Operation::HopResult,
            crate::HopPayload {
                phase: crate::HopPhase::Prefill,
                sequences: payload.sequences,
                legacy: false,
            }
            .encode(ProtocolLimits::default())
            .expect("HOP result"),
        )
        .expect("HOP response frame")
        .write_to(&mut stream, ProtocolLimits::default())
        .expect("HOP response write");
    }
    let unload = Frame::read_from(&mut stream, ProtocolLimits::default()).expect("UNLOAD");
    assert_eq!(unload.header.operation, Operation::Unload);
}
