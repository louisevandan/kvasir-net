use super::*;
use p4_adapter::node_adapter::{OwnedPoll, completion_mailbox_with_limits};
use std::pin::Pin;
use std::task::{Context, Poll};

struct BrokenWrite {
    bytes_left: usize,
}
impl AsyncWrite for BrokenWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.bytes_left == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected after frame prefix",
            )));
        }
        let count = self.bytes_left.min(buf.len());
        self.bytes_left -= count;
        Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn acknowledged_tcp_ingress_commits_once_and_ack_releases_pinned_receipt() {
    use p4_protocol::event::hop::{self, HopFrame, ReceiptStatus};
    let shared = Shared::with_identity(
        "agent-receiver".into(),
        RuntimeLimits {
            queue: 4,
            retained: 8,
            bytes: 1024 * 1024,
            connections: 2,
            hop_receipts: 2,
            hop_receipt_bytes: 1024 * 1024,
            hop_outstanding: 2,
        },
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, incoming) = shared.limits.mailbox();
    let (outer, _outer) = shared.limits.mailbox();
    let (outbound, _outbound) = shared.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(
        own.clone(),
        agent,
        outer,
        outbound,
        32,
    ));
    let mut client = TcpStream::connect((own.host.as_str(), own.port))
        .await
        .unwrap();
    let (accepted, _) = listener.accept().await.unwrap();
    let slot = shared.slots.clone().acquire_owned().await.unwrap();
    let task = tokio::spawn(serve(201, accepted, broker, shared.clone(), Arc::new(slot)));
    let hello = hop::encode(&HopFrame::Hello {
        sender_id: "outer-a".into(),
        connection_generation: 7,
        max_outstanding: 2,
        max_receipt_bytes: 1024 * 1024,
    })
    .unwrap();
    write_bytes_frame(&mut client, &hello).await.unwrap();
    assert!(matches!(
        hop::decode(&read_body(&mut client).await.unwrap().unwrap()).unwrap(),
        Some(HopFrame::HelloAck {
            accepted_connection_generation: 7,
            ..
        })
    ));
    let event = super::super::tests::event(
        &own,
        Endpoint::agent(own.clone()),
        1,
        "application/octet-stream",
        b"opaque",
    );
    let bytes = encode(&event).unwrap();
    let digest = hop::event_digest(&bytes);
    write_bytes_frame(
        &mut client,
        &hop::encode(&HopFrame::Data {
            attempt: 1,
            digest,
            event: bytes,
        })
        .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(next(&incoming).await.unwrap().event(), &event);
    let receipt = hop::decode(&read_body(&mut client).await.unwrap().unwrap())
        .unwrap()
        .unwrap();
    assert!(matches!(
        receipt,
        HopFrame::Receipt {
            attempt: 1,
            status: ReceiptStatus::AcceptedExact,
            ..
        }
    ));
    assert_eq!(shared.receipts.lock().unwrap().snapshot().accepted, 1);
    write_bytes_frame(
        &mut client,
        &hop::encode(&HopFrame::ReceiptAck {
            sender_id: "outer-a".into(),
            connection_generation: 7,
            attempt: 1,
            digest,
        })
        .unwrap(),
    )
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while shared.receipts.lock().unwrap().snapshot().records != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    client.write_u32_le(0).await.unwrap();
    assert_eq!(client.read_u32_le().await.unwrap(), 0);
    task.await.unwrap();
}

