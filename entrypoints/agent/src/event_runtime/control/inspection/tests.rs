use super::*;

#[tokio::test]
async fn inspection_counts_retained_receipts_through_the_actual_control_loop() {
    use p4_agent_core::event_broker::RetainedEventBroker;
    use p4_protocol::{
        Address,
        event::{AGENT_INSPECT_CONTENT_TYPE, Endpoint, Event, EventClass},
    };
    use std::sync::Arc;
    let own = Address::tcp("127.0.0.1", 53120);
    let remote = Address::tcp("127.0.0.2", 53120);
    let (agent_tx, agent_rx) =
        p4_adapter::node_adapter::completion_mailbox_with_limits(1, 8, 1024 * 1024).unwrap();
    let (outer_tx, outer_rx) =
        p4_adapter::node_adapter::completion_mailbox_with_limits(1, 8, 1024 * 1024).unwrap();
    let (outbound_tx, outbound_rx) =
        p4_adapter::node_adapter::completion_mailbox_with_limits(1, 8, 1024 * 1024).unwrap();
    let broker = Arc::new(RetainedEventBroker::new(
        own.clone(),
        agent_tx,
        outer_tx,
        outbound_tx,
        8,
    ));
    let route = Endpoint::outer(own.clone(), "inspection-test", 1);
    let mut event = Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: "retained-input".into(),
            correlation_id: "retained-probe".into(),
            causation_id: None,
            source: route.clone(),
            target: Endpoint::agent(remote),
            return_route: match route {
                Endpoint::Outer(ref outer) => Some(outer.clone()),
                _ => unreachable!(),
            },
            class: EventClass::Data,
            sequence: 1,
            deadline_unix_ms: None,
            adapter_kind: None,
            payload_content_type: "application/octet-stream".into(),
        },
        payload: vec![42; 4096],
    };
    let first_cost = p4_adapter::node_adapter::retained_event_bytes(&event.clone()).unwrap();
    broker.dispatch_ingress(event.clone()).unwrap();
    drop(crate::event_runtime::next(&outbound_rx).await.unwrap()); // Destination consumption does not free the receipt.
    event.envelope.event_id = "inspect-receipts".into();
    event.envelope.target = Endpoint::agent(own.clone());
    event.envelope.class = EventClass::Control;
    event.envelope.sequence = 2;
    event.envelope.payload_content_type = AGENT_INSPECT_CONTENT_TYPE.into();
    event.payload = b"{}".to_vec();
    let second_cost = p4_adapter::node_adapter::retained_event_bytes(&event.clone()).unwrap();
    let limits = crate::event_runtime::RuntimeLimits {
        queue: 1,
        retained: 8,
        bytes: 1024 * 1024,
        connections: 8,
        hop_receipts: 8,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 4,
    };
    let transport = crate::event_runtime::transport::Inspector::detached(limits);
    let task = tokio::spawn(super::super::run(
        own,
        Arc::clone(&broker),
        agent_rx,
        limits,
        transport,
    ));
    broker.dispatch_ingress(event).unwrap();
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        crate::event_runtime::next(&outer_rx),
    )
    .await
    .unwrap()
    .unwrap();
    task.abort();
    let value: Value = serde_json::from_slice(&reply.event().payload).unwrap();
    assert_eq!(value["broker"]["receipts"]["indexed"]["events"], 2);
    assert_eq!(
        value["broker"]["receipts"]["indexed"]["event_bytes"],
        first_cost + second_cost
    );
    assert_eq!(
        value["broker"]["receipts"]["indexed"]["payload_capacity_bytes"],
        4098
    );
    assert_eq!(value["broker"]["receipts"]["retired"]["events"], 0);
    assert_eq!(value["broker"]["receipts"]["committed_events"], 2);
    assert!(value["broker"]["sampled_at_unix_ms"].is_u64());
}

#[tokio::test]
async fn an_empty_agent_still_reports_machine_and_protocol_identity() {
    use p4_agent_core::event_broker::RetainedEventBroker;
    let (agent, _agent_rx) =
        p4_adapter::node_adapter::completion_mailbox_with_limits(1, 8, 1024 * 1024).unwrap();
    let (outer, _outer_rx) =
        p4_adapter::node_adapter::completion_mailbox_with_limits(1, 8, 1024 * 1024).unwrap();
    let (outbound, _outbound_rx) =
        p4_adapter::node_adapter::completion_mailbox_with_limits(1, 8, 1024 * 1024).unwrap();
    let broker = RetainedEventBroker::new(
        p4_protocol::Address::tcp("127.0.0.1", 53001),
        agent,
        outer,
        outbound,
        8,
    );
    let limits = crate::event_runtime::RuntimeLimits {
        queue: 1,
        retained: 8,
        bytes: 1024 * 1024,
        connections: 8,
        hop_receipts: 8,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 4,
    };
    let transport = crate::event_runtime::transport::Inspector::detached(limits);
    let snapshot = snapshot(&HashMap::new(), &broker, &transport).await;

    assert_eq!(snapshot["schema"], 1);
    assert_eq!(snapshot["protocol_version"], Envelope::VERSION);
    assert_eq!(
        snapshot["machine"]["capability"]["os"],
        std::env::consts::OS
    );
    assert_eq!(
        snapshot["machine"]["capability"]["arch"],
        std::env::consts::ARCH
    );
    assert_eq!(
        snapshot["machine"]["capability"]["adapters"],
        json!(super::super::super::adapters::kinds())
    );
    assert!(snapshot["machine"]["capability"]["cpu"]["logical_cores"].is_u64());
    assert!(snapshot["machine"]["capability"]["memory"].is_object());
    assert!(snapshot["machine"]["capability"]["gpus"].is_array());
    assert!(snapshot["machine"]["occupancy"]["memory"].is_object());
    assert!(snapshot["machine"]["occupancy"]["gpus"].is_array());
    assert!(snapshot["machine"]["probes"]["gpus"].is_object());
    assert_eq!(snapshot["nodes"], json!([]));
    assert_eq!(snapshot["transport"]["receipts"]["records"], 0);
    assert_eq!(snapshot["transport"]["transfer"]["hop_data_writes"], 0);
    assert_eq!(snapshot["transport"]["transfer"]["hop_data_bytes"], 0);
    assert_eq!(snapshot["transport"]["failures"]["count"], 0);
    assert!(snapshot["generated_at_unix_ms"].as_u64().unwrap_or(0) > 0);
}

