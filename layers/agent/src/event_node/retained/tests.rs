use super::*;
use p4_adapter::node_adapter::{
    CompletionFront, CompletionPublisher, ReserveError, completion_mailbox_with_limits,
    retained_event_bytes,
};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Envelope, Event, EventClass};
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll as TaskPoll};
use std::time::Duration;

fn queue() -> (CompletionPublisher, Arc<CompletionMailbox>) {
    let (sender, mailbox) = completion_mailbox_with_limits(1, 8, 1 << 20).unwrap();
    (sender, mailbox)
}
fn own() -> Address {
    Address::tcp("127.0.0.1", 52001)
}
fn event(id: &str, target: Endpoint) -> Event {
    let return_route = match &target {
        Endpoint::Outer(route) => Some(route.clone()),
        _ => Some(p4_protocol::event::OuterEndpoint {
            ingress_agent: p4_protocol::Address::tcp("127.0.0.1", 52001),
            channel: "outer".into(),
            connection_generation: 1,
        }),
    };
    Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: id.into(),
            correlation_id: id.into(),
            causation_id: None,
            source: Endpoint::node(own(), "producer", 1),
            target,
            return_route,
            class: EventClass::Control,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: Some("opaque-node-test".into()),
            payload_content_type: "application/test".into(),
        },
        payload: {
            let mut v = Vec::with_capacity(8192);
            v.extend([0, 255, 128, 7]);
            v
        },
    }
}
fn publish(sender: &CompletionPublisher, event: Event) {
    let permission = sender
        .try_reserve(1, retained_event_bytes(&event).unwrap())
        .unwrap();
    sender.publish_reserved(event, permission).unwrap();
}
fn take(mailbox: &CompletionMailbox) -> RetainedCompletion {
    match mailbox.try_take_owned() {
        OwnedPoll::Event(e) => e,
        p => panic!("{p:?}"),
    }
}
async fn until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("actual node consumer did not progress");
}
struct Adapter {
    input: CompletionPublisher,
    completion: Arc<CompletionMailbox>,
    closed: AtomicBool,
}
impl RetainedNodeAdapter for Adapter {
    fn try_offer_retained(&self, completion: RetainedCompletion) -> Result<(), RetainedOfferError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(RetainedOfferError::Closed(completion));
        }
        let (slot, claim) = match self
            .input
            .try_reserve_delivery(retained_event_bytes(completion.event()).unwrap())
        {
            Ok(value) => value,
            Err(ReserveError::Full) => return Err(RetainedOfferError::Full(completion)),
            other => panic!("unexpected admission: {other:?}"),
        };
        completion
            .transfer_with_queue_deferred(&self.input, claim, slot)
            .unwrap()
            .notify();
        Ok(())
    }
    fn peek_retained_completion(&self) -> Option<CompletionFront> {
        self.completion.peek_owned_front()
    }
    fn try_take_retained_matching(&self, expected: &CompletionFront) -> OwnedPoll {
        self.completion.try_take_owned_matching(expected)
    }
    fn poll_take_retained(&self, context: &mut Context<'_>) -> TaskPoll<OwnedPoll> {
        self.completion.poll_take_owned(context)
    }
    fn snapshot(&self) -> String {
        "retained-test-adapter".into()
    }
}

