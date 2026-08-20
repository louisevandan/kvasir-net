use super::wire::*;
use super::*;

#[test]
fn every_agent_message_survives_a_round_trip() {
    for message in [
        ToAgent::CreateNode {
            node: "n0".into(),
            adapter: "llamacpp".into(),
        },
        ToAgent::DeleteNode { node: "n0".into() },
        ToAgent::Inspect,
        ToAgent::InspectModel {
            artifact: "model.gguf".into(),
            adapter: "mock".into(),
        },
        ToAgent::Cancel {
            route: "요청-7".into(),
            request_id: "요청-7".into(),
            stream_id: "stream-7".into(),
            return_channel: "outer-a".into(),
            generation: 3,
        },
        ToAgent::Status,
        ToAgent::Acknowledge {
            return_channel: "outer-a".into(),
            stream_id: "stream-7".into(),
            event_seq: 7,
        },
    ] {
        assert_eq!(
            decode_to_agent(&encode_to_agent(&message)).unwrap(),
            message
        );
    }
}

#[test]
fn every_node_message_survives_a_round_trip() {
    for message in [
        ToNode::Load {
            plan: r#"{"layers":"0-19"}"#.into(),
            artifact: "model.gguf".into(),
            ceiling: 10,
            capability_snapshot_id: String::new(),
            capability_expires_at: 0,
        },
        ToNode::Unload,
        ToNode::Execute {
            prompt: "안녕".into(),
            max_tokens: 500,
            options: r#"{"temperature":0.7}"#.into(),
        },
        ToNode::Continue {
            remaining: 500,
            emitted: 12,
            options: r#"{"temperature":0}"#.into(),
            state: vec![0, 1, 2, 250, 255],
        },
        ToNode::Persist {
            sequence: "s0".into(),
        },
        ToNode::PreparePersist {
            sequence: "s0".into(),
        },
        ToNode::Restore {
            sequence: "s0".into(),
        },
        ToNode::PrepareRestore {
            sequence: "s0".into(),
        },
        ToNode::PrepareDiscard {
            sequence: "s0".into(),
        },
        ToNode::Commit {
            sequence: "s0".into(),
        },
        ToNode::Abort {
            sequence: "s0".into(),
        },
        ToNode::Reconcile {
            sequence: "s0".into(),
        },
    ] {
        assert_eq!(decode_to_node(&encode_to_node(&message)).unwrap(), message);
    }
}

