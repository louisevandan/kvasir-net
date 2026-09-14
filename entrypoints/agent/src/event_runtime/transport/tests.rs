use super::*;
use p4_adapter::node_adapter::{completion_mailbox_with_limits, OwnedPoll};
use std::pin::Pin;
use std::task::{Context, Poll};

struct BrokenWrite { bytes_left: usize }
impl AsyncWrite for BrokenWrite {
    fn poll_write(mut self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        if self.bytes_left == 0 { return Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected after frame prefix"))); }
        let count = self.bytes_left.min(buf.len()); self.bytes_left -= count; Poll::Ready(Ok(count))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> { Poll::Ready(Ok(())) }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> { Poll::Ready(Ok(())) }
}

#[tokio::test]
async fn owned_runtime_tcp_ingress_full_keeps_original_and_waits_for_retirement() {
    let shared = Shared::new(RuntimeLimits { queue: 1, retained: 1, bytes: 1024 * 1024, connections: 4 });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, store) = shared.limits.mailbox();
    let (outer, _outer) = shared.limits.mailbox(); let (outbound, _outbound) = shared.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(own.clone(), agent, outer, outbound, 32));
    let make = |number| super::super::tests::event(&own, Endpoint::agent(own.clone()), number,
        "application/octet-stream", format!("긴 원문 {number}").as_bytes());
    broker.dispatch_ingress(make(1)).unwrap();
    let mut stream = TcpStream::connect((own.host.as_str(), own.port)).await.unwrap();
    let (accepted, _) = listener.accept().await.unwrap();
    let slot = Arc::clone(&shared.slots).acquire_owned().await.unwrap();
    let task = tokio::spawn(serve(100, accepted, broker.clone(), shared.clone(), Arc::new(slot)));
    super::super::tests::send(&mut stream, &make(2)).await;
    super::super::tests::send(&mut stream, &make(3)).await;
    stream.shutdown().await.unwrap(); // Submission half closes; output remains readable.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert!(!task.is_finished());
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(1));
    drop(next(&store).await.unwrap());
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), next(&store)).await.unwrap().unwrap();
    assert_eq!(first.event(), &make(2));
    // Empty queue is not available retained storage while the consumer holds
    // the first decoded Event. The second remains at ingress without commit.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert_eq!(store.storage_snapshot().queued_count, 0);
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(2));
    drop(first);
    let second = tokio::time::timeout(std::time::Duration::from_secs(10), next(&store)).await.unwrap().unwrap();
    assert_eq!(second.event(), &make(3));
    drop(second);
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
    tokio::time::timeout(std::time::Duration::from_secs(10), task).await.unwrap().unwrap();
    shared.spawn(deliver_outer(_outer, shared.clone()));
    let mut response = make(4);
    response.envelope.target = response.envelope.source.clone();
    broker.dispatch_ingress(response.clone()).unwrap();
    assert_eq!(super::super::tests::receive(&mut stream).await, response,
        "input half-close must not discard the still-valid output route");
    drop(stream);
    shared.tasks.lock().unwrap().abort_all();
}

