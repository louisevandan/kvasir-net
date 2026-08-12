use super::*;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener};

#[test]
fn reads_sse_lines_split_across_http_chunks() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/event-stream\r\n\r\n5\r\ndata:\r\n8\r\n hello\n\n\r\n0\r\n\r\n",
                )
                .unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        let mut remainder = Vec::new();
        let _ = stream.read_to_end(&mut remainder);
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut response = open_sse(
            &format!("http://{endpoint}"),
            "/chat",
            &serde_json::json!({"stream": true}),
        )
        .await
        .unwrap();
        assert_eq!(
            response.next_data().await.unwrap().as_deref(),
            Some("hello")
        );
        assert_eq!(response.next_data().await.unwrap(), None);
    });
    server.join().unwrap();
}