#[tokio::test]
async fn node_inspection_reports_request_completion_and_native_storage_separately() {
    use p4_adapter::node_adapter::{
        AdapterRetainedStorage, AdapterRetentionSnapshot, CompletionFront,
        CompletionStorageSnapshot, OwnedPoll, RetainedCompletion, RetainedNodeAdapter,
        RetainedOfferError,
    };
    use p4_agent_core::event_broker::RetainedEventBroker;
    use std::sync::Arc;
    use std::task::{Context, Poll};

    struct Adapter {
        completion: CompletionStorageSnapshot,
    }
    impl RetainedNodeAdapter for Adapter {
        fn try_offer_retained(
            &self,
            completion: RetainedCompletion,
        ) -> Result<(), RetainedOfferError> {
            Err(RetainedOfferError::Closed(completion))
        }
        fn peek_retained_completion(&self) -> Option<CompletionFront> {
            None
        }
        fn try_take_retained_matching(&self, _: &CompletionFront) -> OwnedPoll {
            OwnedPoll::Empty
        }
        fn poll_take_retained(&self, _: &mut Context<'_>) -> Poll<OwnedPoll> {
            Poll::Pending
        }
        fn snapshot(&self) -> String {
            "loaded".into()
        }
        fn completion_storage_snapshot(&self) -> Option<CompletionStorageSnapshot> {
            Some(self.completion)
        }
        fn retention_snapshot(&self) -> Option<AdapterRetentionSnapshot> {
            Some(AdapterRetentionSnapshot {
                pending_requests: AdapterRetainedStorage {
                    count: 3,
                    bytes: 4096,
                },
                native_responses: AdapterRetainedStorage {
                    count: 1,
                    bytes: 8192,
                },
            })
        }
    }

    let own = p4_protocol::Address::tcp("127.0.0.1", 53002);
    let mailbox =
        || p4_adapter::node_adapter::completion_mailbox_with_limits(1, 8, 1024 * 1024).unwrap();
    let (agent, _) = mailbox();
    let (outer, _) = mailbox();
    let (outbound, _) = mailbox();
    let broker = RetainedEventBroker::new(own.clone(), agent, outer, outbound, 8);
    let (_, inbound) = mailbox();
    let mut nodes = HashMap::new();
    nodes.insert(
        "snapshot-node".into(),
        super::super::NodeOwner {
            generation: 1,
            adapter_kind: "fixture".into(),
            adapter: Arc::new(Adapter {
                completion: CompletionStorageSnapshot {
                    capacity: 8,
                    queue_capacity: 1,
                    byte_limit: Some(1024 * 1024),
                    retained_count: 2,
                    retained_bytes: 16_384,
                    queued_count: 1,
                    reserved_queue_slots: 1,
                    queue_backing_bytes: 256,
                    closed: false,
                },
            }),
            inbound,
            task: tokio::spawn(async {
                std::future::pending::<
                    Result<(), p4_agent_core::event_node::RetainedEventNodeFailure>,
                >()
                .await
            }),
            lifecycle_phase: super::super::LifecyclePhase::Loaded,
            pending_lifecycle: None,
            admission_pause: None,
            last_lifecycle_result: None,
        },
    );
    let limits = crate::event_runtime::RuntimeLimits {
        queue: 1,
        retained: 8,
        bytes: 1024 * 1024,
        connections: 8,
        hop_receipts: 8,
        hop_receipt_bytes: 1024 * 1024,
        hop_outstanding: 4,
    };
    let transport = crate::event_runtime::transport::Inspector::detached(limits);
    let value = snapshot(&nodes, &broker, &transport).await;
    let retention = &value["nodes"][0]["retention"];
    assert_eq!(value["nodes"][0]["lifecycle_state"], "loaded");
    assert_eq!(retention["pending_requests"]["count"], 3);
    assert_eq!(retention["pending_requests"]["bytes"], 4096);
    assert_eq!(retention["completions"]["retained_count"], 2);
    assert_eq!(retention["completions"]["retained_bytes"], 16_384);
    assert_eq!(retention["native_responses"]["count"], 1);
    assert_eq!(retention["native_responses"]["bytes"], 8192);
}