#[tokio::test]
async fn owned_runtime_transport_owner_keeps_failed_writer_and_queued_originals() {
    let shared = Shared::new(RuntimeLimits { queue: 2, retained: 4, bytes: 1024 * 1024, connections: 4 });
    let (publisher, store) = completion_mailbox_with_limits(1, 4, 1024 * 1024).unwrap();
    let slot = Arc::clone(&shared.slots).acquire_owned().await.unwrap();
    let sender = shared.writer_with_finish(BrokenWrite { bytes_left: 11 }, Arc::new(slot), Some(Arc::new(AtomicBool::new(true))));
    let own = Address::tcp("127.0.0.1", 53112);
    let mut pointers = Vec::new();
    for number in 1..=2 {
        let event = super::super::tests::event(&own, Endpoint::agent(own.clone()), number, "application/octet-stream", &[42; 4096]);
        pointers.push(event.payload.as_ptr() as usize);
        publisher.try_publish_owned(event).unwrap();
        let OwnedPoll::Event(event) = store.try_take_owned() else { panic!("owned input"); };
        sender.send(event).await.unwrap();
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while shared.failures.lock().unwrap().is_empty() { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(store.storage_snapshot().retained_count, 2);
    assert_eq!(shared.local_writes.load(Ordering::Relaxed), 0);
    assert_eq!(shared.slots.available_permits(), 3, "failed connection retains its admission slot");
    {
        let mut failures = shared.failures.lock().unwrap();
        let Failure::Writer { value, .. } = &mut failures[0] else { panic!("writer remainder"); };
        assert!(value.started);
        assert_eq!(value.current.event().payload.as_ptr() as usize, pointers[0]);
        assert_eq!(value.pending.len(), 1);
        let pending = value.pending.try_recv().unwrap();
        assert_eq!(pending.event().payload.as_ptr() as usize, pointers[1]);
        drop(pending);
    }
    drop(shared);
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
}

#[tokio::test]
async fn owned_runtime_finish_drains_originals_before_ack_and_returns_slot() {
    let shared = Shared::new(RuntimeLimits { queue: 2, retained: 4, bytes: 1024 * 1024, connections: 1 });
    let (publisher, store) = shared.limits.mailbox();
    let (writer, mut peer) = tokio::io::duplex(16);
    let slot = shared.slots.clone().acquire_owned().await.unwrap();
    let sender = shared.writer_with_finish(writer, Arc::new(slot), Some(Arc::new(AtomicBool::new(true))));
    let own = Address::tcp("127.0.0.1", 53113);
    let mut expected = Vec::new();
    for number in 1..=2 {
        let event = super::super::tests::event(&own, Endpoint::agent(own.clone()), number, "application/x-independent-backend", &[42; 4096]);
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
    assert_eq!(peer.read_u32_le().await.unwrap(), 0, "ACK follows every queued output");
    assert_eq!(peer.read(&mut [0; 1]).await.unwrap(), 0);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while shared.slots.available_permits() != 1 { tokio::task::yield_now().await; }
    }).await.unwrap();
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
    assert_eq!(shared.local_writes.load(Ordering::Relaxed), 2);
    assert!(shared.failures.lock().unwrap().is_empty());
}

#[tokio::test]
async fn owned_runtime_finish_keeps_replacement_route_and_preserves_late_original() {
    let shared = Shared::new(RuntimeLimits { queue: 2, retained: 8, bytes: 1024 * 1024, connections: 2 });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let own = Address::tcp("127.0.0.1", listener.local_addr().unwrap().port());
    let (agent, incoming) = shared.limits.mailbox();
    let (outer, outgoing) = shared.limits.mailbox(); let (outbound, _outbound) = shared.limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(own.clone(), agent, outer, outbound, 32));
    shared.spawn(deliver_outer(outgoing.clone(), shared.clone()));
    let mut peers = Vec::new();
    let make = |number| super::super::tests::event(&own, Endpoint::agent(own.clone()), number, "application/x-independent-backend", b"opaque");
    for number in 1..=2 {
        let mut peer = TcpStream::connect((own.host.as_str(), own.port)).await.unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let slot = shared.slots.clone().acquire_owned().await.unwrap();
        shared.spawn(serve(number, stream, broker.clone(), shared.clone(), Arc::new(slot)));
        super::super::tests::send(&mut peer, &make(number)).await;
        drop(next(&incoming).await.unwrap());
        peers.push(peer);
    }
    let tombstone = match Endpoint::outer(own.clone(), "failed-generation", 1) { Endpoint::Outer(route) => route, _ => unreachable!() };
    shared.connections.lock().await.insert(tombstone.clone(), None);
    let before = broker.receipt_snapshot().unwrap();
    peers[0].write_u32_le(0).await.unwrap();
    assert_eq!(peers[0].read_u32_le().await.unwrap(), 0);
    let mut reply = make(3); reply.envelope.target = reply.envelope.source.clone();
    broker.dispatch_ingress(reply.clone()).unwrap();
    assert_eq!(super::super::tests::receive(&mut peers[1]).await, reply, "old FINISH cannot erase replacement binding");
    peers[1].write_u32_le(0).await.unwrap();
    assert_eq!(peers[1].read_u32_le().await.unwrap(), 0);
    assert_eq!(shared.connections.lock().await.len(), 1);
    assert!(matches!(shared.connections.lock().await.get(&tombstone), Some(None)));
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, before.committed_events.map(|n| n + 1), "FINISH has no receipt authority");

    // Late node work is not declared settled by transport retirement. Its
    // original allocation remains charged in the OUTER failure owner.
    let (publisher, store) = shared.limits.mailbox();
    let mut late = make(4); late.envelope.target = late.envelope.source.clone();
    let pointer = late.payload.as_ptr() as usize;
    publisher.try_publish_owned(late).unwrap();
    let charged = store.storage_snapshot().retained_bytes;
    broker.dispatch_retained(next(&store).await.unwrap()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while shared.failures.lock().unwrap().is_empty() { tokio::task::yield_now().await; }
    }).await.unwrap();
    {
        let failures = shared.failures.lock().unwrap();
        let Failure::Undelivered { event, .. } = &failures[0] else { panic!("late output must remain owned"); };
        assert_eq!(event.event().payload.as_ptr() as usize, pointer);
    }
    assert_eq!(store.storage_snapshot().retained_bytes, 0, "broker transfers the charge to the OUTER store");
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
        let mut event = super::super::tests::event(&own, Endpoint::agent(own.clone()), number, "application/octet-stream", &[42; 32]);
        event.payload.reserve_exact(8192);
        pointers.push(event.payload.as_ptr() as usize);
        publisher.try_publish_owned(event).unwrap();
        let OwnedPoll::Event(event) = store.try_take_owned() else { panic!("owned input"); };
        sender.send(event).await.unwrap();
    }
    let before = store.storage_snapshot();
    let count = AtomicU64::new(0);
    let Err(mut failure) = write_loop(BrokenWrite { bytes_left: 11 }, receiver, &count).await else { panic!("must fail"); };
    assert!(failure.started);
    assert_eq!(count.load(Ordering::Relaxed), 0);
    assert_eq!(failure.current.event().payload.as_ptr() as usize, pointers[0]);
    assert_eq!(failure.pending.len(), 1);
    assert!(sender.is_closed());
    assert_eq!(store.storage_snapshot().retained_count, 2);
    assert_eq!(store.storage_snapshot().retained_bytes, before.retained_bytes);
    let queued = failure.pending.try_recv().unwrap();
    assert_eq!(queued.event().payload.as_ptr() as usize, pointers[1]);
    drop(failure);
    assert_eq!(store.storage_snapshot().retained_count, 1);
    drop(queued);
    assert_eq!(store.storage_snapshot().retained_bytes, 0);
}
