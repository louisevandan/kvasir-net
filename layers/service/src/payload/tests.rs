use super::*;
use crate::message::wire::encode_to_node;
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
            deadline_unix_ms: 0,
            reply_to: None,
            chain: Some(chain),
        },
        body,
    }
}

#[test]
fn an_execute_body_becomes_a_sequence_named_by_its_route() {
    let body = encode_to_node(&ToNode::Execute {
        prompt: "안녕".into(),
        max_tokens: 64,
        options: r#"{"temperature":0.2}"#.into(),
    });
    let sequence = Bodies.sequence(&frame(body)).expect("executable");

    assert_eq!(sequence.sequence, "route-7");
    assert_eq!(sequence.prompt.as_deref(), Some("안녕"));
    assert_eq!(sequence.remaining, 64);
    assert_eq!(sequence.options, r#"{"temperature":0.2}"#);
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

    assert!(Bodies.sequence(&frame).is_none());
    let Some(Work::Load(load)) = Bodies.lifecycle(&frame) else {
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
    assert_eq!(Bodies.ceiling(&frame(body)), Some(24));
}

#[test]
fn an_execute_declares_no_ceiling() {
    let body = encode_to_node(&ToNode::Execute {
        prompt: "p".into(),
        max_tokens: 1,
        options: "{}".into(),
    });
    assert_eq!(Bodies.ceiling(&frame(body.clone())), None);
    assert!(Bodies.lifecycle(&frame(body)).is_none());
}

#[test]
fn an_unload_is_lifecycle() {
    let frame = frame(encode_to_node(&ToNode::Unload));
    assert!(matches!(Bodies.lifecycle(&frame), Some(Work::Unload(_))));
}

#[test]
fn an_unreadable_body_is_refused_rather_than_guessed_at() {
    // Malformed work reaching a backend is how a protocol fault turns into a
    // crash somewhere it cannot be traced.
    let frame = frame(vec![200, 1, 2, 3]);
    assert!(Bodies.sequence(&frame).is_none());
    assert!(Bodies.lifecycle(&frame).is_none());
    assert!(Bodies.ceiling(&frame).is_none());
}

#[test]
fn the_deployment_comes_from_the_chain_rather_than_the_body() {
    // So a body cannot disagree with the route it travelled.
    let frame = frame(encode_to_node(&ToNode::Unload));
    assert_eq!(Bodies.deployment(&frame).as_deref(), Some("deployment-a"));
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
    let error = Bodies
        .lifecycle_error(&frame(body))
        .expect("expired snapshot");
    assert!(error.contains("cap-old"));
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
    assert!(Bodies.lifecycle_error(&frame(body)).is_none());
}
