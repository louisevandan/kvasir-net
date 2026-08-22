use super::*;
use crate::message::wire::encode_to_node;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};

fn frame(body: Vec<u8>) -> Frame {
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
            chain: Some(
                Chain::new(vec![Link {
                    address: Address::tcp("127.0.0.1", 19001),
                    node: "n0".into(),
                    binding: "deployment-a".into(),
                    generation: 1,
                }])
                .expect("chain"),
            ),
        },
        body,
    }
}

#[test]
fn a_submission_stays_backend_neutral_until_the_deployment_client() {
    let options = r#"{"temperature":0.2,"stream":false}"#;
    let body = encode_to_node(&ToNode::Execute {
        prompt: "러스트를 설명하라".into(),
        max_tokens: 64,
        options: options.into(),
        session_epoch: 1,
    });
    let submit = Bodies::default()
        .submission(&frame(body))
        .expect("fresh submission");

    assert_eq!(
        submit.request,
        serde_json::json!({
            "prompt": "러스트를 설명하라",
            "max_tokens": 64,
            "options": options,
        })
    );
    assert!(submit.request.get("messages").is_none());
    assert!(submit.request.get("stream").is_none());
}
