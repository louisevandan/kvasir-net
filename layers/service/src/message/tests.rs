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
        },
        ToNode::Unload,
        ToNode::Execute {
            prompt: "안녕".into(),
            max_tokens: 500,
            options: r#"{"temperature":0.7}"#.into(),
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
        },
        Reply::Failed {
            detail: "backend refused".into(),
        },
        Reply::Machine {
            snapshot: "{}".into(),
        },
    ] {
        assert_eq!(decode_reply(&encode_reply(&reply)).unwrap(), reply);
    }
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
    });
    for cut in 1..bytes.len() {
        assert!(
            decode_to_node(&bytes[..cut]).is_err(),
            "prefix {cut} decoded"
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
