use super::RuntimeStream;
use crate::domain::state::{Binding, Config};
use p4_protocol::{ExecutionRequest, Message, Phase};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

#[test]
fn multiplexes_independent_requests_over_one_runtime_stream() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = tokio::spawn(mock_runtime(listener, 32));
        let stream = Arc::new(
            RuntimeStream::connect(&format!("http://{endpoint}"))
                .await
                .unwrap(),
        );
        let mut bindings = HashMap::new();
        bindings.insert(
            ("node".into(), "binding".into()),
            Binding {
                deployment_id: "deployment".into(),
                generation: 1,
            },
        );
        let config = Config {
            host: format!("http://{endpoint}"),
            agent_endpoint: "unused".into(),
            adapter_id: "pipeline".into(),
            nodes: Arc::new(RwLock::new(HashSet::new())),
            bindings: Arc::new(RwLock::new(bindings)),
            capacity: Arc::new(crate::domain::capacity::CapacityRegistry::new(16, 256)),
        };
        let mut tasks = Vec::new();
        for index in 0..32 {
            let stream = Arc::clone(&stream);
            let config = config.clone();
            tasks.push(tokio::spawn(async move {
                let (responses, mut response_rx) = mpsc::channel(8);
                stream
                    .execute(execute(index), &config, &responses)
                    .await
                    .unwrap();
                match response_rx.recv().await.unwrap() {
                    Message::Done(done) => done.request_id,
                    other => panic!("unexpected response {other:?}"),
                }
            }));
        }
        for (index, task) in tasks.into_iter().enumerate() {
            assert_eq!(task.await.unwrap(), format!("request-{index}"));
        }
        server.await.unwrap();
    });
}

async fn mock_runtime(listener: TcpListener, count: usize) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0u8; 1];
        socket.read_exact(&mut byte).await.unwrap();
        header.push(byte[0]);
    }
    assert!(
        String::from_utf8(header)
            .unwrap()
            .contains("Upgrade: linker-pipeline-inference-stream-v1")
    );
    socket
        .write_all(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: linker-pipeline-inference-stream-v1\r\nConnection: Upgrade\r\n\r\n",
        )
        .await
        .unwrap();
    let (reader, mut writer) = socket.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut request_ids = Vec::new();
    while request_ids.len() < count {
        let command: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        request_ids.push(command["request_id"].as_str().unwrap().to_owned());
    }
    for request_id in request_ids.into_iter().rev() {
        writer
            .write_all(
                format!(
                    "{}\n",
                    json!({
                        "protocol": "linker-pipeline-inference-stream-v1",
                        "type": "done",
                        "request_id": request_id,
                        "generated_tokens": 1,
                        "reason": "stop"
                    })
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    }
}

fn execute(index: usize) -> ExecutionRequest {
    ExecutionRequest {
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
    }
}
