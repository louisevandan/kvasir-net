use super::*;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};

fn link(node: &str, port: u16) -> Link {
    Link {
        address: Address::tcp("127.0.0.1", port),
        node: node.into(),
        binding: format!("{node}-b"),
        generation: 1,
    }
}

fn carrier(hops: u16, position: usize, reply: bool) -> Frame {
    let chain = Chain::at(
        (0..hops).map(|i| link(&format!("n{i}"), 52001 + i)).collect(),
        position,
    )
    .unwrap();
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Prefill,
            route: "route-1".into(),
            deadline_unix_ms: 0,
            reply_to: reply.then(|| Address::tcp("10.0.0.1", 19001)),
            chain: Some(chain),
        },
        body: b"work".to_vec(),
    }
}

fn outcome(text: &str, stop: Option<&str>) -> Outcome {
    Outcome {
        sequence: "s1".into(),
        text: text.into(),
        position: 1,
        stop: stop.map(Into::into),
    }
}

#[test]
fn a_middle_node_hands_the_work_on_with_its_body_untouched() {
    let Next::Hop(frame) = next(&carrier(3, 0, true), &outcome("", None)) else {
        panic!("a middle node hops");
    };
    assert_eq!(frame.envelope.recipient, Recipient::node("n1"));
    assert_eq!(frame.body, b"work");
}

#[test]
fn a_middle_node_hops_even_when_it_produced_no_text() {
    // Only the chain's end produces text. Reading emptiness as completion
    // would end every sequence at the first stage.
    let result = next(&carrier(3, 1, true), &outcome("", None));
    assert!(matches!(result, Next::Hop(_)));
}

#[test]
fn the_last_node_finishing_replies_to_whoever_asked() {
    let Next::Finish(frame) = next(&carrier(3, 2, true), &outcome("done", Some("stop"))) else {
        panic!("a finished sequence at the end replies");
    };
    assert_eq!(frame.envelope.target, Address::tcp("10.0.0.1", 19001));
    assert_eq!(frame.envelope.lane, QueueClass::Response);
    assert_eq!(frame.body, b"stop");
}

#[test]
fn the_last_node_still_generating_reports_a_token_and_starts_a_lap() {
    // This is the ring: one lap of the chain produces one token.
    let Next::Lap { token, lap } = next(&carrier(3, 2, true), &outcome("tok", None)) else {
        panic!("an unfinished sequence at the end laps");
    };
    assert_eq!(token.envelope.target, Address::tcp("10.0.0.1", 19001));
    assert_eq!(token.body, b"tok");

    assert_eq!(lap.envelope.recipient, Recipient::node("n0"));
    assert_eq!(lap.envelope.lane, QueueClass::Decode);
    assert_eq!(lap.envelope.chain.as_ref().unwrap().position(), 0);
    assert_eq!(lap.body, b"work");
}

#[test]
fn a_token_is_enqueued_before_the_lap_it_precedes() {
    // Otherwise a reader could see a later position's token before an earlier
    // one, which looks like reordering in P4 rather than in the caller.
    let frames = next(&carrier(2, 1, true), &outcome("tok", None)).frames();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].body, b"tok");
    assert_eq!(frames[1].envelope.lane, QueueClass::Decode);
}

#[test]
fn a_single_node_chain_laps_against_itself() {
    // vLLM and SGLang run this way, and a lap of a one-link chain is a decode
    // step on the same node.
    let Next::Lap { lap, .. } = next(&carrier(1, 0, true), &outcome("tok", None)) else {
        panic!("a one-node chain still laps");
    };
    assert_eq!(lap.envelope.recipient, Recipient::node("n0"));
    assert_eq!(lap.envelope.lane, QueueClass::Decode);
}

#[test]
fn work_nobody_is_listening_for_is_reported_rather_than_dropped() {
    assert_eq!(next(&carrier(1, 0, false), &outcome("tok", None)), Next::Unheard);
    assert!(next(&carrier(1, 0, false), &outcome("", Some("stop")))
        .frames()
        .is_empty());
}

#[test]
fn a_finished_sequence_produces_exactly_one_terminal() {
    // Two terminals on one route is a defect this shape has to make
    // impossible.
    let frames = next(&carrier(2, 1, true), &outcome("", Some("stop"))).frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].envelope.lane, QueueClass::Response);
}
