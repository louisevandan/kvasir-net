use super::*;
use p4_protocol::{ExecutionDone, ExecutionRequest, Phase};
use std::sync::atomic::AtomicUsize;
use tokio::net::TcpListener;

#[test]
fn multiplexes_1024_requests_over_one_peer_connection() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_io()
        .build()
        .unwrap();
    runtime.block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        let accepts = Arc::new(AtomicUsize::new(0));
        let server_accepts = Arc::clone(&accepts);
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            server_accepts.fetch_add(1, Ordering::Relaxed);
            let mut requests = Vec::with_capacity(1024);
            for _ in 0..1024 {
                let routed = read_message(&mut stream).await.unwrap();
                let Message::Execute(request) = routed.message else {
                    panic!("expected Execute");
                };
                requests.push((routed.route_id, request));
            }
            for (route_id, request) in requests.into_iter().rev() {
                let done = Message::Done(ExecutionDone {
                    controller_id: request.controller_id,
                    node_id: request.node_id,
                    request_id: request.request_id,
                    session_id: request.session_id,
                    reason: "stop".into(),
                    generated_tokens: 1,
                });
                stream
                    .write_all(
                        &encode_routed_message(&RoutedMessage {
                            route_id,
                            deadline_unix_ms: 0,
                            message: done,
                        })
                        .unwrap(),
                    )
                    .await
                    .unwrap();
            }
        });

        let pool = PeerMuxPool::default();
        let mut responses = Vec::with_capacity(1024);
        for index in 0..1024 {
            responses.push(
                pool.execute(&endpoint, format!("route-{index}"), 0, execute(index))
                    .await
                    .unwrap(),
            );
        }
        for (index, mut response) in responses.into_iter().enumerate() {
            let Message::Done(done) = response.recv().await.unwrap() else {
                panic!("expected Done");
            };
            assert_eq!(done.request_id, "same-business-id", "route {index}");
        }
        server.await.unwrap();
        assert_eq!(accepts.load(Ordering::Relaxed), 1);
    });
}

fn execute(index: usize) -> Message {
    Message::Execute(ExecutionRequest {
        controller_id: "controller".into(),
        node_id: "node".into(),
        deployment_id: "deployment".into(),
        binding_id: "binding".into(),
        runtime_generation: 1,
        request_id: "same-business-id".into(),
        session_id: format!("session-{index}"),
        phase: Phase::Prefill,
        position: 0,
        max_tokens: 1,
        temperature: 0.0,
        prompt: "test".into(),
        options: "{}".into(),
    })
}
