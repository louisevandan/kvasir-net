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
            session_epoch: 42,
        },
        ToNode::Continue {
            remaining: 500,
            emitted: 12,
            options: r#"{"temperature":0}"#.into(),
            state: vec![0, 1, 2, 250, 255],
            session_epoch: 42,
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
        ToNode::SessionClose {
            sequence: "s0".into(),
            close_id: 7,
            session_epoch: 42,
        },
        ToNode::SessionClosed {
            sequence: "s0".into(),
            close_id: 7,
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
                    lane: crate::status::ActiveHopLane::Decode,
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
                    lane: crate::status::ActiveHopLane::Prefill,
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
                    lane: crate::status::ActiveHopLane::Decode,
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

/// Schema 6 is a shipped wire format. Its active-hop lane byte has meant
/// `Prefill -> 0, Decode -> 1` since before `QueueClass` existed, and a peer
/// already built against schema 6 depends on exactly that. This test pins
/// the byte directly rather than only round-tripping, because a round trip
/// alone stays green even if encode and decode drift to the same wrong byte
/// together — which is precisely how the regression this test guards against
/// slipped in once already (commit 1aa011086 quietly moved this field onto
/// the four-value `QueueClass` codec while schema stayed at 6).
#[test]
fn schema_six_active_hop_lane_pins_the_shipped_zero_one_wire_bytes() {
    let build = |lane: crate::status::ActiveHopLane| Reply::StatusSnapshot {
        snapshot: crate::status::StatusSnapshot {
            schema: 6,
            snapshot_seq: 1,
            generated_at_unix_ms: 2,
            address: "tcp://lane-pin:1".into(),
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
                outbox_lost: 0,
                waiting: vec![],
                backend: "mock".into(),
                waiting_requests: vec![],
                active_hop: Some(crate::status::ActiveHopSnapshot {
                    id: 5,
                    lane,
                    timed_out: false,
                    requests: vec![],
                }),
            }],
        },
    };

    let prefill = encode_reply(&build(crate::status::ActiveHopLane::Prefill));
    let decode = encode_reply(&build(crate::status::ActiveHopLane::Decode));

    // The two snapshots are identical apart from the lane, so they must be
    // the same length and differ at exactly one byte: the lane tag itself.
    // Finding that byte by diffing survives an unrelated field being added
    // ahead of it in the snapshot, where a hardcoded offset would silently
    // start comparing the wrong byte instead of failing.
    assert_eq!(prefill.len(), decode.len());
    let differences: Vec<(usize, u8, u8)> = prefill
        .iter()
        .zip(decode.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, (&a, &b))| (index, a, b))
        .collect();
    assert_eq!(
        differences.len(),
        1,
        "prefill and decode snapshots must differ in exactly one byte"
    );
    let (lane_index, prefill_byte, decode_byte) = differences[0];
    assert_eq!((prefill_byte, decode_byte), (0, 1));

    // Tag 2 in that same position must be rejected outright, not silently
    // reinterpreted as `Control` the way the shared four-value lane codec
    // (`status_lane_tag`/`status_lane`, used for `RequestSnapshot::lane`)
    // would read it.
    let mut poisoned = prefill.clone();
    poisoned[lane_index] = 2;
    assert!(
        decode_reply(&poisoned)
            .expect_err("tag 2 is outside the active hop lane's two-value domain")
            .0
            .contains("unknown active hop lane 2")
    );

    // Both values round-trip.
    let Reply::StatusSnapshot { snapshot } = decode_reply(&prefill).unwrap() else {
        panic!("expected a status snapshot reply");
    };
    assert_eq!(
        snapshot.nodes[0].active_hop.as_ref().unwrap().lane,
        crate::status::ActiveHopLane::Prefill
    );
    let Reply::StatusSnapshot { snapshot } = decode_reply(&decode).unwrap() else {
        panic!("expected a status snapshot reply");
    };
    assert_eq!(
        snapshot.nodes[0].active_hop.as_ref().unwrap().lane,
        crate::status::ActiveHopLane::Decode
    );
}

/// A schema 4 `StatusSnapshot` byte-for-byte as a genuinely older binary
/// would have written it, built by hand from the wire layout rather than
/// through the current encoder -- so this does not merely prove the current
/// encoder and decoder agree with each other, which the round-trip tests
/// above already do. `ActiveHopSnapshot::lane`'s byte has meant
/// `Prefill -> 0, Decode -> 1` since before `QueueClass` existed, and
/// `active_hop_lane_tag`/`active_hop_lane` keep exactly that mapping, so
/// bytes shaped like this are what a pre-fix peer actually put on the wire.
#[test]
fn a_hand_built_old_shaped_schema_four_snapshot_decodes_to_the_right_lane() {
    fn raw_schema_four_snapshot(active_hop_lane_byte: u8) -> Vec<u8> {
        let mut body = vec![43u8]; // STATUS_SNAPSHOT tag
        body.extend_from_slice(&4u32.to_le_bytes()); // schema
        body.extend_from_slice(&1u64.to_le_bytes()); // snapshot_seq
        body.extend_from_slice(&2u64.to_le_bytes()); // generated_at_unix_ms
        body.extend_from_slice(&0u32.to_le_bytes()); // address (empty text)
        for _ in 0..16 {
            // 15 traffic/lane/peer/continuation/subscription counters, plus
            // the schema>=2 ack-rejected counter: all zero.
            body.extend_from_slice(&0u64.to_le_bytes());
        }
        body.extend_from_slice(&1u32.to_le_bytes()); // node_count = 1
        body.extend_from_slice(&0u32.to_le_bytes()); // node name (empty text)
        body.extend_from_slice(&0u64.to_le_bytes()); // depth
        body.extend_from_slice(&0u64.to_le_bytes()); // running
        body.extend_from_slice(&0u32.to_le_bytes()); // backend (empty text)
        body.extend_from_slice(&0u32.to_le_bytes()); // waiting.len() = 0
        body.extend_from_slice(&0u32.to_le_bytes()); // waiting_requests.len() = 0 (schema >= 3)
        body.push(1); // active_hop marker: Some (schema >= 4)
        body.extend_from_slice(&5u64.to_le_bytes()); // active_hop.id
        body.push(active_hop_lane_byte); // active_hop.lane, old-style 0/1
        body.extend_from_slice(&0u32.to_le_bytes()); // active_hop.requests.len() = 0
        body
    }

    let Reply::StatusSnapshot { snapshot } = decode_reply(&raw_schema_four_snapshot(0)).unwrap()
    else {
        panic!("expected a status snapshot reply");
    };
    assert_eq!(
        snapshot.nodes[0].active_hop.as_ref().unwrap().lane,
        crate::status::ActiveHopLane::Prefill
    );

    let Reply::StatusSnapshot { snapshot } = decode_reply(&raw_schema_four_snapshot(1)).unwrap()
    else {
        panic!("expected a status snapshot reply");
    };
    assert_eq!(
        snapshot.nodes[0].active_hop.as_ref().unwrap().lane,
        crate::status::ActiveHopLane::Decode
    );
}

#[test]
fn a_body_carrying_no_text_is_a_single_byte() {
    // The tokens of a stream are the most frequent frames in the system, so
    // the empty cases staying small is worth checking.
    assert_eq!(encode_to_agent(&ToAgent::Inspect).len(), 1);
    assert_eq!(encode_to_node(&ToNode::Unload).len(), 1);
    assert_eq!(encode_reply(&Reply::Released).len(), 1);
}
