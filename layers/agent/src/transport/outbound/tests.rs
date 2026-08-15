use super::*;
use p4_protocol::{Envelope, QueueClass, Recipient};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

fn frame(target: Address, route: &str) -> Frame {
    Frame {
        envelope: Envelope {
            target,
            recipient: Recipient::Agent,
            lane: QueueClass::Control,
            route: route.into(),
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: b"body".to_vec(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn frames_for_one_peer_share_a_single_connection() {
    // The previous transport opened a socket per call, so a chain paid a
    // connect and a teardown per hop.
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut seen = 0;
            let mut buffer = [0u8; 4096];
            while seen < 3 {
                match stream.read(&mut buffer).await {
                    Ok(0) | Err(_) => break,
                    Ok(bytes) => {
                        seen += count_frames(&buffer[..bytes]);
                    }
                }
            }
            seen
        });

        let peers = Peers::default();
        let target = Address::tcp("127.0.0.1", port);
        for index in 0..3 {
            peers
                .send(frame(target.clone(), &format!("r{index}")))
                .await
                .unwrap();
        }

        // One accept served all three, so only one connection was opened.
        assert_eq!(accepted.await.unwrap(), 3);
        assert_eq!(peers.connected().await, 1);
    });
}

#[test]
fn a_peer_that_never_answers_does_not_block_the_caller() {
    // Sending must return whether or not anyone is there; the deadline is what
    // answers for an unreachable machine, not a stalled worker.
    runtime().block_on(async {
        let peers = Peers::default();
        let nowhere = Address::tcp("127.0.0.1", 1);
        assert!(peers.send(frame(nowhere, "r")).await.is_ok());
    });
}

#[test]
fn each_peer_gets_its_own_connection() {
    runtime().block_on(async {
        let peers = Peers::default();
        for port in [1u16, 2, 3] {
            peers
                .send(frame(Address::tcp("127.0.0.1", port), "r"))
                .await
                .unwrap();
        }
        assert_eq!(peers.connected().await, 3);
    });
}

fn count_frames(mut bytes: &[u8]) -> usize {
    let mut seen = 0;
    while bytes.len() >= 16 {
        let Ok(total) = frame::frame_len(bytes) else {
            break;
        };
        if bytes.len() < total {
            break;
        }
        seen += 1;
        bytes = &bytes[total..];
    }
    seen
}