#[test]
fn every_reply_survives_a_round_trip() {
    for reply in [
        Reply::Accepted {
            detail: "queued".into(),
        },
        Reply::Progress {
            stage: 3,
            percent: 75,
        },
        Reply::Bound {
            generation: u64::MAX,
        },
        Reply::Released,
        Reply::Token {
            index: 41,
            text: "토큰".into(),
        },
        Reply::Done {
            reason: "stop".into(),
            generated: 500,
            final_token: None,
        },
        Reply::Failed {
            detail: "backend refused".into(),
        },
        Reply::CacheFailed {
            deployment: "deployment-a".into(),
            stage_id: "stage-1".into(),
            generation: 7,
            operation_id: "operation-7".into(),
            sequence: "sequence-7".into(),
            detail: "disk refused".into(),
        },
        Reply::Machine {
            snapshot: "{}".into(),
        },
        Reply::Model {
            artifact: "model.gguf".into(),
            adapter: "mock".into(),
            profile: r#"{"architecture":"mock","layers":2}"#.into(),
            capability_snapshot_id: "cap-test".into(),
            generated_at: 100,
            expires_at: 200,
        },
        Reply::Status {
            snapshot: "address=tcp://h:1\nnode=n0 depth=2 running=true routes=[a,b]".into(),
        },
        Reply::StatusSnapshot {
            snapshot: crate::status::StatusSnapshot {
                schema: 2,
                snapshot_seq: 7,
                generated_at_unix_ms: 123,
                address: "tcp://h:1".into(),
                traffic: crate::status::TrafficSnapshot {
                    forwarded: 1,
                    consumed: 2,
                    to_nodes: 3,
                    unrouted: 4,
                    refused: 5,
                    emergency_lost: 6,
                },
                lanes: crate::status::LaneSnapshot {
                    control: 1,
                    prefill: 2,
                    decode: 3,
                    response: 4,
                },
                peers: 2,
                continuations: 8,
                subscription_pending: 1,
                subscription_unacked: 2,
                subscription_dropped: 3,
                subscription_ack_rejected: 4,
                nodes: vec![crate::status::NodeSnapshot {
                    node: "n0".into(),
                    depth: 2,
                    running: 1,
                    outbox_lost: 0,
                    waiting: vec!["route".into()],
                    backend: "opaque".into(),
                    waiting_requests: vec![],
                    active_hop: None,
                }],
            },
        },
        Reply::Cached {
            deployment: "deployment-a".into(),
            stage_id: "stage-1".into(),
            generation: 7,
            operation_id: "operation-7".into(),
            sequence: "sequence-7".into(),
            bytes: 4096,
            detail: "persisted".into(),
        },
        Reply::CacheStatus {
            deployment: "deployment-a".into(),
            stage_id: "stage-1".into(),
            generation: 7,
            operation_id: "operation-7".into(),
            sequence: "sequence-7".into(),
            state: "committed".into(),
            bytes: 4096,
            detail: "receipt state=committed".into(),
        },
    ] {
        assert_eq!(decode_reply(&encode_reply(&reply)).unwrap(), reply);
    }
}

#[test]
fn a_schema_one_status_snapshot_decodes_without_the_schema_two_counter() {
    let legacy = Reply::StatusSnapshot {
        snapshot: crate::status::StatusSnapshot {
            schema: 1,
            snapshot_seq: 3,
            generated_at_unix_ms: 4,
            address: "tcp://legacy:1".into(),
            traffic: crate::status::TrafficSnapshot {
                forwarded: 5,
                consumed: 6,
                to_nodes: 7,
                unrouted: 8,
                refused: 9,
                emergency_lost: 10,
            },
            lanes: crate::status::LaneSnapshot {
                control: 11,
                prefill: 12,
                decode: 13,
                response: 14,
            },
            peers: 15,
            continuations: 16,
            subscription_pending: 17,
            subscription_unacked: 18,
            subscription_dropped: 19,
            subscription_ack_rejected: 0,
            nodes: vec![],
        },
    };
    let decoded = decode_reply(&encode_reply(&legacy)).unwrap();
    assert_eq!(decoded, legacy);
}

#[test]
fn an_unknown_status_schema_is_rejected_before_reading_its_layout() {
    for schema in [0, 7] {
        let future = Reply::StatusSnapshot {
            snapshot: crate::status::StatusSnapshot {
                schema,
                snapshot_seq: 1,
                generated_at_unix_ms: 2,
                address: "tcp://future:1".into(),
                traffic: crate::status::TrafficSnapshot {
                    forwarded: 0,
                    consumed: 0,
                    to_nodes: 0,
                    unrouted: 0,
                    refused: 0,
                    emergency_lost: 0,
                },
                lanes: crate::status::LaneSnapshot {
                    control: 0,
                    prefill: 0,
                    decode: 0,
                    response: 0,
                },
                peers: 0,
                continuations: 0,
                subscription_pending: 0,
                subscription_unacked: 0,
                subscription_dropped: 0,
                subscription_ack_rejected: 7,
                nodes: vec![],
            },
        };
        let encoded = encode_reply(&future);
        let error = decode_reply(&encoded).expect_err("unsupported status schema must fail closed");
        assert!(
            error
                .0
                .contains(&format!("unsupported status schema {schema}"))
        );
    }
}

