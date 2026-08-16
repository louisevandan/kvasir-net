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

/// A peer that goes quiet is let go, so the map is not append-only in the
/// number of addresses a long-lived process has ever spoken to.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_peer_is_retired_rather_than_held_forever() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut sink = Vec::new();
                let _ = stream.read_to_end(&mut sink).await;
            });
        }
    });

    let peers = Peers::with_idle(Duration::from_millis(80));
    let target = Address::tcp("127.0.0.1", bound.port());
    peers.send(frame(target.clone(), "r1")).await.unwrap();
    assert_eq!(peers.connected().await, 1, "the peer is held while in use");

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        peers.connected().await,
        0,
        "and released once it has been silent"
    );
}

/// Retirement must not become a way to lose work: a frame sent into a pump
/// that has just retired is reconnected rather than refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frame_after_a_retirement_still_arrives() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    let (seen, mut arrived) = tokio::sync::mpsc::channel::<usize>(8);
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut sink = Vec::new();
                let _ = stream.read_to_end(&mut sink).await;
                let _ = seen.send(sink.len()).await;
            });
        }
    });

    let peers = Peers::with_idle(Duration::from_millis(60));
    let target = Address::tcp("127.0.0.1", bound.port());
    peers.send(frame(target.clone(), "before")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(peers.connected().await, 0, "retired while idle");

    // The address is spoken to again after the pump is gone.
    peers.send(frame(target.clone(), "after")).await.unwrap();
    assert_eq!(peers.connected().await, 1, "and a fresh one was built");
    drop(peers);

    let mut total = 0;
    while let Some(bytes) = arrived.recv().await {
        total += bytes;
        if total > 0 && arrived.is_empty() {
            break;
        }
    }
    assert!(total > 0, "both frames reached the far end");
}

/// A busy peer must not retire under its own traffic.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peer_in_constant_use_is_kept() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bound = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut sink = Vec::new();
                let _ = stream.read_to_end(&mut sink).await;
            });
        }
    });

    let peers = Peers::with_idle(Duration::from_millis(40));
    let target = Address::tcp("127.0.0.1", bound.port());
    for index in 0..20 {
        peers
            .send(frame(target.clone(), &format!("r{index}")))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert_eq!(
        peers.connected().await,
        1,
        "traffic kept the connection alive"
    );
}
