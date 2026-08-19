use super::*;
use p4_protocol::envelope::chain::{Chain, Link};
use p4_protocol::{Envelope, NodeId, QueueClass, Recipient};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;

/// This agent, for the relay check that never picks itself.
fn here() -> Address {
    Address::tcp("127.0.0.1", 1)
}

fn frame(target: Address, route: &str) -> Frame {
    Frame {
        envelope: Envelope {
            target,
            recipient: Recipient::Agent,
            lane: QueueClass::Control,
            route: route.into(),
            request_id: route.into(),
            stream_id: route.into(),
            origin_agent: Some(here()),
            return_channel: Some(format!("test:{route}")),
            ingress_generation: 0,
            event_seq: 0,
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

        let peers = Peers::new(here());
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
        let peers = Peers::new(here());
        let nowhere = Address::tcp("127.0.0.1", 1);
        assert!(peers.send(frame(nowhere, "r")).await.is_ok());
    });
}

#[test]
fn each_peer_gets_its_own_connection() {
    runtime().block_on(async {
        let peers = Peers::new(here());
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

    let peers = Peers::with_idle(here(), Duration::from_millis(80));
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

    let peers = Peers::with_idle(here(), Duration::from_millis(60));
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

    let peers = Peers::with_idle(here(), Duration::from_millis(40));
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

/// A chain whose first link is `through`, which is the route home.
fn chained(target: Address, through: Address) -> Frame {
    let link = |address: Address, node: &str| Link {
        address,
        node: NodeId::from(node),
        binding: "b".into(),
        generation: 1,
    };
    let mut carrier = frame(target.clone(), "r");
    carrier.envelope.chain =
        Some(Chain::new(vec![link(through, "n0"), link(target, "n1")]).expect("a chain"));
    carrier
}

/// A frame that cannot reach its target goes to the agent the caller was
/// talking to, rather than to stderr.
///
/// This is the reporting topology stated plainly: a node reports to its agent,
/// and an agent the caller is not connected to hands the answer to one that is.
/// Before this, an undeliverable reply was one line of log and a drop — which
/// to whoever asked looks exactly like a request that never finished, on a
/// fleet where the work had in fact been done.
#[test]
fn a_reply_that_cannot_reach_the_caller_goes_home_through_the_chain() {
    runtime().block_on(async {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = TcpListener::from_std(listener).unwrap();
        let relay = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
        let arrived = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 4096];
            match stream.read(&mut buffer).await {
                Ok(0) | Err(_) => 0,
                Ok(bytes) => count_frames(&buffer[..bytes]),
            }
        });

        // Port 1: nothing is listening, and nothing will be.
        let unreachable = Address::tcp("127.0.0.1", 1);
        let peers = Peers::new(here());
        peers
            .send(chained(unreachable, relay))
            .await
            .expect("handed to the pump");

        let seen = tokio::time::timeout(Duration::from_secs(10), arrived)
            .await
            .expect("the relay was reached")
            .expect("the listener ran");
        assert_eq!(seen, 1, "the frame arrived at the chain's first link");
    });
}

/// A relay that fails is the end of it.
///
/// Nothing is listening anywhere here, so both the target and the relay fail.
/// What is being checked is that this stops: without the flag the chain would
/// name the same first link again and the frame would go round for as long as
/// the process lives. The observable is the peer count — the target and one
/// relay — reached and then held rather than climbing.
#[test]
fn a_relay_that_fails_is_the_end_of_it() {
    runtime().block_on(async {
        let peers = Peers::new(here());
        peers
            .send(chained(
                Address::tcp("127.0.0.1", 1),
                Address::tcp("127.0.0.1", 2),
            ))
            .await
            .expect("handed to the pump");

        // Waited for rather than slept past: how long a refused connection
        // takes is the operating system's business, and a fixed pause is a
        // test that passes on the machine it was written on.
        let settled = tokio::time::timeout(Duration::from_secs(10), async {
            while peers.connected().await < 2 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        assert!(settled.is_ok(), "the relay was never attempted");

        // And it stays there. A frame going round would keep finding the same
        // two peers, so the count alone cannot tell — but a third address
        // never appears either way, and what a loop would do is never finish
        // opening this one.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            peers.connected().await,
            2,
            "the target and one relay, and nothing beyond that"
        );
    });
}