#[test]
fn a_schema_four_status_snapshot_round_trips_active_hop_phase_and_requests() {
    let expected = Reply::StatusSnapshot {
        snapshot: crate::status::StatusSnapshot {
            schema: 4,
            snapshot_seq: 10,
            generated_at_unix_ms: 11,
            address: "tcp://active:1".into(),
            traffic: crate::status::TrafficSnapshot {
                forwarded: 0,
                consumed: 0,
                to_nodes: 0,
                unrouted: 0,
                refused: 0,
                emergency_lost: 0,
            },
            lanes: crate::status::LaneSnapshot {
                control: 0,
                prefill: 0,
                decode: 1,
                response: 0,
            },
            peers: 0,
            continuations: 0,
            subscription_pending: 0,
            subscription_unacked: 0,
            subscription_dropped: 0,
            subscription_ack_rejected: 0,
            nodes: vec![crate::status::NodeSnapshot {
                node: "n0".into(),
                depth: 0,
                running: 1,
                outbox_lost: 0,
                waiting: vec![],
                backend: "mock".into(),
                waiting_requests: vec![],
                active_hop: Some(crate::status::ActiveHopSnapshot {
                    id: 77,
                    lane: p4_protocol::QueueClass::Decode,
                    timed_out: false,
                    requests: vec![crate::status::RequestSnapshot {
                        route: "active-route".into(),
                        request_id: "active-request".into(),
                        stream_id: "active-stream".into(),
                        lane: p4_protocol::QueueClass::Decode,
                        deadline_unix_ms: 300,
                    }],
                }),
            }],
        },
    };
    let decoded = decode_reply(&encode_reply(&expected)).unwrap();
    assert_eq!(decoded, expected);
}

#[test]
fn a_schema_five_status_snapshot_round_trips_timed_out_active_hop() {
    let expected = Reply::StatusSnapshot {
        snapshot: crate::status::StatusSnapshot {
            schema: 5,
            snapshot_seq: 12,
            generated_at_unix_ms: 13,
            address: "tcp://timeout:1".into(),
            traffic: crate::status::TrafficSnapshot {
                forwarded: 0,
                consumed: 0,
                to_nodes: 0,
                unrouted: 0,
                refused: 0,
                emergency_lost: 0,
            },
            lanes: crate::status::LaneSnapshot {
                control: 0,
                prefill: 0,
                decode: 1,
                response: 0,
            },
            peers: 0,
            continuations: 0,
            subscription_pending: 0,
            subscription_unacked: 0,
            subscription_dropped: 0,
            subscription_ack_rejected: 0,
            nodes: vec![crate::status::NodeSnapshot {
                node: "n0".into(),
                depth: 0,
                running: 1,
                outbox_lost: 0,
                waiting: vec![],
                backend: "mock".into(),
                waiting_requests: vec![],
                active_hop: Some(crate::status::ActiveHopSnapshot {
                    id: 88,
                    lane: p4_protocol::QueueClass::Prefill,
                    timed_out: true,
                    requests: vec![],
                }),
            }],
        },
    };
    let decoded = decode_reply(&encode_reply(&expected)).unwrap();
    assert_eq!(decoded, expected);
}

#[test]
fn a_schema_five_rejects_an_unknown_timeout_marker() {
    let reply = Reply::StatusSnapshot {
        snapshot: crate::status::StatusSnapshot {
            schema: 5,
            snapshot_seq: 1,
            generated_at_unix_ms: 2,
            address: "tcp://timeout-marker:1".into(),
            traffic: crate::status::TrafficSnapshot {
                forwarded: 0,
                consumed: 0,
                to_nodes: 0,
                unrouted: 0,
                refused: 0,
                emergency_lost: 0,
            },
            lanes: crate::status::LaneSnapshot {
                control: 0,
                prefill: 0,
                decode: 0,
                response: 0,
            },
            peers: 0,
            continuations: 0,
            subscription_pending: 0,
            subscription_unacked: 0,
            subscription_dropped: 0,
            subscription_ack_rejected: 0,
            nodes: vec![crate::status::NodeSnapshot {
                node: "n0".into(),
                depth: 0,
                running: 1,
                outbox_lost: 0,
                waiting: vec![],
                backend: "mock".into(),
                waiting_requests: vec![],
                active_hop: Some(crate::status::ActiveHopSnapshot {
                    id: 1,
                    lane: p4_protocol::QueueClass::Decode,
                    timed_out: false,
                    requests: vec![],
                }),
            }],
        },
    };
    let mut encoded = encode_reply(&reply);
    *encoded.last_mut().expect("timeout marker exists") = 2;
    assert!(
        decode_reply(&encoded)
            .expect_err("unknown timeout marker must fail closed")
            .0
            .contains("invalid active hop timeout marker")
    );
}

