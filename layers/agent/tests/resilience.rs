//! What arrives that should not, and what a long run does to the bookkeeping.
//!
//! An agent that runs for weeks meets everything: half-open sockets, garbage
//! from a port scanner, a frame truncated by a machine that died mid-write, a
//! reply for a route that finished an hour ago, a peer that reconnects from a
//! new port every time. None of it may end the process, and — the part that
//! only shows up in the run nobody restarts — none of it may leave anything
//! behind.
//!
//! Every test here ends the same way: the agent still serves. Surviving the
//! input is half of it; still being useful afterwards is the claim.

mod common;

use common::{Outer, Silent, chain_over, request, runtime, settle, start, until};
use p4_mock::Mock;
use p4_mock::profile::Profile;
use p4_protocol::frame::{self, Frame};
use p4_protocol::{Envelope, QueueClass, Recipient};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// Sends raw bytes at an agent's listener, as any stranger could.
async fn raw(agent: &Arc<p4_agent_core::agent::Agent>, bytes: &[u8]) {
    let address = agent.address();
    let Ok(mut stream) = TcpStream::connect((address.host.as_str(), address.port)).await else {
        return;
    };
    let _ = stream.write_all(bytes).await;
    let _ = stream.flush().await;
}

/// A frame addressed to a node that does not exist here.
fn for_missing_node(agent: &Arc<p4_agent_core::agent::Agent>) -> Frame {
    Frame {
        envelope: Envelope {
            target: agent.address().clone(),
            recipient: Recipient::node("no-such-node"),
            lane: QueueClass::Prefill,
            route: "stray".into(),
            request_id: "stray".into(),
            stream_id: "stray".into(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: b"execute".to_vec(),
    }
}

/// Garbage, truncation and lies about length, then ordinary work.
#[test]
fn malformed_input_is_refused_without_taking_the_agent_with_it() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        let good = frame::encode(&for_missing_node(&a).envelope, b"execute").unwrap();

        // Nothing here is a frame, and each is wrong in its own way.
        raw(&a, b"").await;
        raw(&a, b"hello").await;
        raw(&a, &[0xff; 64]).await;
        raw(&a, &good[..8]).await; // header cut in half
        raw(&a, &good[..good.len() - 3]).await; // body cut short
        let mut wrong_magic = good.clone();
        wrong_magic[0] ^= 0xff;
        raw(&a, &wrong_magic).await;
        let mut wrong_version = good.clone();
        wrong_version[4] = 0xfe;
        raw(&a, &wrong_version).await;
        let mut impossible = good.clone();
        impossible[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        raw(&a, &impossible).await;
        // A header claiming a huge body, then nothing — the allocation trap.
        let mut greedy = good[..16].to_vec();
        greedy[12..16].copy_from_slice(&(900 * 1024u32).to_le_bytes());
        raw(&a, &greedy).await;

        settle(120).await;

        // Still serving.
        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(request("after", &chain, &outer, 2)).unwrap();
        until(|| outer_duties.frames_for("after").len() >= 3).await;
        assert_eq!(
            outer_duties.frames_for("after").len(),
            3,
            "the agent kept working after all of that"
        );
    });
}

/// Frames that are well formed and still make no sense here.
///
/// A node that was never created, a reply nobody is waiting for. Both are
/// ordinary in a fleet — a stale caller, a node deleted mid-flight — and both
/// must be counted rather than retained.
#[test]
fn frames_with_nowhere_to_go_are_counted_and_dropped() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;

        for _ in 0..200 {
            a.enqueue(for_missing_node(&a)).unwrap();
        }
        settle(150).await;

        let traffic = a.traffic();
        assert!(
            traffic.unrouted + traffic.refused >= 200,
            "each was accounted for: unrouted={}, refused={}",
            traffic.unrouted,
            traffic.refused
        );
        assert_eq!(
            a.continuations().outstanding(),
            0,
            "and none left a reply expected forever"
        );

        let chain = chain_over(&[(&a, "n0")]);
        a.enqueue(request("after", &chain, &outer, 1)).unwrap();
        until(|| !outer_duties.frames_for("after").is_empty()).await;
    });
}

