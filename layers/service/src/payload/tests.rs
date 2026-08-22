use super::*;
use crate::capability::{Capability, CapabilityRegistry};
use crate::message::Reply;
use crate::message::wire::{decode_reply, encode_reply, encode_to_node};
use p4_adapter::CacheAction;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};

fn frame(body: Vec<u8>) -> Frame {
    let chain = Chain::new(vec![Link {
        address: Address::tcp("127.0.0.1", 19001),
        node: "n0".into(),
        binding: "deployment-a".into(),
        generation: 1,
    }])
    .unwrap();
    Frame {
        envelope: Envelope {
            target: Address::tcp("127.0.0.1", 19001),
            recipient: Recipient::node("n0"),
            lane: QueueClass::Prefill,
            route: "route-7".into(),
            request_id: "request-7".into(),
            stream_id: "stream-7".into(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: None,
            chain: Some(chain),
        },
        body,
    }
}

#[test]
fn an_execute_body_becomes_a_sequence_named_by_its_request() {
    let body = encode_to_node(&ToNode::Execute {
        prompt: "안녕".into(),
        max_tokens: 64,
        options: r#"{"temperature":0.2}"#.into(),
        session_epoch: 1,
    });
    let sequence = Bodies::default()
        .sequence(&frame(body))
        .expect("executable");

    assert_eq!(sequence.sequence, "request-7");
    assert_eq!(sequence.prompt.as_deref(), Some("안녕"));
    assert_eq!(sequence.remaining, 64);
    assert_eq!(sequence.options, r#"{"temperature":0.2}"#);
    assert_eq!(
        sequence.session_epoch, 1,
        "the adapter boundary must see the same session identity the wire carries, \
         not silently drop it"
    );
}

#[test]
fn sequence_identity_does_not_follow_a_reused_transport_route() {
    let body = encode_to_node(&ToNode::Execute {
        prompt: "p".into(),
        max_tokens: 4,
        options: "{}".into(),
        session_epoch: 1,
    });
    let mut request = frame(body);
    request.envelope.route = "reused-route".into();
    request.envelope.request_id = "request-stable".into();

    let sequence = Bodies::default().sequence(&request).expect("executable");
    assert_eq!(sequence.sequence, "request-stable");
}

#[test]
fn a_continuation_hands_the_adapter_its_own_state_back() {
    let body = encode_to_node(&ToNode::Continue {
        remaining: 64,
        emitted: 3,
        options: r#"{"temperature":0.2}"#.into(),
        state: vec![1, 2, 3],
        session_epoch: 1,
    });
    let sequence = Bodies::default()
        .sequence(&frame(body))
        .expect("continuation is executable");
    // Byte for byte, and no prompt: a later stage continues from what it
    // wrote, not from text it would have to tokenize again.
    assert_eq!(sequence.state, Some(vec![1, 2, 3]));
    assert!(sequence.prompt.is_none());
    assert_eq!(sequence.remaining, 64);
    assert_eq!(sequence.options, r#"{"temperature":0.2}"#);
}

#[test]
fn a_continuation_without_state_is_still_executable() {
    let body = encode_to_node(&ToNode::Continue {
        remaining: 64,
        emitted: 12,
        options: "{}".into(),
        state: Vec::new(),
        session_epoch: 1,
    });
    let sequence = Bodies::default()
        .sequence(&frame(body))
        .expect("continuation is executable");
    // An adapter that needs nothing to continue says so by writing nothing,
    // and P4 has no opinion about the difference.
    assert_eq!(sequence.state, Some(Vec::new()));
    assert!(sequence.prompt.is_none());
    assert_eq!(sequence.remaining, 64);
}

#[test]
fn a_malformed_cut_set_wrapper_is_refused_instead_of_read_as_a_node_message() {
    let mut body = b"P4CUT01\0".to_vec();
    body.extend_from_slice(&[0, 0, 0, 0]);
    assert!(Bodies::default().sequence(&frame(body)).is_none());
}

#[test]
fn a_load_body_is_lifecycle_rather_than_a_sequence() {
    // Lifecycle never batches, so the two readings must not overlap.
    let body = encode_to_node(&ToNode::Load {
        plan: r#"{"layers":"0-19"}"#.into(),
        artifact: "model.gguf".into(),
        ceiling: 10,
        capability_snapshot_id: String::new(),
        capability_expires_at: 0,
    });
    let frame = frame(body);

    assert!(Bodies::default().sequence(&frame).is_none());
    let Some(Work::Load(load)) = Bodies::default().lifecycle(&frame) else {
        panic!("a load");
    };
    assert_eq!(load.deployment, "deployment-a");
    assert_eq!(load.artifact, "model.gguf");
    // The plan is opaque: it travels whole and nothing here reads inside it.
    assert_eq!(load.plan, r#"{"layers":"0-19"}"#);
}

#[test]
fn the_ceiling_comes_from_the_load_that_declared_it() {
    let body = encode_to_node(&ToNode::Load {
        plan: "{}".into(),
        artifact: "m".into(),
        ceiling: 24,
        capability_snapshot_id: String::new(),
        capability_expires_at: 0,
    });
    assert_eq!(Bodies::default().ceiling(&frame(body)), Some(24));
}

#[test]
fn an_execute_declares_no_ceiling() {
    let body = encode_to_node(&ToNode::Execute {
        prompt: "p".into(),
        max_tokens: 1,
        options: "{}".into(),
        session_epoch: 1,
    });
    assert_eq!(Bodies::default().ceiling(&frame(body.clone())), None);
    assert!(Bodies::default().lifecycle(&frame(body)).is_none());
}

#[test]
fn a_session_close_carries_its_epoch_into_work_close() {
    // `session_epoch` is on the wire (frame v8's `SessionClose` body) purely
    // so the receiving adapter can tell a redelivered close for a session
    // that has since released and been reused apart from one that still
    // owns its reservation. Dropping it here -- as this conversion used to
    // -- means the adapter never gets to make that distinction at all.
    let body = encode_to_node(&ToNode::SessionClose {
        sequence: "seq-1".into(),
        close_id: 7,
        session_epoch: 42,
    });
    let Some(Work::Close(close)) = Bodies::default().lifecycle(&frame(body)) else {
        panic!("a session close");
    };
    assert_eq!(close.sequence, "seq-1");
    assert_eq!(close.session_epoch, 42);
}

#[test]
fn an_unload_is_lifecycle() {
    let frame = frame(encode_to_node(&ToNode::Unload));
    assert!(matches!(
        Bodies::default().lifecycle(&frame),
        Some(Work::Unload(_))
    ));
}

#[test]
fn an_unreadable_body_is_refused_rather_than_guessed_at() {
    // Malformed work reaching a backend is how a protocol fault turns into a
    // crash somewhere it cannot be traced.
    let frame = frame(vec![200, 1, 2, 3]);
    assert!(Bodies::default().sequence(&frame).is_none());
    assert!(Bodies::default().lifecycle(&frame).is_none());
    assert!(Bodies::default().ceiling(&frame).is_none());
}

#[test]
fn the_deployment_comes_from_the_chain_rather_than_the_body() {
    // So a body cannot disagree with the route it travelled.
    let frame = frame(encode_to_node(&ToNode::Unload));
    assert_eq!(
        Bodies::default().deployment(&frame).as_deref(),
        Some("deployment-a")
    );
}

#[test]
fn an_expired_discovery_snapshot_refuses_load_before_the_adapter() {
    let body = encode_to_node(&ToNode::Load {
        plan: "{}".into(),
        artifact: "model.gguf".into(),
        ceiling: 1,
        capability_snapshot_id: "cap-old".into(),
        capability_expires_at: 1,
    });
    let error = Bodies::default()
        .lifecycle_error(&frame(body))
        .expect("expired snapshot");
    assert!(error.contains("cap-old"));
}

#[test]
fn a_load_without_a_discovery_snapshot_refuses_before_the_adapter() {
    let body = encode_to_node(&ToNode::Load {
        plan: "{}".into(),
        artifact: "model.gguf".into(),
        ceiling: 1,
        capability_snapshot_id: String::new(),
        capability_expires_at: u64::MAX,
    });
    let error = Bodies::default()
        .lifecycle_error(&frame(body))
        .expect("missing snapshot id");
    assert!(
        error.contains("requires a capability snapshot id"),
        "{error}"
    );
}

#[test]
fn a_load_with_a_zero_expiry_snapshot_refuses_before_the_adapter() {
    let body = encode_to_node(&ToNode::Load {
        plan: "{}".into(),
        artifact: "model.gguf".into(),
        ceiling: 1,
        capability_snapshot_id: "cap-no-expiry".into(),
        capability_expires_at: 0,
    });
    let error = Bodies::default()
        .lifecycle_error(&frame(body))
        .expect("zero expiry");
    assert!(error.contains("has no expiry"), "{error}");
}

#[test]
fn inspect_model_snapshot_identity_and_expiry_are_preserved_in_load_work() {
    let inspected = Reply::Model {
        artifact: "model.gguf".into(),
        adapter: "llamacpp-staged".into(),
        profile: "profile".into(),
        capability_snapshot_id: "cap-inspected".into(),
        generated_at: 10,
        expires_at: u64::MAX,
    };
    let Reply::Model {
        artifact,
        capability_snapshot_id,
        expires_at,
        ..
    } = decode_reply(&encode_reply(&inspected)).expect("inspect model reply")
    else {
        panic!("model reply");
    };

    let body = encode_to_node(&ToNode::Load {
        plan: "opaque".into(),
        artifact,
        ceiling: 1,
        capability_snapshot_id,
        capability_expires_at: expires_at,
    });
    let Some(Work::Load(load)) = Bodies::default().lifecycle(&frame(body)) else {
        panic!("load work");
    };

    assert_eq!(load.capability_snapshot_id, "cap-inspected");
    assert_eq!(load.capability_expires_at, u64::MAX);
}

#[test]
fn a_live_discovery_snapshot_can_reach_the_adapter() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let body = encode_to_node(&ToNode::Load {
        plan: "{}".into(),
        artifact: "model.gguf".into(),
        ceiling: 1,
        capability_snapshot_id: "cap-live".into(),
        capability_expires_at: now + 60_000,
    });
    assert!(Bodies::default().lifecycle_error(&frame(body)).is_none());
}

#[test]
fn a_registered_discovery_snapshot_must_match_the_loaded_artifact() {
    let registry = CapabilityRegistry::default();
    let expires_at = u64::MAX;
    registry.insert(
        "cap-good".into(),
        Capability {
            artifact: "model.gguf".into(),
            adapter: "mock".into(),
            profile: "profile".into(),
            expires_at,
        },
    );
    let body = encode_to_node(&ToNode::Load {
        plan: "opaque".into(),
        artifact: "other.gguf".into(),
        ceiling: 1,
        capability_snapshot_id: "cap-good".into(),
        capability_expires_at: expires_at,
    });
    let error = Bodies::with_capabilities(registry)
        .lifecycle_error(&frame(body))
        .expect("artifact mismatch");
    assert!(error.contains("does not match"), "{error}");
}

#[test]
fn cache_work_carries_request_identity_and_chain_generation() {
    let body = encode_to_node(&ToNode::Persist {
        sequence: "sequence-7".into(),
    });
    let Some(Work::Cache(cache)) = Bodies::default().lifecycle(&frame(body)) else {
        panic!("cache work");
    };

    assert_eq!(cache.deployment, "deployment-a");
    assert_eq!(cache.generation, 1);
    assert_eq!(cache.operation_id, "request-7");
    assert_eq!(cache.sequence, "sequence-7");
    assert_eq!(cache.action, CacheAction::Persist);
}

#[test]
fn reconcile_work_carries_the_same_operation_identity() {
    let body = encode_to_node(&ToNode::Reconcile {
        sequence: "sequence-7".into(),
    });
    let Some(Work::Cache(cache)) = Bodies::default().lifecycle(&frame(body)) else {
        panic!("reconcile work");
    };

    assert_eq!(cache.operation_id, "request-7");
    assert_eq!(cache.sequence, "sequence-7");
    assert_eq!(cache.action, CacheAction::Reconcile);
}

#[test]
fn cache_failure_payload_preserves_the_barrier_identity_on_the_wire() {
    let body = encode_to_node(&ToNode::Persist {
        sequence: "sequence-7".into(),
    });
    let frame = frame(body);
    let encoded = <Bodies as p4_agent_core::node::payload::Payload>::cache_failure(
        &Bodies::default(),
        &frame,
        "disk refused",
    );
    assert_eq!(
        decode_reply(&encoded).unwrap(),
        Reply::CacheFailed {
            deployment: "deployment-a".into(),
            stage_id: "n0".into(),
            generation: 1,
            operation_id: "request-7".into(),
            sequence: "sequence-7".into(),
            detail: "disk refused".into(),
        }
    );
}
