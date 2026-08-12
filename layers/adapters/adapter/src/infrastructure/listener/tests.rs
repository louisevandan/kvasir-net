use super::*;
use crate::domain::state::Binding;
use p4_protocol::{ExecutionRequest, Phase};
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener as TokioTcpListener;

#[test]
fn admission_limit_has_a_safe_default() {
    assert_eq!(configured_limit("P4_PIPELINE_TEST_MISSING", 4), 4);
    assert_eq!(configured_nonnegative_limit("P4_PIPELINE_TEST_MISSING", 0), 0);
}

#[test]
fn production_runtime_enables_batch_timer() {
    let runtime = execution_runtime().unwrap();
    runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_millis(50),
            tokio::time::sleep(Duration::from_millis(1)),
        )
        .await
        .unwrap();
    });
}

#[test]
fn coalesces_only_when_an_explicit_window_is_configured() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let backend = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let backend_endpoint = backend.local_addr().unwrap().to_string();
        let backend_task = tokio::spawn(mock_backend(
            backend,
            Arc::clone(&active),
            Arc::clone(&peak),
        ));

        let mut bindings = HashMap::new();
        bindings.insert(
            ("node".into(), "binding".into()),
            Binding {
                deployment_id: "deployment".into(),
                generation: 1,
            },
        );
        let config = Config {
            host: backend_endpoint,
            agent_endpoint: "unused".into(),
            adapter_id: "pipeline".into(),
            nodes: Arc::new(RwLock::new(HashSet::new())),
            bindings: Arc::new(RwLock::new(bindings)),
            capacity: Arc::new(crate::domain::capacity::CapacityRegistry::new(64, 256)),
        };
        let pipeline = TokioTcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(pipeline.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = pipeline.accept().await.unwrap();
        let relay = tokio::spawn(dispatch(
            server,
            config,
            Arc::new(Semaphore::new(64)),
            1,
            5,
        ));
        for index in 0..64 {
            let message = execute(index);
            client
                .write_all(
                    &encode_routed_message(&RoutedMessage {
                        route_id: format!("route-{index}"),
                        deadline_unix_ms: 0,
                        message,
                    })
                    .unwrap(),
                )
                .await
                .unwrap();
        }
        let mut completed = 0;
        while completed < 64 {
            match read_message(&mut client).await.unwrap().message {
                Message::Done(_) => completed += 1,
                Message::Token(_) => {}
                other => panic!("unexpected response {other:?}"),
            }
        }
        client.shutdown().await.unwrap();
        relay.await.unwrap().unwrap();
        backend_task.await.unwrap();
        assert_eq!(peak.load(Ordering::Relaxed), 1);
    });
}

async fn mock_backend(
    listener: TokioTcpListener,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
    peak.fetch_max(now, Ordering::SeqCst);
    let mut body = String::new();
    for index in 0..64 {
        body.push_str(&format!(
            "data: {{\"request_id\":\"route-{index}\",\"data\":{{\"choices\":[{{\"delta\":{{\"content\":\"x\"}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"completion_tokens\":1}}}}}}\n\n"
        ));
    }
    body.push_str("data: [DONE]\n\n");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).await.unwrap();
    let _ = stream.shutdown().await;
    active.fetch_sub(1, Ordering::SeqCst);
    let mut request = Vec::new();
    let _ = stream.read_to_end(&mut request).await;
}

fn execute(index: usize) -> Message {
    Message::Execute(ExecutionRequest {
        controller_id: "controller".into(),
        node_id: "node".into(),
        deployment_id: "deployment".into(),
        binding_id: "binding".into(),
        runtime_generation: 1,
        request_id: format!("request-{index}"),
        session_id: format!("session-{index}"),
        phase: Phase::Prefill,
        position: 0,
        max_tokens: 1,
        temperature: 0.0,
        prompt: "test".into(),
        options: "{}".into(),
    })
}