/// Sockets that open and die, over and over.
///
/// The connection ceiling is a semaphore, so a permit that is not returned is
/// a slow strangulation: the agent keeps serving for a while and then stops
/// accepting, hours later, for no visible reason.
#[test]
fn connection_churn_does_not_exhaust_the_listener() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::terminal(0, Profile::default())), 4)
            .await;
        let address = a.address().clone();

        // Far more than the ceiling, opened and abandoned in every way a peer
        // can abandon one.
        for index in 0..600 {
            let Ok(mut stream) = TcpStream::connect((address.host.as_str(), address.port)).await
            else {
                continue;
            };
            match index % 3 {
                0 => drop(stream), // closed at once
                1 => {
                    let _ = stream.write_all(b"\x00").await; // one useless byte
                    drop(stream);
                }
                _ => {
                    let _ = stream.shutdown().await; // half-open then gone
                }
            }
        }
        settle(200).await;

        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..8 {
            a.enqueue(request(&format!("r{index}"), &chain, &outer, 1))
                .unwrap();
        }
        until(|| outer_duties.routes() >= 8).await;
        assert_eq!(
            outer_duties.routes(),
            8,
            "the listener still accepts after 600 dead connections"
        );
    });
}

/// The bookkeeping after a long, mixed run.
///
/// Rounds of ordinary work interleaved with rubbish, checking at the end that
/// what should be empty is empty. A leak here is not a crash; it is a process
/// that dies next week.
#[test]
fn a_long_mixed_run_leaves_nothing_behind() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        let b = start(Arc::new(Silent)).await;
        a.create_node("n0", Arc::new(Mock::staged(0, Profile::default())), 8)
            .await;
        b.create_node("n1", Arc::new(Mock::terminal(1, Profile::default())), 8)
            .await;

        let chain = chain_over(&[(&a, "n0"), (&b, "n1")]);
        let mut sent = 0;
        for round in 0..12 {
            for index in 0..20 {
                a.enqueue(request(&format!("r{round}-{index}"), &chain, &outer, 3))
                    .unwrap();
                sent += 1;
            }
            // Rubbish between every round, so the agent never gets a clean
            // stretch to recover in.
            raw(&a, &[0xab; 40]).await;
            a.enqueue(for_missing_node(&a)).unwrap();
        }
        until(|| outer_duties.total_frames() >= sent * (3 + 1)).await;

        assert_eq!(outer_duties.routes(), sent, "every real request answered");
        assert_eq!(
            a.node_depth("n0").await,
            Some(0),
            "the node is idle, not holding remnants"
        );
        assert_eq!(b.node_depth("n1").await, Some(0), "and so is the far one");
        let lanes = a.queue().depth();
        assert_eq!(
            lanes.control + lanes.prefill + lanes.decode + lanes.response,
            0,
            "the lanes drained"
        );
        assert_eq!(
            a.continuations().outstanding(),
            0,
            "no reply is still expected"
        );

        // Nothing was lost or double-counted along the way.
        for node in a.node_counts().await {
            assert!(
                node.contains("lost=0") && node.contains("orphaned=0"),
                "{node}"
            );
        }
    });
}