#[tokio::test]
async fn full_receipt_store_rejects_before_broker_commit() {
    use p4_protocol::event::hop::{self, HopFrame, ReceiptStatus};
    let limits = RuntimeLimits {
        queue: 2,
        retained: 4,
        bytes: 1024 * 1024,
        connections: 2,
        hop_receipts: 1,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 1,
    };
    let shared = Shared::with_identity("receiver".into(), limits);
    let occupied = super::hop::ReceiptKey {
        sender_id: "other".into(),
        connection_generation: 1,
        attempt: 1,
    };
    let occupied_digest = hop::event_digest(b"occupied");
    shared
        .receipts
        .lock()
        .unwrap()
        .reserve(occupied, occupied_digest, "occupied")
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, incoming) = shared.limits.mailbox();
    let (outer, _outer) = shared.limits.mailbox();
    let (outbound, _outbound) = shared.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(
        own.clone(),
        agent,
        outer,
        outbound,
        32,
    ));
    let before = broker.receipt_snapshot().unwrap();
    let mut client = TcpStream::connect((own.host.as_str(), own.port))
        .await
        .unwrap();
    let (accepted, _) = listener.accept().await.unwrap();
    let slot = shared.slots.clone().acquire_owned().await.unwrap();
    let task = tokio::spawn(serve(
        202,
        accepted,
        broker.clone(),
        shared.clone(),
        Arc::new(slot),
    ));
    write_bytes_frame(
        &mut client,
        &hop::encode(&HopFrame::Hello {
            sender_id: "outer".into(),
            connection_generation: 2,
            max_outstanding: 1,
            max_receipt_bytes: 1024,
        })
        .unwrap(),
    )
    .await
    .unwrap();
    let _ = read_body(&mut client).await.unwrap();
    let event = super::super::tests::event(
        &own,
        Endpoint::agent(own.clone()),
        1,
        "application/octet-stream",
        b"must-not-commit",
    );
    let bytes = encode(&event).unwrap();
    let digest = hop::event_digest(&bytes);
    write_bytes_frame(
        &mut client,
        &hop::encode(&HopFrame::Data {
            attempt: 1,
            digest,
            event: bytes,
        })
        .unwrap(),
    )
    .await
    .unwrap();
    let receipt = hop::decode(&read_body(&mut client).await.unwrap().unwrap())
        .unwrap()
        .unwrap();
    assert!(matches!(
        receipt,
        HopFrame::Receipt {
            status: ReceiptStatus::Rejected,
            ..
        }
    ));
    assert!(matches!(incoming.try_take_owned(), OwnedPoll::Empty));
    assert_eq!(broker.receipt_snapshot().unwrap(), before);
    client.write_u32_le(0).await.unwrap();
    assert_eq!(client.read_u32_le().await.unwrap(), 0);
    task.await.unwrap();
}

#[tokio::test]
async fn lost_inbound_receipt_keeps_store_authority_without_leaking_connection_slot() {
    use p4_protocol::event::hop::{HopFrame, ReceiptStatus, event_digest};
    let limits = RuntimeLimits {
        queue: 1,
        retained: 2,
        bytes: 1024 * 1024,
        connections: 1,
        hop_receipts: 2,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 1,
    };
    let shared = Shared::with_identity("receiver".into(), limits);
    let (writer, mut peer) = tokio::io::duplex(4096);
    let slot = shared.slots.clone().acquire_owned().await.unwrap();
    let sender = shared.hop_writer(writer, Arc::new(slot), None, 1, 1, None);
    let digest = event_digest(b"accepted-event");
    let key = super::hop::ReceiptKey {
        sender_id: "sender".into(),
        connection_generation: 7,
        attempt: 1,
    };
    shared
        .receipts
        .lock()
        .unwrap()
        .reserve(key.clone(), digest, "event-1")
        .unwrap();
    shared
        .receipts
        .lock()
        .unwrap()
        .commit(&key, digest, ReceiptStatus::AcceptedExact, "")
        .unwrap();
    sender
        .control(HopFrame::Receipt {
            attempt: 1,
            digest,
            status: ReceiptStatus::AcceptedExact,
            detail: String::new(),
        })
        .await
        .unwrap();
    let body = read_body(&mut peer).await.unwrap().unwrap();
    assert!(matches!(
        p4_protocol::event::hop::decode(&body).unwrap(),
        Some(HopFrame::Receipt {
            attempt: 1,
            status: ReceiptStatus::AcceptedExact,
            ..
        })
    ));

    sender.peer_closed("injected receipt loss").await;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while shared.slots.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        shared.failures.lock().unwrap().is_empty(),
        "receipt state belongs to the bounded store, not a dead response socket"
    );
    let receipts = shared.receipts.lock().unwrap().snapshot();
    assert_eq!((receipts.records, receipts.accepted), (1, 1));
}