#[test]
fn a_schema_six_reports_node_outbox_loss() {
    let expected = Reply::StatusSnapshot {
        snapshot: crate::status::StatusSnapshot {
            schema: 6,
            snapshot_seq: 21,
            generated_at_unix_ms: 22,
            address: "tcp://loss:1".into(),
            traffic: crate::status::TrafficSnapshot {
                forwarded: 0,
                consumed: 0,
                to_nodes: 0,
                unrouted: 0,
                refused: 0,
                emergency_lost: 0,
            },
            lanes: crate::status::LaneSnapshot {
                control: 0,
                prefill: 0,
                decode: 0,
                response: 0,
            },
            peers: 0,
            continuations: 0,
            subscription_pending: 0,
            subscription_unacked: 0,
            subscription_dropped: 0,
            subscription_ack_rejected: 0,
            nodes: vec![crate::status::NodeSnapshot {
                node: "n0".into(),
                depth: 0,
                running: 0,
                outbox_lost: 7,
                waiting: vec![],
                backend: "mock".into(),
                waiting_requests: vec![],
                active_hop: None,
            }],
        },
    };
    assert_eq!(decode_reply(&encode_reply(&expected)).unwrap(), expected);
}

#[test]
fn a_schema_three_status_snapshot_round_trips_waiting_request_identity() {
    let snapshot = crate::status::StatusSnapshot {
        schema: 3,
        snapshot_seq: 8,
        generated_at_unix_ms: 9,
        address: "tcp://status:1".into(),
        traffic: crate::status::TrafficSnapshot {
            forwarded: 0,
            consumed: 0,
            to_nodes: 0,
            unrouted: 0,
            refused: 0,
            emergency_lost: 0,
        },
        lanes: crate::status::LaneSnapshot {
            control: 0,
            prefill: 1,
            decode: 0,
            response: 0,
        },
        peers: 0,
        continuations: 0,
        subscription_pending: 0,
        subscription_unacked: 0,
        subscription_dropped: 0,
        subscription_ack_rejected: 0,
        nodes: vec![crate::status::NodeSnapshot {
            node: "n0".into(),
            depth: 1,
            running: 0,
            outbox_lost: 0,
            waiting: vec!["route-1".into()],
            backend: "mock".into(),
            waiting_requests: vec![
                crate::status::RequestSnapshot {
                    route: "route-1".into(),
                    request_id: "request-1".into(),
                    stream_id: "stream-1".into(),
                    lane: p4_protocol::QueueClass::Prefill,
                    deadline_unix_ms: 100,
                },
                crate::status::RequestSnapshot {
                    route: "route-2".into(),
                    request_id: "request-2".into(),
                    stream_id: "stream-2".into(),
                    lane: p4_protocol::QueueClass::Decode,
                    deadline_unix_ms: 200,
                },
            ],
            active_hop: None,
        }],
    };
    let expected = Reply::StatusSnapshot { snapshot };
    let decoded = decode_reply(&encode_reply(&expected)).unwrap();
    assert_eq!(decoded, expected);
}

#[test]
fn an_empty_body_is_refused_rather_than_read_as_a_variant() {
    assert!(decode_to_agent(&[]).is_err());
    assert!(decode_to_node(&[]).is_err());
    assert!(decode_reply(&[]).is_err());
}

