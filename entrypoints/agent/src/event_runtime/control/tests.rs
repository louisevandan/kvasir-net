use super::*;
use super::super::{tests::{event, limits}, next};

#[tokio::test]
async fn owned_runtime_control_full_retries_and_permanent_failure_retains_inputs_and_reply() {
    let limits = limits();
    let (agent, input) = limits.mailbox();
    let (outer, output) = limits.mailbox();
    let (outbound, _) = limits.mailbox();
    let own = Address::tcp("127.0.0.1", 54100);
    let broker = Arc::new(RetainedEventBroker::new(own.clone(), agent, outer, outbound, 32));
    let route = Endpoint::outer(own.clone(), "owned-runtime", 1);
    broker.dispatch_ingress(event(&own, route, 10, "application/octet-stream", b"occupy output")).unwrap();
    let first = event(&own, Endpoint::agent(own.clone()), 11, "application/x-control-test", b"original first");
    broker.dispatch_ingress(first).unwrap();
    let task = tokio::spawn(run(own.clone(), broker.clone(), input.clone(), limits));
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while input.storage_snapshot().queued_count != 0 { tokio::task::yield_now().await; }
    }).await.unwrap();
    broker.dispatch_ingress(event(&own, Endpoint::agent(own.clone()), 12, "application/x-control-test", b"second")).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    assert!(!task.is_finished());
    assert_eq!(input.storage_snapshot().retained_count, 2, "active and queued inputs stay charged during Full");
    assert_eq!(input.storage_snapshot().queued_count, 1);
    assert_eq!(next(&output).await.unwrap().event().payload, b"occupy output");
    for number in [11, 12] {
        let reply = tokio::time::timeout(std::time::Duration::from_secs(10), next(&output)).await.unwrap().unwrap();
        assert_eq!(reply.event().envelope.causation_id, Some(format!("input-{number}")));
    }
    let mut bad = event(&own, Endpoint::agent(own.clone()), 13, "application/x-control-test", b"terminal original");
    bad.envelope.return_route = None;
    bad.envelope.source = Endpoint::node(own.clone(), "missing-return", 1);
    let pointer = bad.payload.as_ptr() as usize;
    broker.dispatch_ingress(bad).unwrap();
    let stopped = tokio::time::timeout(std::time::Duration::from_secs(10), task).await.unwrap().unwrap();
    assert!(stopped.error.as_deref().unwrap().contains("UnknownNode"));
    assert_eq!(stopped.held_input.as_ref().unwrap().event().payload.as_ptr() as usize, pointer);
    let Some(PendingReply::Owned(reply)) = &stopped.held_reply else { panic!("original owned reply must remain"); };
    assert_eq!(reply.event().envelope.causation_id.as_deref(), Some("input-13"));
    broker.dispatch_ingress(event(&own, Endpoint::agent(own.clone()), 14, "application/x-control-test", b"queued after stop")).unwrap();
    assert_eq!(stopped.receiver.storage_snapshot().retained_count, 2);
    drop(stopped);
    assert_eq!(input.storage_snapshot().retained_count, 1);
    drop(next(&input).await.unwrap());
    assert_eq!(input.storage_snapshot().retained_bytes, 0);
}

#[tokio::test]
async fn owned_runtime_delete_refuses_held_completion_even_when_adapter_says_empty() {
    use p4_adapter::node_adapter::{CompletionFront, OwnedPoll, RetainedOfferError};
    use std::task::{Context, Poll};
    struct Adapter { mailbox: Arc<CompletionMailbox> }
    impl RetainedNodeAdapter for Adapter {
        fn try_offer_retained(&self, event: RetainedCompletion) -> Result<(), RetainedOfferError> { Err(RetainedOfferError::Full(event)) }
        fn peek_retained_completion(&self) -> Option<CompletionFront> { self.mailbox.peek_owned_front() }
        fn try_take_retained_matching(&self, front: &CompletionFront) -> OwnedPoll { self.mailbox.try_take_owned_matching(front) }
        fn poll_take_retained(&self, cx: &mut Context<'_>) -> Poll<OwnedPoll> { self.mailbox.poll_take_owned(cx) }
        fn snapshot(&self) -> String { "empty".into() }
        fn completion_storage_snapshot(&self) -> Option<p4_adapter::node_adapter::CompletionStorageSnapshot> { Some(self.mailbox.storage_snapshot()) }
    }
    let own = Address::tcp("127.0.0.1", 54101);
    let limits = limits();
    let (agent, _) = limits.mailbox(); let (outer, _) = limits.mailbox(); let (outbound, _) = limits.mailbox();
    let broker = Arc::new(RetainedEventBroker::new(own.clone(), agent, outer, outbound, 32));
    let (node_sender, inbound) = limits.mailbox();
    broker.register_node("node", 1, node_sender).unwrap();
    let (completion, store) = limits.mailbox();
    completion.try_publish_owned(event(&own, Endpoint::agent(own.clone()), 1, "application/octet-stream", b"held output")).unwrap();
    let held = next(&store).await.unwrap();
    let pointer = held.event().payload.as_ptr();
    let task = tokio::spawn(std::future::pending());
    let mut nodes = HashMap::from([("node".into(), NodeOwner { generation: 1, adapter_kind: "opaque-test".into(),
        adapter: Arc::new(Adapter { mailbox: store.clone() }), inbound: inbound.clone(), task })]);
    let delete = event(&own, Endpoint::agent(own.clone()), 2, DELETE, b"{\"node_id\":\"node\",\"node_generation\":1}");
    assert!(remove(&broker, &mut nodes, &delete).await.unwrap_err().contains("delivery must be drained"));
    assert!(nodes.contains_key("node"));
    assert_eq!(held.event().payload.as_ptr(), pointer);
    assert_eq!(store.storage_snapshot().retained_count, 1);
    // Refusal releases the temporary admission fence: the exact route works.
    broker.dispatch_ingress(event(&own, Endpoint::node(own.clone(), "node", 1), 3, "application/octet-stream", b"next")).unwrap();
    drop(next(&inbound).await.unwrap());
    drop(held);
    assert_eq!(remove(&broker, &mut nodes, &delete).await.unwrap(), "node");
}
