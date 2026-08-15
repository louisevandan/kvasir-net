use super::*;
use crate::{Allocation, ExecutionRequest, Message, Phase};

fn request() -> Message {
    Message::Execute(ExecutionRequest {
        controller_id: "c".into(),
        node_id: "n".into(),
        deployment_id: "d".into(),
        binding_id: "b".into(),
        runtime_generation: 1,
        request_id: "r".into(),
        session_id: "s".into(),
        phase: Phase::Prefill,
        position: 0,
        max_tokens: 4,
        temperature: 0.2,
        prompt: "hello".into(),
        options: r#"{"top_p":0.9,"top_k":40}"#.into(),
    })
}

#[test]
fn round_trips_execution() {
    let mut bytes = Vec::new();
    let expected = request();
    write_message(&mut bytes, &expected).unwrap();
    assert_eq!(read_message(&mut bytes.as_slice()).unwrap(), expected);
}

#[test]
fn preserves_transport_route_independently_from_business_request_id() {
    let expected =
        RoutedMessage::new("controller-a:route-7", 1_800_000_000_000, request()).unwrap();
    let bytes = encode_routed_message(&expected).unwrap();
    assert_eq!(decode_routed_message(&bytes).unwrap(), expected);
}

#[test]
fn round_trips_draft_report() {
    let expected = Message::DraftReport {
        operation_id: "load-1".into(),
        node_id: "n".into(),
        total_bytes: 30,
        allocations: vec![
            Allocation {
                category: "stage0.context_reserved".into(),
                bytes: 10,
            },
            Allocation {
                category: "stage1.context_reserved".into(),
                bytes: 20,
            },
        ],
        detail: "measured".into(),
    };
    let mut bytes = Vec::new();
    write_message(&mut bytes, &expected).unwrap();
    assert_eq!(read_message(&mut bytes.as_slice()).unwrap(), expected);
}

#[test]
fn a_draft_report_carries_no_allocations_when_nothing_was_reserved() {
    let expected = Message::DraftReport {
        operation_id: "load-1".into(),
        node_id: "n".into(),
        total_bytes: 0,
        allocations: Vec::new(),
        detail: "nothing reported".into(),
    };
    let mut bytes = Vec::new();
    write_message(&mut bytes, &expected).unwrap();
    assert_eq!(read_message(&mut bytes.as_slice()).unwrap(), expected);
}

#[test]
fn an_allocation_count_past_the_ceiling_is_refused_rather_than_allocated() {
    let expected = Message::DraftReport {
        operation_id: "load-1".into(),
        node_id: "n".into(),
        total_bytes: 0,
        allocations: (0..257)
            .map(|index| Allocation {
                category: format!("stage{index}"),
                bytes: 0,
            })
            .collect(),
        detail: String::new(),
    };
    let mut bytes = Vec::new();
    assert!(write_message(&mut bytes, &expected).is_err());
}

#[test]
fn round_trips_correlated_health() {
    let expected = Message::Health {
        request_id: "health-42".into(),
        node_id: "node-a".into(),
        ready: true,
        detail: "ready".into(),
    };
    let mut bytes = Vec::new();
    write_message(&mut bytes, &expected).unwrap();
    assert_eq!(read_message(&mut bytes.as_slice()).unwrap(), expected);
}

#[test]
fn distinguishes_a_clean_peer_close_from_a_truncated_frame() {
    assert!(
        read_message(&mut [].as_slice())
            .unwrap_err()
            .is_peer_closed()
    );
    assert!(
        !read_message(&mut [b'P'].as_slice())
            .unwrap_err()
            .is_peer_closed()
    );
}
