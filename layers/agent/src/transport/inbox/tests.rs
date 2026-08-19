use super::*;
use crate::queue::lane::{Budget, Lanes};
use crate::queue::main::channel;
use p4_protocol::return_channel::with_capability;
use p4_protocol::{Address, Envelope, QueueClass, Recipient};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn frame(route: &str, lane: QueueClass) -> Frame {
    Frame {
        envelope: Envelope {
            target: Address::tcp("127.0.0.1", 19001),
            recipient: Recipient::Agent,
            lane,
            route: route.into(),
            request_id: route.into(),
            stream_id: route.into(),
            origin_agent: None,
            return_channel: None,
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

fn capability_channel(logical: &str) -> String {
    with_capability(logical, &"ab".repeat(32))
}

fn text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn acknowledge_body(channel: &str, stream_id: &str, event_seq: u64) -> Vec<u8> {
    let mut bytes = vec![7u8];
    text(&mut bytes, channel);
    text(&mut bytes, stream_id);
    bytes.extend_from_slice(&event_seq.to_le_bytes());
    bytes
}

#[test]
fn what_arrives_on_a_socket_lands_on_the_queue_unchanged() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        tokio::spawn(serve(listener, sender, 16));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let sent = frame("r1", QueueClass::Prefill);
        let bytes = frame::encode(&sent.envelope, &sent.body).unwrap();
        client.write_all(&bytes).await.unwrap();

        let arrived = tokio::time::timeout(Duration::from_secs(2), receiver.take())
            .await
            .expect("a frame arrived")
            .expect("the queue is open");
        assert_eq!(arrived, sent);
    });
}

#[test]
fn several_frames_on_one_connection_all_arrive() {
    // A connection is multiplexed: many routes share it, and the reader must
    // never be the thing that serialises them.
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        tokio::spawn(serve(listener, sender, 16));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut bytes = Vec::new();
        for index in 0..8 {
            let one = frame(&format!("r{index}"), QueueClass::Control);
            bytes.extend_from_slice(&frame::encode(&one.envelope, &one.body).unwrap());
        }
        client.write_all(&bytes).await.unwrap();

        for index in 0..8 {
            let arrived = tokio::time::timeout(Duration::from_secs(2), receiver.take())
                .await
                .expect("a frame arrived")
                .unwrap();
            assert_eq!(arrived.envelope.route, format!("r{index}"));
        }
    });
}

#[test]
fn a_frame_of_a_foreign_version_closes_the_connection_rather_than_being_read() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        tokio::spawn(serve(listener, sender, 16));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let one = frame("r1", QueueClass::Control);
        let mut bytes = frame::encode(&one.envelope, &one.body).unwrap();
        bytes[4] = 5;
        client.write_all(&bytes).await.unwrap();

        // Nothing reaches the queue, and the reader gives up on the connection
        // instead of trying to interpret the rest of the stream.
        let nothing = tokio::time::timeout(Duration::from_millis(200), receiver.take()).await;
        assert!(nothing.is_err());
    });
}

#[test]
fn a_return_channel_rebinds_and_drains_its_bounded_pending_frames() {
    runtime().block_on(async {
        let subscriptions = Subscriptions::default();
        let (generation, mut first, _) = subscriptions.bind("outer-a").await;
        assert!(
            subscriptions
                .deliver("outer-a", frame("one", QueueClass::Response))
                .await
        );
        assert_eq!(first.recv().await.unwrap().envelope.route, "one");

        subscriptions.unbind("outer-a", generation).await;
        assert!(
            subscriptions
                .deliver("outer-a", frame("two", QueueClass::Response))
                .await
        );
        let (_, mut second, pending) = subscriptions.bind("outer-a").await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].envelope.route, "two");
        assert!(second.try_recv().is_err());
    });
}

#[test]
fn an_unacknowledged_delivery_replays_until_the_outer_acknowledges_it() {
    runtime().block_on(async {
        let subscriptions = Subscriptions::default();
        let (generation, mut first, _) = subscriptions.bind("outer-a").await;
        let mut sent = frame("reply", QueueClass::Response);
        sent.envelope.event_seq = 7;
        assert!(subscriptions.deliver("outer-a", sent.clone()).await);
        let delivered = first.recv().await.unwrap();
        subscriptions
            .record_delivered("outer-a", generation, &delivered)
            .await;
        subscriptions.unbind("outer-a", generation).await;

        let (rebound_generation, _, replay) = subscriptions.bind("outer-a").await;
        assert_eq!(replay, vec![sent.clone()]);
        assert!(
            !subscriptions
                .acknowledge("outer-a", generation, "reply", 7)
                .await
        );
        assert!(
            subscriptions
                .acknowledge("outer-a", rebound_generation, "reply", 7)
                .await
        );
        subscriptions.unbind("outer-a", rebound_generation).await;
        let (_, _, replay_after_ack) = subscriptions.bind("outer-a").await;
        assert!(replay_after_ack.is_empty());
    });
}