#[test]
fn an_unknown_tag_is_refused() {
    assert!(decode_to_agent(&[200]).is_err());
    assert!(decode_to_node(&[200]).is_err());
    assert!(decode_reply(&[200]).is_err());
}

#[test]
fn a_truncated_body_is_refused_at_every_length() {
    let bytes = encode_to_node(&ToNode::Load {
        plan: "plan".into(),
        artifact: "artifact".into(),
        ceiling: 4,
        capability_snapshot_id: String::new(),
        capability_expires_at: 0,
    });
    for cut in 1..bytes.len() {
        assert!(
            decode_to_node(&bytes[..cut]).is_err(),
            "prefix {cut} decoded"
        );
    }
}

#[test]
fn a_truncated_cache_failure_is_refused_at_every_length() {
    let bytes = encode_reply(&Reply::CacheFailed {
        deployment: "deployment-a".into(),
        stage_id: "stage-1".into(),
        generation: 7,
        operation_id: "operation-7".into(),
        sequence: "sequence-7".into(),
        detail: "disk refused".into(),
    });
    for cut in 1..bytes.len() {
        assert!(
            decode_reply(&bytes[..cut]).is_err(),
            "cache failure prefix {cut} decoded"
        );
    }
}

#[test]
fn a_truncated_cache_status_is_refused_at_every_length() {
    let bytes = encode_reply(&Reply::CacheStatus {
        deployment: "deployment-a".into(),
        stage_id: "stage-1".into(),
        generation: 7,
        operation_id: "operation-7".into(),
        sequence: "sequence-7".into(),
        state: "committed".into(),
        bytes: 4096,
        detail: "receipt state=committed".into(),
    });
    for cut in 1..bytes.len() {
        assert!(
            decode_reply(&bytes[..cut]).is_err(),
            "cache status prefix {cut} decoded"
        );
    }
}

#[test]
fn trailing_bytes_are_refused() {
    // A sender and a reader disagreeing about the shape is worth failing on
    // rather than ignoring the difference.
    let mut bytes = encode_to_agent(&ToAgent::Inspect);
    bytes.push(0);
    assert!(decode_to_agent(&bytes).is_err());
}

#[test]
fn cache_failure_trailing_bytes_are_refused() {
    let mut bytes = encode_reply(&Reply::CacheFailed {
        deployment: "deployment-a".into(),
        stage_id: "stage-1".into(),
        generation: 7,
        operation_id: "operation-7".into(),
        sequence: "sequence-7".into(),
        detail: "disk refused".into(),
    });
    bytes.push(0);
    assert!(decode_reply(&bytes).is_err());
}

#[test]
fn cache_status_trailing_bytes_are_refused() {
    let mut bytes = encode_reply(&Reply::CacheStatus {
        deployment: "deployment-a".into(),
        stage_id: "stage-1".into(),
        generation: 7,
        operation_id: "operation-7".into(),
        sequence: "sequence-7".into(),
        state: "committed".into(),
        bytes: 4096,
        detail: "receipt state=committed".into(),
    });
    bytes.push(0);
    assert!(decode_reply(&bytes).is_err());
}

#[test]
fn tags_are_fixed_so_reordering_a_variant_cannot_change_the_wire() {
    assert_eq!(encode_to_agent(&ToAgent::Inspect)[0], 3);
    assert_eq!(encode_to_node(&ToNode::Unload)[0], 17);
    assert_eq!(encode_reply(&Reply::Released)[0], 35);
}

#[test]
fn a_body_carrying_no_text_is_a_single_byte() {
    // The tokens of a stream are the most frequent frames in the system, so
    // the empty cases staying small is worth checking.
    assert_eq!(encode_to_agent(&ToAgent::Inspect).len(), 1);
    assert_eq!(encode_to_node(&ToNode::Unload).len(), 1);
    assert_eq!(encode_reply(&Reply::Released).len(), 1);
}
