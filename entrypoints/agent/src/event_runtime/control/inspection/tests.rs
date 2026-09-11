use super::*;

#[tokio::test]
async fn inspection_counts_retained_receipts_through_the_actual_control_loop() {
    use p4_agent_core::event_broker::{EventBroker, bounded_queue};
    use p4_protocol::{Address, event::{Endpoint, Event, EventClass, AGENT_INSPECT_CONTENT_TYPE}};
    use std::sync::Arc;
    let own = Address::tcp("127.0.0.1", 53120);
    let remote = Address::tcp("127.0.0.2", 53120);
    let (agent_tx, agent_rx) = bounded_queue(1);
    let (outer_tx, mut outer_rx) = bounded_queue(1);
    let (outbound_tx, mut outbound_rx) = bounded_queue(1);
    let broker = Arc::new(EventBroker::new(own.clone(), agent_tx, outer_tx, outbound_tx, 8));
    let route = Endpoint::outer(own.clone(), "inspection-test", 1);
    let mut event = Event {
        envelope: Envelope {
            protocol_version: Envelope::VERSION,
            event_id: "retained-input".into(), correlation_id: "retained-probe".into(),
            causation_id: None, source: route.clone(), target: Endpoint::agent(remote),
            return_route: match route { Endpoint::Outer(ref outer) => Some(outer.clone()), _ => unreachable!() },
            class: EventClass::Data, sequence: 1, deadline_unix_ms: None, adapter_kind: None,
            payload_content_type: "application/octet-stream".into(),
        },
        payload: vec![42; 4096],
    };
    let first_cost = p4_adapter::node_adapter::retained_event_bytes(&event.clone()).unwrap();
    broker.dispatch(event.clone()).unwrap();
    drop(outbound_rx.recv().await.unwrap()); // Destination consumption does not free the receipt.
    event.envelope.event_id = "inspect-receipts".into();
    event.envelope.target = Endpoint::agent(own.clone());
    event.envelope.class = EventClass::Control;
    event.envelope.sequence = 2;
    event.envelope.payload_content_type = AGENT_INSPECT_CONTENT_TYPE.into();
    event.payload = b"{}".to_vec();
    let second_cost = p4_adapter::node_adapter::retained_event_bytes(&event.clone()).unwrap();
    let task = tokio::spawn(super::super::run(own, Arc::clone(&broker), agent_rx));
    broker.dispatch(event).unwrap();
    let reply = tokio::time::timeout(std::time::Duration::from_secs(20), outer_rx.recv())
        .await.unwrap().unwrap();
    task.abort();
    let value: Value = serde_json::from_slice(&reply.payload).unwrap();
    assert_eq!(value["broker"]["receipts"]["indexed"]["events"], 2);
    assert_eq!(value["broker"]["receipts"]["indexed"]["event_bytes"], first_cost + second_cost);
    assert_eq!(value["broker"]["receipts"]["indexed"]["payload_capacity_bytes"], 4098);
    assert_eq!(value["broker"]["receipts"]["retired"]["events"], 0);
    assert_eq!(value["broker"]["receipts"]["committed_events"], 2);
    assert!(value["broker"]["sampled_at_unix_ms"].is_u64());
}

#[tokio::test]
async fn an_empty_agent_still_reports_machine_and_protocol_identity() {
    use p4_agent_core::event_broker::bounded_queue;
    let (agent, _agent_rx) = bounded_queue(1);
    let (outer, _outer_rx) = bounded_queue(1);
    let (outbound, _outbound_rx) = bounded_queue(1);
    let broker = EventBroker::new(p4_protocol::Address::tcp("127.0.0.1", 53001), agent, outer, outbound, 8);
    let snapshot = snapshot(&HashMap::new(), &broker).await;

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
        json!(["llamacpp"])
    );
    assert!(snapshot["machine"]["capability"]["cpu"]["logical_cores"].is_u64());
    assert!(snapshot["machine"]["capability"]["memory"].is_object());
    assert!(snapshot["machine"]["capability"]["gpus"].is_array());
    assert!(snapshot["machine"]["occupancy"]["memory"].is_object());
    assert!(snapshot["machine"]["occupancy"]["gpus"].is_array());
    assert!(snapshot["machine"]["probes"]["gpus"].is_object());
    assert_eq!(snapshot["nodes"], json!([]));
    assert!(snapshot["generated_at_unix_ms"].as_u64().unwrap_or(0) > 0);
}