#[tokio::test]
async fn retained_node_keeps_both_held_claims_and_drains_independent_front_at_cap1() {
    let (agent_tx, agent_rx) = queue();
    let (outer_tx, outer_rx) = queue();
    let (out_tx, _out_rx) = queue();
    let broker = Arc::new(RetainedEventBroker::new(
        own(),
        agent_tx,
        outer_tx,
        out_tx,
        8,
    ));
    let (inbound_tx, inbound) = queue();
    broker.register_node("node", 1, inbound_tx).unwrap();
    let (adapter_input, adapter_rx) = queue();
    publish(
        &adapter_input,
        event("adapter-busy", Endpoint::agent(own())),
    );
    let (completion_tx, completion_rx) = queue();
    let adapter = Arc::new(Adapter {
        input: adapter_input,
        completion: completion_rx.clone(),
        closed: AtomicBool::new(false),
    });
    broker
        .dispatch_ingress(event("agent-busy", Endpoint::agent(own())))
        .unwrap();
    let input = event("input", Endpoint::node(own(), "node", 1));
    let input_pointer = input.payload.as_ptr() as usize;
    broker.dispatch_ingress(input).unwrap();
    let output = event("output", Endpoint::agent(own()));
    let output_pointer = output.payload.as_ptr() as usize;
    publish(&completion_tx, output);
    let task = tokio::spawn(
        RetainedEventNode::new(adapter.clone(), inbound.clone(), broker.clone()).run(),
    );
    until(|| {
        inbound.storage_snapshot().queued_count == 0
            && completion_rx.storage_snapshot().queued_count == 0
    })
    .await;
    assert_eq!(
        inbound.storage_snapshot().retained_count,
        1,
        "held input remains charged"
    );
    assert_eq!(
        completion_rx.storage_snapshot().retained_count,
        1,
        "held output remains charged"
    );
    let independent = event("independent", Endpoint::outer(own(), "outer", 1));
    let pointer = independent.payload.as_ptr() as usize;
    publish(&completion_tx, independent);
    until(|| outer_rx.storage_snapshot().queued_count == 1).await;
    assert_eq!(completion_rx.storage_snapshot().retained_count, 1);
    let independent = take(&outer_rx);
    assert_eq!(independent.event().payload.as_ptr() as usize, pointer);
    assert_eq!(outer_rx.storage_snapshot().retained_count, 1);
    // A terminal adapter refusal must return BOTH exact owners; it does not
    // drain either queue, fabricate completion, or retire them as cleanup.
    adapter.closed.store(true, Ordering::SeqCst);
    let failure = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.error, EventNodeError::AdapterClosed);
    assert_eq!(
        failure
            .held_input
            .as_ref()
            .unwrap()
            .event()
            .payload
            .as_ptr() as usize,
        input_pointer
    );
    assert_eq!(
        failure
            .held_output
            .as_ref()
            .unwrap()
            .event()
            .payload
            .as_ptr() as usize,
        output_pointer
    );
    assert!(failure.completion_at_failure.is_none());
    assert_eq!(inbound.storage_snapshot().retained_count, 1);
    assert_eq!(completion_rx.storage_snapshot().retained_count, 1);
    assert_eq!(broker.receipt_snapshot().unwrap().committed_events, Some(3));
    drop(failure);
    assert_eq!(inbound.storage_snapshot().retained_count, 0);
    assert_eq!(completion_rx.storage_snapshot().retained_count, 0);
    independent.retire();
    take(&agent_rx).retire();
    take(&adapter_rx).retire();
}

#[tokio::test]
async fn retained_node_full_front_stays_queued_and_never_overtakes_same_stream() {
    let (agent_tx, agent_rx) = queue();
    let (outer_tx, outer_rx) = queue();
    let (out_tx, _out_rx) = queue();
    let broker = Arc::new(RetainedEventBroker::new(
        own(),
        agent_tx,
        outer_tx,
        out_tx,
        8,
    ));
    let (_inbound_tx, inbound) = queue();
    let (adapter_input, _adapter_rx) = queue();
    let (completion_tx, completion_rx) = queue();
    let adapter = Arc::new(Adapter {
        input: adapter_input,
        completion: completion_rx.clone(),
        closed: AtomicBool::new(false),
    });
    broker
        .dispatch_ingress(event("agent-busy", Endpoint::agent(own())))
        .unwrap();
    broker
        .dispatch_ingress(event("outer-busy", Endpoint::outer(own(), "outer", 1)))
        .unwrap();
    let blocked = event("blocked", Endpoint::agent(own()));
    let stream = blocked.envelope.correlation_id.clone();
    publish(&completion_tx, blocked);
    let node = RetainedEventNode::new(adapter, inbound, broker.clone());
    let held = take(&completion_rx);
    publish(
        &completion_tx,
        event("independent-full", Endpoint::outer(own(), "outer", 1)),
    );
    let before = completion_rx.storage_snapshot();
    node.forward_independent_front(&held).unwrap();
    assert_eq!(
        completion_rx.storage_snapshot(),
        before,
        "Full must leave independent front in its actual store"
    );
    take(&completion_rx).retire();
    take(&outer_rx).retire();
    let mut same = event("same-stream", Endpoint::outer(own(), "outer", 1));
    same.envelope.correlation_id = stream;
    same.envelope.sequence = 2;
    publish(&completion_tx, same);
    node.forward_independent_front(&held).unwrap();
    assert_eq!(
        outer_rx.storage_snapshot().queued_count,
        0,
        "same source/correlation may not overtake"
    );
    assert_eq!(completion_rx.storage_snapshot().queued_count, 1);
    take(&agent_rx).retire();
    held.retire();
    take(&completion_rx).retire();
}