#[tokio::test]
async fn explicit_reconcile_queries_exact_receipt_before_resuming_original_queue() {
    use p4_protocol::event::hop::{ReceiptStatus, event_digest};
    let limits = RuntimeLimits {
        queue: 4,
        retained: 8,
        bytes: 1024 * 1024,
        connections: 8,
        hop_receipts: 8,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 2,
    };
    let remote = Shared::with_identity("agent-remote".into(), limits);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_address = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, incoming) = remote.limits.mailbox();
    let (outer, _outer) = remote.limits.mailbox();
    let (outbound, _outbound) = remote.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(
        remote_address.clone(),
        agent,
        outer,
        outbound,
        32,
    ));
    remote.spawn(accept(listener, broker, remote.clone()));

    let local = Shared::with_identity("agent-local".into(), limits);
    let (publisher, store) = completion_mailbox_with_limits(2, 4, 1024 * 1024).unwrap();
    let first = super::super::tests::event(
        &remote_address,
        Endpoint::agent(remote_address.clone()),
        1,
        "application/octet-stream",
        b"accepted-before-disconnect",
    );
    let second = super::super::tests::event(
        &remote_address,
        Endpoint::agent(remote_address.clone()),
        2,
        "application/octet-stream",
        b"must-not-overtake",
    );
    let first_digest = event_digest(&encode(&first).unwrap());
    let receipt_key = hop::ReceiptKey {
        sender_id: "agent-local".into(),
        connection_generation: 7,
        attempt: 1,
    };
    remote
        .receipts
        .lock()
        .unwrap()
        .reserve(receipt_key.clone(), first_digest, &first.envelope.event_id)
        .unwrap();
    remote
        .receipts
        .lock()
        .unwrap()
        .commit(&receipt_key, first_digest, ReceiptStatus::AcceptedExact, "")
        .unwrap();
    publisher.try_publish_owned(first).unwrap();
    let first_owned = next(&store).await.unwrap();
    publisher.try_publish_owned(second.clone()).unwrap();
    let second_owned = next(&store).await.unwrap();
    let slot = local.slots.clone().acquire_owned().await.unwrap();
    local.preserve(Failure::HopWriter {
        value: HopWriteFailure {
            error: io::Error::new(io::ErrorKind::ConnectionReset, "injected receipt loss"),
            state: HopFailureState::Uncertain,
            current: None,
            outstanding: VecDeque::from([HopOutstanding {
                attempt: 1,
                digest: first_digest,
                event: first_owned,
                live: None,
            }]),
            pending_acks: VecDeque::new(),
            pending: VecDeque::from([second_owned]),
        },
        target: Some(remote_address),
        generation: 7,
        _slot: Arc::new(slot),
    });

    let result = Inspector(local.clone()).reconcile("transport-1").await;
    assert_eq!(result["ok"], true);
    assert_eq!(result["state"], "accepted_exact");
    let delivered = tokio::time::timeout(std::time::Duration::from_secs(2), next(&incoming))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivered.event(), &second);
    drop(delivered);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while store.storage_snapshot().retained_count != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(local.failures.lock().unwrap().is_empty());
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while remote.receipts.lock().unwrap().snapshot().records != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    local.tasks.lock().unwrap().abort_all();
    remote.tasks.lock().unwrap().abort_all();
}

#[tokio::test]
async fn absent_receipt_quarantines_original_without_replay_or_retirement() {
    use p4_protocol::event::hop::event_digest;
    let limits = RuntimeLimits {
        queue: 2,
        retained: 4,
        bytes: 1024 * 1024,
        connections: 4,
        hop_receipts: 4,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 2,
    };
    let remote = Shared::with_identity("agent-remote".into(), limits);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote_address = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, incoming) = remote.limits.mailbox();
    let (outer, _outer) = remote.limits.mailbox();
    let (outbound, _outbound) = remote.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(
        remote_address.clone(),
        agent,
        outer,
        outbound,
        32,
    ));
    remote.spawn(accept(listener, broker, remote.clone()));
    let local = Shared::with_identity("agent-local".into(), limits);
    let (publisher, store) = completion_mailbox_with_limits(1, 2, 1024 * 1024).unwrap();
    let event = super::super::tests::event(
        &remote_address,
        Endpoint::agent(remote_address.clone()),
        1,
        "application/octet-stream",
        b"uncertain",
    );
    let digest = event_digest(&encode(&event).unwrap());
    publisher.try_publish_owned(event).unwrap();
    let owned = next(&store).await.unwrap();
    let slot = local.slots.clone().acquire_owned().await.unwrap();
    local.preserve(Failure::HopWriter {
        value: HopWriteFailure {
            error: io::Error::new(io::ErrorKind::ConnectionReset, "injected"),
            state: HopFailureState::Uncertain,
            current: Some(HopOutstanding {
                attempt: 1,
                digest,
                event: owned,
                live: None,
            }),
            outstanding: VecDeque::new(),
            pending_acks: VecDeque::new(),
            pending: VecDeque::new(),
        },
        target: Some(remote_address),
        generation: 7,
        _slot: Arc::new(slot),
    });
    let result = Inspector(local.clone()).reconcile("transport-1").await;
    assert_eq!(result["ok"], false);
    assert_eq!(result["state"], "unknown");
    assert_eq!(local.failures.lock().unwrap().len(), 1);
    assert_eq!(store.storage_snapshot().retained_count, 1);
    assert!(matches!(
        incoming.try_take_owned(),
        p4_adapter::node_adapter::OwnedPoll::Empty
    ));
    local.tasks.lock().unwrap().abort_all();
    remote.tasks.lock().unwrap().abort_all();
}