#[test]
fn a_journal_replays_unacked_frames_after_subscription_restart() {
    runtime().block_on(async {
        let root =
            std::env::temp_dir().join(format!("p4-subscription-journal-{}", std::process::id()));
        let channel = capability_channel("journal");
        let first = Subscriptions::with_journal(&root);
        let (_generation, _receiver, _) = first.bind(&channel).await;
        let mut delivered = frame("journal-route", QueueClass::Response);
        delivered.envelope.stream_id = "journal-stream".into();
        delivered.envelope.event_seq = 7;
        assert!(first.deliver(&channel, delivered.clone()).await);
        drop(first);

        let restarted = Subscriptions::with_journal(&root);
        let (rebound_generation, _receiver, replay) = restarted.bind(&channel).await;
        assert_eq!(replay, vec![delivered]);
        assert!(
            restarted
                .acknowledge(&channel, rebound_generation, "journal-stream", 7)
                .await
        );
        drop(restarted);

        let recovered = Subscriptions::with_journal(&root);
        let (_, _receiver, replay) = recovered.bind(&channel).await;
        assert!(replay.is_empty(), "acknowledged frames must not replay");
        let _ = std::fs::remove_dir_all(root);
    });
}

#[test]
fn a_corrupt_journal_refuses_the_channel_instead_of_starting_empty() {
    runtime().block_on(async {
        let root =
            std::env::temp_dir().join(format!("p4-subscription-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let channel = capability_channel("corrupt");
        std::fs::write(journal_path(&root, &channel), b"not-a-journal").unwrap();

        let subscriptions = Subscriptions::with_journal(&root);
        let (generation, _receiver, replay) = subscriptions.bind(&channel).await;
        assert_eq!(generation, 0);
        assert!(replay.is_empty());
        let _ = std::fs::remove_dir_all(root);
    });
}

#[test]
fn unknown_return_channels_are_not_claimed_by_the_registry() {
    runtime().block_on(async {
        let subscriptions = Subscriptions::default();
        assert!(
            !subscriptions
                .deliver("never-registered", frame("one", QueueClass::Response))
                .await
        );
    });
}

#[test]
fn failed_journal_replacement_keeps_the_previous_snapshot() {
    runtime().block_on(async {
        let root = std::env::temp_dir().join(format!(
            "p4-subscription-replace-failure-{}",
            std::process::id()
        ));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let destination = root.join("channel.p4sub");
        let missing_temp = root.join("missing.p4sub.tmp");
        tokio::fs::write(&destination, b"previous").await.unwrap();

        assert!(super::replace_journal_file(&missing_temp, &destination).is_err());
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"previous");
        let _ = tokio::fs::remove_dir_all(root).await;
    });
}

#[test]
fn subscription_slots_have_a_global_bound() {
    runtime().block_on(async {
        let subscriptions = Subscriptions::default();
        for index in 0..MAX_SUBSCRIPTION_SLOTS {
            let (generation, _receiver, _) = subscriptions.bind(&format!("slot-{index}")).await;
            assert_eq!(generation, 1);
        }
        let (generation, _receiver, replay) = subscriptions.bind("slot-overflow").await;
        assert_eq!(generation, 0);
        assert!(replay.is_empty());
    });
}

#[test]
fn a_bound_return_channel_writes_a_reply_back_on_the_same_socket() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let subscriptions = Subscriptions::default();
        tokio::spawn(serve_with_subscriptions(
            listener,
            sender,
            16,
            subscriptions.clone(),
        ));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut request = frame("request", QueueClass::Prefill);
        let channel = capability_channel("outer-a");
        request.envelope.return_channel = Some(channel.clone());
        client
            .write_all(&frame::encode(&request.envelope, &request.body).unwrap())
            .await
            .unwrap();
        receiver.take().await.unwrap();

        let mut reply = frame("reply", QueueClass::Response);
        reply.envelope.return_channel = Some(channel.clone());
        assert!(subscriptions.deliver(&channel, reply.clone()).await);
        let mut header = [0u8; HEADER_BYTES];
        tokio::time::timeout(Duration::from_secs(2), client.read_exact(&mut header))
            .await
            .unwrap()
            .unwrap();
        let total = frame::frame_len(&header).unwrap();
        let mut bytes = Vec::from(header);
        bytes.resize(total, 0);
        client.read_exact(&mut bytes[HEADER_BYTES..]).await.unwrap();
        assert_eq!(frame::decode(&bytes).unwrap(), reply);
    });
}