/// Peers seen once and never again are not kept forever.
///
/// This is the leak that hides best: a fleet has few addresses, so the map
/// looks bounded, right up until a caller starts arriving from a new port each
/// time.
#[test]
fn peers_that_go_quiet_are_released() {
    runtime().block_on(async {
        let peers = p4_agent_core::transport::outbound::Peers::with_idle(
            p4_protocol::Address::tcp("127.0.0.1", 1),
            Duration::from_millis(60),
        );
        let sink = start(Arc::new(Silent)).await;

        // One address is real; the rest are gone the moment they are used,
        // which is what a restarted caller looks like.
        for port in 0..24u16 {
            let target = if port == 0 {
                sink.address().clone()
            } else {
                p4_protocol::Address::tcp("127.0.0.1", 49_000 + port)
            };
            let _ = peers
                .send(Frame {
                    envelope: Envelope {
                        target,
                        recipient: Recipient::Agent,
                        lane: QueueClass::Control,
                        route: format!("p{port}"),
                        request_id: format!("p{port}"),
                        stream_id: format!("p{port}"),
                        origin_agent: None,
                        return_channel: None,
                        ingress_generation: 0,
                        event_seq: 0,
                        deadline_unix_ms: 0,
                        reply_to: None,
                        chain: None,
                    },
                    body: b"x".to_vec(),
                })
                .await;
        }
        assert!(peers.connected().await > 1, "they were all opened");

        // Polled rather than slept once: a pump still inside a connect to a
        // dead address cannot retire until that connect returns, and how long
        // the operating system takes to refuse is not this layer's to fix. The
        // claim is that it releases, not that it releases within a fixed time.
        for _ in 0..40 {
            settle(100).await;
            if peers.connected().await == 0 {
                break;
            }
        }
        assert_eq!(
            peers.connected().await,
            0,
            "and all released once silent, however many there were"
        );
    });
}

/// An adapter that says when it is finally let go.
///
/// A leaked node is inert — it answers nothing and breaks nothing — so the
/// only way to see it is to ask its adapter whether it was ever dropped.
struct Reports(Arc<std::sync::atomic::AtomicUsize>);

impl Drop for Reports {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl p4_adapter::Adapter for Reports {
    fn distribution(&self) -> p4_adapter::Distribution {
        p4_adapter::Distribution::Internal
    }
    fn start(&self, _: p4_adapter::Work, _: &dyn p4_adapter::EventSink) {}
}

/// Replacing or deleting a node has to release the old one.
///
/// This is the leak a soak found and no unit test could: the node owns its own
/// event sender, so a run loop waiting for that channel to close waits for
/// itself. Every node ever replaced stayed resident with its adapter, its
/// queue and its in-flight map, and a process that re-creates nodes — which is
/// what every deployment does — grew until it died.
#[test]
fn replacing_and_deleting_nodes_releases_them() {
    runtime().block_on(async {
        let a = start(Arc::new(Silent)).await;
        let released = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // The same name, over and over, as a redeploying fleet does.
        for _ in 0..24 {
            a.create_node("n0", Arc::new(Reports(Arc::clone(&released))), 4)
                .await;
            settle(5).await;
        }
        settle(120).await;
        assert!(
            released.load(std::sync::atomic::Ordering::SeqCst) >= 23,
            "every replaced node was released, not just unreferenced: {}",
            released.load(std::sync::atomic::Ordering::SeqCst)
        );

        assert!(a.delete_node("n0").await, "and the last one deletes");
        settle(120).await;
        assert_eq!(
            released.load(std::sync::atomic::Ordering::SeqCst),
            24,
            "including the deleted one"
        );
    });
}

#[test]
fn deleting_a_busy_node_terminalizes_waiting_and_active_requests() {
    runtime().block_on(async {
        let outer_duties = Outer::default();
        let outer = start(Arc::new(outer_duties.clone())).await;
        let a = start(Arc::new(Silent)).await;
        a.create_node(
            "n0",
            Arc::new(Mock::terminal(
                0,
                Profile {
                    leading_hop: Duration::from_millis(100),
                    ..Profile::default()
                },
            )),
            1,
        )
        .await;
        let chain = chain_over(&[(&a, "n0")]);
        for index in 0..8 {
            a.enqueue(request(&format!("delete-{index}"), &chain, &outer, 1))
                .unwrap();
        }

        settle(20).await;
        assert!(a.delete_node("n0").await);
        until(|| {
            (0..8).all(|index| {
                !outer_duties
                    .frames_for(&format!("delete-{index}"))
                    .is_empty()
            })
        })
        .await;
        for index in 0..8 {
            let frames = outer_duties.frames_for(&format!("delete-{index}"));
            assert_eq!(
                frames.len(),
                1,
                "node deletion must be exactly-once: {frames:?}"
            );
            assert_eq!(
                frames[0].body, b"node removed before request completed",
                "every queued or active request receives the teardown terminal"
            );
        }
    });
}