#[tokio::test]
async fn owned_runtime_tcp_ingress_full_keeps_original_and_waits_for_retirement() {
    let shared = Shared::new(RuntimeLimits {
        queue: 1,
        retained: 1,
        bytes: 1024 * 1024,
        connections: 4,
        hop_receipts: 4,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 2,
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, store) = shared.limits.mailbox();
    let (outer, _outer) = shared.limits.mailbox();
    let (outbound, _outbound) = shared.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(
        own.clone(),
        agent,
        outer,
        outbound,
        32,
    ));
    let make = |number| {
        super::super::tests::event(
            &own,
            Endpoint::agent(own.clone()),
            number,
            "application/octet-stream",
            format!("긴 원문 {number}").as_bytes(),
        )
    };
    broker.dispatch_ingress(make(1)).unwrap();
    let mut stream = TcpStream::connect((own.host.as_str(), own.port))
        .await
        .unwrap();
    let (accepted, _) = listener.accept().await.unwrap();
    let slot = Arc::clone(&shared.slots).acquire_owned().await.unwrap();
    let task = tokio::spawn(serve(
        100,
        accepted,
        broker.clone(),
        shared.clone(),
        Arc::new(slot),
    ));
    super::super::tests::send(&mut stream, &make(2)).await;
    super::super::tests::send(&mut stream, &make(3)).await;
    stream.shutdown().await.unwrap(); // Submission half closes; output remains readable.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert!(!task.is_finished());
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(1));
    drop(next(&store).await.unwrap());
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), next(&store))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.event(), &make(2));
    // Empty queue is not available retained storage while the consumer holds
    // the first decoded Event. The second remains at ingress without commit.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert_eq!(store.storage_snapshot().queued_count, 0);
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(2));
    drop(first);
    let second = tokio::time::timeout(std::time::Duration::from_secs(10), next(&store))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.event(), &make(3));
    drop(second);
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
    tokio::time::timeout(std::time::Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    shared.spawn(deliver_outer(_outer, shared.clone()));
    let mut response = make(4);
    response.envelope.target = response.envelope.source.clone();
    broker.dispatch_ingress(response.clone()).unwrap();
    assert_eq!(
        super::super::tests::receive(&mut stream).await,
        response,
        "input half-close must not discard the still-valid output route"
    );
    drop(stream);
    shared.tasks.lock().unwrap().abort_all();
}