#[test]
fn a_return_channel_without_a_capability_never_binds_a_socket() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let subscriptions = Subscriptions::default();
        tokio::spawn(serve_with_subscriptions(
            listener,
            sender,
            16,
            subscriptions.clone(),
        ));

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut request = frame("request", QueueClass::Prefill);
        request.envelope.return_channel = Some("outer-a".into());
        client
            .write_all(&frame::encode(&request.envelope, &request.body).unwrap())
            .await
            .unwrap();
        receiver.take().await.unwrap();

        let reply = frame("reply", QueueClass::Response);
        assert!(!subscriptions.deliver("outer-a", reply).await);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), client.readable())
                .await
                .is_err()
        );
    });
}

#[test]
fn the_tcp_reader_stamps_generation_and_fences_a_stale_socket_ack() {
    runtime().block_on(async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, mut receiver, _) = channel(Lanes::default(), Budget::default());
        let subscriptions = Subscriptions::default();
        tokio::spawn(serve_with_subscriptions(
            listener,
            sender,
            16,
            subscriptions.clone(),
        ));

        let channel = capability_channel("outer-a");
        let mut first = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut bind = frame("bind-1", QueueClass::Prefill);
        bind.envelope.return_channel = Some(channel.clone());
        first
            .write_all(&frame::encode(&bind.envelope, &bind.body).unwrap())
            .await
            .unwrap();
        receiver.take().await.unwrap();

        let mut second = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut rebound = frame("bind-2", QueueClass::Prefill);
        rebound.envelope.return_channel = Some(channel.clone());
        second
            .write_all(&frame::encode(&rebound.envelope, &rebound.body).unwrap())
            .await
            .unwrap();
        receiver.take().await.unwrap();

        // The old reader may observe its writer task closing on either of
        // these frames. It must retire the channel locally, never bind a
        // third generation for the old socket.
        for route in ["old-reclaim-1", "old-reclaim-2"] {
            let mut reclaim = frame(route, QueueClass::Control);
            reclaim.envelope.return_channel = Some(channel.clone());
            first
                .write_all(&frame::encode(&reclaim.envelope, &reclaim.body).unwrap())
                .await
                .unwrap();
            let arrived = receiver.take().await.unwrap();
            assert_eq!(arrived.envelope.route, route);
        }
        assert_eq!(subscriptions.generation(&channel).await, Some(2));

        let mut probe = frame("probe", QueueClass::Control);
        probe.envelope.return_channel = Some(channel.clone());
        second
            .write_all(&frame::encode(&probe.envelope, &probe.body).unwrap())
            .await
            .unwrap();
        let probe = receiver.take().await.unwrap();
        let journal_generation = probe.envelope.ingress_generation;

        let mut delivered = frame("reply", QueueClass::Response);
        delivered.envelope.return_channel = Some(channel.clone());
        delivered.envelope.stream_id = "stream".into();
        delivered.envelope.event_seq = 7;
        subscriptions
            .record_delivered(&channel, journal_generation, &delivered)
            .await;
        assert_eq!(subscriptions.metrics().await.unacked, 1);
        assert_eq!(
            subscriptions.generation(&channel).await,
            Some(journal_generation)
        );

        let mut stale_ack = frame("ack-1", QueueClass::Control);
        stale_ack.envelope.return_channel = Some(channel.clone());
        stale_ack.body = acknowledge_body(&channel, "stream", 7);
        first
            .write_all(&frame::encode(&stale_ack.envelope, &stale_ack.body).unwrap())
            .await
            .unwrap();
        let stale = receiver.take().await.unwrap();
        assert_ne!(stale.envelope.ingress_generation, journal_generation);
        assert!(
            !subscriptions
                .acknowledge(&channel, stale.envelope.ingress_generation, "stream", 7,)
                .await
        );
        assert_eq!(subscriptions.metrics().await.unacked, 1);

        let mut current_ack = frame("ack-2", QueueClass::Control);
        current_ack.envelope.return_channel = Some(channel.clone());
        current_ack.body = acknowledge_body(&channel, "stream", 7);
        second
            .write_all(&frame::encode(&current_ack.envelope, &current_ack.body).unwrap())
            .await
            .unwrap();
        let current = receiver.take().await.unwrap();
        assert_eq!(current.envelope.ingress_generation, journal_generation);
        assert!(
            subscriptions
                .acknowledge(&channel, current.envelope.ingress_generation, "stream", 7,)
                .await,
            "current generation {} journal generation {}",
            current.envelope.ingress_generation,
            journal_generation
        );
    });
}