#[test]
fn retained_node_raw_boundary_witness_discharges_bytes_while_payload_is_still_live() {
    // Historical raw consumer behavior, not an assertion that raw consumers
    // gained retained semantics. The migrated tests above must keep one claim.
    let (publisher, mailbox) = queue();
    let original = event("raw-witness", Endpoint::agent(own()));
    let pointer = original.payload.as_ptr();
    publisher.try_publish(original).unwrap();
    let p4_adapter::node_adapter::Poll::Event(held) = mailbox.try_take() else {
        panic!("raw witness");
    };
    assert_eq!(held.payload.as_ptr(), pointer);
    assert_eq!(held.payload.len(), 4);
    assert_eq!(
        mailbox.storage_snapshot().retained_bytes,
        0,
        "historical dequeue already returned its claim"
    );
}

#[tokio::test]
async fn retained_node_repeated_real_input_and_completion_handoffs_retire_each_store() {
    let (agent_tx, agent_rx) = queue();
    let (outer_tx, outer_rx) = queue();
    let (out_tx, _out_rx) = queue();
    let broker = Arc::new(RetainedEventBroker::new(
        own(),
        agent_tx,
        outer_tx,
        out_tx,
        8,
    ));
    let (inbound_tx, inbound) = queue();
    broker.register_node("node", 1, inbound_tx).unwrap();
    let (adapter_input, adapter_rx) = queue();
    let (completion_tx, completion_rx) = queue();
    let adapter = Arc::new(Adapter {
        input: adapter_input,
        completion: completion_rx.clone(),
        closed: AtomicBool::new(false),
    });
    let task = tokio::spawn(RetainedEventNode::new(adapter, inbound.clone(), broker.clone()).run());
    for i in 0..32 {
        let input = event(&format!("request-{i}"), Endpoint::node(own(), "node", 1));
        let pointer = input.payload.as_ptr() as usize;
        broker.dispatch_ingress(input).unwrap();
        until(|| adapter_rx.storage_snapshot().queued_count == 1).await;
        let input = take(&adapter_rx);
        assert_eq!(input.event().payload.as_ptr() as usize, pointer);
        assert_eq!(inbound.storage_snapshot().retained_count, 0);
        assert_eq!(adapter_rx.storage_snapshot().retained_count, 1);
        let output = event(&format!("answer-{i}"), Endpoint::outer(own(), "outer", 1));
        let pointer = output.payload.as_ptr() as usize;
        publish(&completion_tx, output);
        input.retire();
        until(|| outer_rx.storage_snapshot().queued_count == 1).await;
        let output = take(&outer_rx);
        assert_eq!(output.event().payload.as_ptr() as usize, pointer);
        assert_eq!(completion_rx.storage_snapshot().retained_count, 0);
        assert_eq!(outer_rx.storage_snapshot().retained_count, 1);
        output.retire();
        for store in [&inbound, &adapter_rx, &completion_rx, &outer_rx, &agent_rx] {
            let snapshot = store.storage_snapshot();
            assert_eq!(snapshot.retained_count, 0);
            assert_eq!(snapshot.retained_bytes, 0);
            assert_eq!(snapshot.reserved_queue_slots, 0);
        }
    }
    assert_eq!(broker.receipt_snapshot().unwrap().indexed.events, 8);
    assert_eq!(
        broker.receipt_snapshot().unwrap().committed_events,
        Some(64)
    );
    broker.unregister_node("node", 1).unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