#[tokio::test]
async fn owned_runtime_transport_owner_keeps_failed_writer_and_queued_originals() {
    let shared = Shared::new(RuntimeLimits {
        queue: 2,
        retained: 4,
        bytes: 1024 * 1024,
        connections: 4,
        hop_receipts: 4,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 2,
    });
    let (publisher, store) = completion_mailbox_with_limits(1, 4, 1024 * 1024).unwrap();
    let slot = Arc::clone(&shared.slots).acquire_owned().await.unwrap();
    let sender = shared.writer_with_finish(
        BrokenWrite { bytes_left: 11 },
        Arc::new(slot),
        Some(Arc::new(AtomicBool::new(true))),
    );
    let own = Address::tcp("127.0.0.1", 53112);
    let mut pointers = Vec::new();
    for number in 1..=2 {
        let event = super::super::tests::event(
            &own,
            Endpoint::agent(own.clone()),
            number,
            "application/octet-stream",
            &[42; 4096],
        );
        pointers.push(event.payload.as_ptr() as usize);
        publisher.try_publish_owned(event).unwrap();
        let OwnedPoll::Event(event) = store.try_take_owned() else {
            panic!("owned input");
        };
        sender.send(event).await.unwrap();
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while shared.failures.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(store.storage_snapshot().retained_count, 2);
    assert_eq!(shared.local_writes.load(Ordering::Relaxed), 0);
    assert_eq!(
        shared.slots.available_permits(),
        3,
        "failed connection retains its admission slot"
    );
    let retained_event_bytes;
    {
        let mut failures = shared.failures.lock().unwrap();
        let Failure::Writer { value, .. } = &mut failures[0].value else {
            panic!("writer remainder");
        };
        assert!(value.started);
        assert_eq!(value.current.event().payload.as_ptr() as usize, pointers[0]);
        assert_eq!(value.pending.len(), 1);
        let pending = value.pending.pop_front().unwrap();
        assert_eq!(pending.event().payload.as_ptr() as usize, pointers[1]);
        drop(pending);
        retained_event_bytes =
            p4_adapter::node_adapter::retained_event_bytes(value.current.event()).unwrap();
    }
    let snapshot = Inspector(shared.clone()).snapshot();
    assert_eq!(snapshot["failures"]["count"], 1);
    assert_eq!(snapshot["failures"]["states"]["uncertain"], 1);
    assert_eq!(
        snapshot["failures"]["retained_event_bytes"],
        retained_event_bytes
    );
    assert_eq!(
        snapshot["failures"]["failure_ids"][0]["failure_id"],
        "transport-1"
    );
    drop(shared);
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
}

#[tokio::test]
async fn owned_runtime_finish_drains_originals_before_ack_and_returns_slot() {
    let shared = Shared::new(RuntimeLimits {
        queue: 2,
        retained: 4,
        bytes: 1024 * 1024,
        connections: 1,
        hop_receipts: 4,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 2,
    });
    let (publisher, store) = shared.limits.mailbox();
    let (writer, mut peer) = tokio::io::duplex(16);
    let slot = shared.slots.clone().acquire_owned().await.unwrap();
    let sender = shared.writer_with_finish(
        writer,
        Arc::new(slot),
        Some(Arc::new(AtomicBool::new(true))),
    );
    let own = Address::tcp("127.0.0.1", 53113);
    let mut expected = Vec::new();
    for number in 1..=2 {
        let event = super::super::tests::event(
            &own,
            Endpoint::agent(own.clone()),
            number,
            "application/x-independent-backend",
            &[42; 4096],
        );
        expected.push(event.clone());
        publisher.try_publish_owned(event).unwrap();
        sender.send(next(&store).await.unwrap()).await.unwrap();
    }
    drop(sender);
    tokio::task::yield_now().await;
    assert_eq!(store.storage_snapshot().retained_count, 2);
    assert_eq!(shared.slots.available_permits(), 0);
    for event in expected {
        assert_eq!(read_event(&mut peer).await.unwrap(), Some(event));
    }
    assert_eq!(
        peer.read_u32_le().await.unwrap(),
        0,
        "ACK follows every queued output"
    );
    assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while shared.slots.available_permits() != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
    assert_eq!(shared.local_writes.load(Ordering::Relaxed), 2);
    assert!(shared.failures.lock().unwrap().is_empty());
}

#[tokio::test]
async fn owned_runtime_finish_keeps_replacement_route_and_preserves_late_original() {
    let shared = Shared::new(RuntimeLimits {
        queue: 2,
        retained: 8,
        bytes: 1024 * 1024,
        connections: 2,
        hop_receipts: 8,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 2,
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, incoming) = shared.limits.mailbox();
    let (outer, outgoing) = shared.limits.mailbox();
    let (outbound, _outbound) = shared.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(
        own.clone(),
        agent,
        outer,
        outbound,
        32,
    ));
    shared.spawn(deliver_outer(outgoing.clone(), shared.clone()));
    let mut peers = Vec::new();
    let make = |number| {
        super::super::tests::event(
            &own,
            Endpoint::agent(own.clone()),
            number,
            "application/x-independent-backend",
            b"opaque",
        )
    };
    for number in 1..=2 {
        let mut peer = TcpStream::connect((own.host.as_str(), own.port))
            .await
            .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let slot = shared.slots.clone().acquire_owned().await.unwrap();
        shared.spawn(serve(
            number,
            stream,
            broker.clone(),
            shared.clone(),
            Arc::new(slot),
        ));
        super::super::tests::send(&mut peer, &make(number)).await;
        drop(next(&incoming).await.unwrap());
        peers.push(peer);
    }
    let tombstone = match Endpoint::outer(own.clone(), "failed-generation", 1) {
        Endpoint::Outer(route) => route,
        _ => unreachable!(),
    };
    shared
        .connections
        .lock()
        .await
        .insert(tombstone.clone(), None);
    let before = broker.receipt_snapshot().unwrap();
    peers[0].write_u32_le(0).await.unwrap();
    assert_eq!(peers[0].read_u32_le().await.unwrap(), 0);
    let mut reply = make(3);
    reply.envelope.target = reply.envelope.source.clone();
    broker.dispatch_ingress(reply.clone()).unwrap();
    assert_eq!(
        super::super::tests::receive(&mut peers[1]).await,
        reply,
        "old FINISH cannot erase replacement binding"
    );
    peers[1].write_u32_le(0).await.unwrap();
    assert_eq!(peers[1].read_u32_le().await.unwrap(), 0);
    assert_eq!(shared.connections.lock().await.len(), 1);
    assert!(matches!(
        shared.connections.lock().await.get(&tombstone),
        Some(None)
    ));
    assert_eq!(
        broker.receipt_snapshot().unwrap().committed_events,
        before.committed_events.map(|n| n + 1),
        "FINISH has no receipt authority"
    );

    // Late node work is not declared settled by transport retirement. Its
    // original allocation remains charged in the OUTER failure owner.
    let (publisher, store) = shared.limits.mailbox();
    let mut late = make(4);
    late.envelope.target = late.envelope.source.clone();
    let pointer = late.payload.as_ptr() as usize;
    publisher.try_publish_owned(late).unwrap();
    let charged = store.storage_snapshot().retained_bytes;
    broker
        .dispatch_retained(next(&store).await.unwrap())
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while shared.failures.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    {
        let failures = shared.failures.lock().unwrap();
        let Failure::Undelivered { event, .. } = &failures[0].value else {
            panic!("late output must remain owned");
        };
        assert_eq!(event.event().payload.as_ptr() as usize, pointer);
    }
    assert_eq!(
        store.storage_snapshot().retained_bytes,
        0,
        "broker transfers the charge to the OUTER store"
    );
    assert_eq!(outgoing.storage_snapshot().retained_bytes, charged);
    assert_eq!(outgoing.storage_snapshot().retained_count, 1);
    assert_eq!(shared.local_writes.load(Ordering::Relaxed), 1);
    shared.tasks.lock().unwrap().abort_all();
}

#[tokio::test]
async fn owned_runtime_writer_preserves_partial_write_and_unstarted_queue_claims() {
    let (publisher, store) = completion_mailbox_with_limits(1, 4, 1024 * 1024).unwrap();
    let (sender, receiver) = mpsc::channel(2);
    let own = Address::tcp("127.0.0.1", 53111);
    let mut pointers = Vec::new();
    for number in 1..=2 {
        let mut event = super::super::tests::event(
            &own,
            Endpoint::agent(own.clone()),
            number,
            "application/octet-stream",
            &[42; 32],
        );
        event.payload.reserve_exact(8192);
        pointers.push(event.payload.as_ptr() as usize);
        publisher.try_publish_owned(event).unwrap();
        let OwnedPoll::Event(event) = store.try_take_owned() else {
            panic!("owned input");
        };
        sender.send(event).await.unwrap();
    }
    let before = store.storage_snapshot();
    let count = AtomicU64::new(0);
    let Err(mut failure) = write_loop(BrokenWrite { bytes_left: 11 }, receiver, &count).await
    else {
        panic!("must fail");
    };
    assert!(failure.started);
    assert_eq!(count.load(Ordering::Relaxed), 0);
    assert_eq!(
        failure.current.event().payload.as_ptr() as usize,
        pointers[0]
    );
    assert_eq!(failure.pending.len(), 1);
    assert!(sender.is_closed());
    assert_eq!(store.storage_snapshot().retained_count, 2);
    assert_eq!(
        store.storage_snapshot().retained_bytes,
        before.retained_bytes
    );
    let queued = failure.pending.pop_front().unwrap();
    assert_eq!(queued.event().payload.as_ptr() as usize, pointers[1]);
    drop(failure);
    assert_eq!(store.storage_snapshot().retained_count, 1);
    drop(queued);
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
}

#[tokio::test]
async fn hop_partial_write_is_uncertain_and_keeps_current_plus_unstarted_queue() {
    let (publisher, store) = completion_mailbox_with_limits(2, 4, 1024 * 1024).unwrap();
    let (events, receiver) = mpsc::channel(2);
    let (_controls, control_receiver) = mpsc::channel(2);
    let own = Address::tcp("127.0.0.1", 53114);
    for number in 1..=2 {
        publisher
            .try_publish_owned(super::super::tests::event(
                &own,
                Endpoint::agent(own.clone()),
                number,
                "application/octet-stream",
                &[42; 32],
            ))
            .unwrap();
        events.send(next(&store).await.unwrap()).await.unwrap();
    }
    let writes = AtomicU64::new(0);
    let hop_writes = AtomicU64::new(0);
    let hop_bytes = AtomicU64::new(0);
    let failure = write_hop_loop(
        &mut BrokenWrite { bytes_left: 11 },
        receiver,
        control_receiver,
        &writes,
        &hop_writes,
        &hop_bytes,
        None,
        2,
        HopLiveTracker::default(),
        "agent-a",
        7,
    )
    .await
    .unwrap_err();
    assert_eq!(failure.state, HopFailureState::Uncertain);
    assert!(failure.current.is_some());
    assert_eq!(failure.outstanding.len(), 0);
    assert_eq!(failure.pending.len(), 1);
    assert_eq!(store.storage_snapshot().retained_count, 2);
    drop(failure);
    assert_eq!(store.storage_snapshot().retained_count, 0);
}

#[tokio::test]
async fn inspect_separates_live_hop_outstanding_from_receipts_and_failures() {
    use p4_protocol::event::hop::{self, HopFrame, ReceiptStatus};
    let limits = super::super::tests::limits();
    let shared = Shared::new(limits);
    let inspector = Inspector(Arc::clone(&shared));
    let (publisher, store) = completion_mailbox_with_limits(1, 2, 1024 * 1024).unwrap();
    let event = super::super::tests::event(
        &Address::tcp("127.0.0.1", 53115),
        Endpoint::agent(Address::tcp("127.0.0.2", 53115)),
        1,
        "application/octet-stream",
        &[42; 32],
    );
    publisher.try_publish_owned(event).unwrap();
    let owned = next(&store).await.unwrap();
    let retained_bytes = owned.retained_bytes();
    let (events, receiver) = mpsc::channel(1);
    let (controls, control_receiver) = mpsc::channel(2);
    events.send(owned).await.unwrap();
    drop(events);
    let (mut reader, mut writer) = tokio::io::duplex(16 * 1024);
    let live = shared.hop_live.clone();
    let counters = Arc::clone(&shared);
    let task = tokio::spawn(async move {
        write_hop_loop(
            &mut writer,
            receiver,
            control_receiver,
            &AtomicU64::new(0),
            &counters.hop_data_writes,
            &counters.hop_data_bytes,
            None,
            1,
            live,
            "agent-a",
            7,
        )
        .await
    });
    let size = reader.read_u32_le().await.unwrap() as usize;
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).await.unwrap();
    let HopFrame::Data {
        attempt, digest, ..
    } = hop::decode(&bytes).unwrap().unwrap()
    else {
        panic!("first frame is not hop data");
    };
    let snapshot = inspector.snapshot();
    assert_eq!(snapshot["outstanding"]["events"], 1);
    assert_eq!(snapshot["outstanding"]["event_bytes"], retained_bytes);
    assert_eq!(snapshot["transfer"]["hop_data_writes"], 1);
    assert!(snapshot["transfer"]["hop_data_bytes"].as_u64().unwrap() > 0);
    assert_eq!(snapshot["receipts"]["records"], 0);
    assert_eq!(snapshot["failures"]["count"], 0);
    controls
        .send(HopCommand::Received(HopFrame::Receipt {
            attempt,
            digest,
            status: ReceiptStatus::AcceptedExact,
            detail: "accepted".into(),
        }))
        .await
        .unwrap();
    assert!(task.await.unwrap().is_ok());
    assert_eq!(inspector.snapshot()["outstanding"]["events"], 0);
    assert_eq!(store.storage_snapshot().retained_count, 0);
}
