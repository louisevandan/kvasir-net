use super::*;
use crate::node::payload::Payload;
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
        (0..hops)
            .map(|i| link(&format!("n{i}"), 52001 + i))
            .collect(),
        position,
    )
    .unwrap();
    Frame {
        envelope: Envelope {
            target: chain.current().address.clone(),
            recipient: Recipient::node(chain.current().node.clone()),
            lane: QueueClass::Prefill,
            route: "route-1".into(),
            request_id: "request-1".into(),
            stream_id: "stream-1".into(),
            origin_agent: reply.then(|| Address::tcp("10.0.0.1", 19001)),
            return_channel: reply.then(|| "channel-1".into()),
            ingress_generation: 0,
            event_seq: 0,
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
        outbound_cut_set: None,
        text: text.into(),
        token: None,
        position: 1,
        stop: stop.map(Into::into),
    }
}

#[test]
fn a_middle_stage_forwards_its_outbound_cut_set() {
    let carrier = carrier(2, 0, true);
    let outcome = Outcome {
        sequence: "s1".into(),
        outbound_cut_set: Some(vec![9, 8, 7]),
        text: String::new(),
        token: None,
        position: 1,
        stop: None,
    };

    let Next::Hop(next) = next(&carrier, &outcome, &Plain) else {
        panic!("middle stage must forward the hop");
    };
    let (cut_set, original) = p4_adapter::decode_continuation(&next.body)
        .expect("staged continuation must preserve the opaque cut set");
    assert_eq!(cut_set, vec![9, 8, 7]);
    assert_eq!(original, b"work");
}

#[test]
fn a_repeated_stage_replaces_the_existing_cut_set_wrapper() {
    let first = carrier(3, 0, true);
    let Outcome {
        sequence,
        text,
        token,
        position,
        stop,
        ..
    } = Outcome {
        sequence: "s1".into(),
        outbound_cut_set: Some(vec![1, 2]),
        text: String::new(),
        token: None,
        position: 1,
        stop: None,
    };
    let first = match next(
        &first,
        &Outcome {
            sequence,
            outbound_cut_set: Some(vec![1, 2]),
            text,
            token,
            position,
            stop,
        },
        &Plain,
    ) {
        Next::Hop(frame) => frame,
        _ => panic!("first stage must hop"),
    };
    let second = match next(
        &first,
        &Outcome {
            sequence: "s1".into(),
            outbound_cut_set: Some(vec![3, 4]),
            text: String::new(),
            token: None,
            position: 1,
            stop: None,
        },
        &Plain,
    ) {
        Next::Hop(frame) => frame,
        _ => panic!("repeated stage must hop"),
    };
    let (cut_set, original) = p4_adapter::decode_continuation(&second.body).unwrap();
    assert_eq!(cut_set, vec![3, 4]);
    assert_eq!(original, b"work");
}

#[test]
fn a_decode_lap_drops_the_previous_tail_cut_set_before_stage_zero() {
    let first = carrier(1, 0, true);
    let first = match next(
        &first,
        &Outcome {
            sequence: "s1".into(),
            outbound_cut_set: Some(vec![1, 2]),
            text: "tok".into(),
            token: None,
            position: 1,
            stop: None,
        },
        &Plain,
    ) {
        Next::Lap { lap, .. } => lap,
        _ => panic!("the first terminal stage must start a lap"),
    };
    let second = match next(
        &first,
        &Outcome {
            sequence: "s1".into(),
            outbound_cut_set: Some(vec![3, 4]),
            text: "tok".into(),
            token: None,
            position: 2,
            stop: None,
        },
        &Plain,
    ) {
        Next::Lap { lap, .. } => lap,
        _ => panic!("the repeated terminal stage must start another lap"),
    };
    assert!(p4_adapter::decode_continuation(&second.body).is_none());
    assert_eq!(second.body, b"work");
}

#[test]
fn a_decode_lap_asks_the_payload_owner_for_continuation_body() {
    let carrier = carrier(1, 0, true);
    let Next::Lap { lap, .. } = next(&carrier, &outcome("tok", None), &Continuing) else {
        panic!("the terminal stage must start a decode lap");
    };
    assert_eq!(lap.body, b"continue-at-1");
}

#[test]
fn a_malformed_cut_set_is_failed_before_hop_or_lap_forwarding() {
    let mut carrier = carrier(2, 0, true);
    carrier.body = b"P4CUT01\0\0".to_vec();
    let Next::Finish(frame) = next(&carrier, &outcome("", None), &Plain) else {
        panic!("a malformed wrapper must not be forwarded");
    };
    assert_eq!(frame.envelope.lane, QueueClass::Response);
    assert_eq!(frame.body, b"malformed P4CUT01 continuation");
}

#[test]
fn a_middle_node_hands_the_work_on_with_its_body_untouched() {
    let Next::Hop(frame) = next(&carrier(3, 0, true), &outcome("", None), &Plain) else {
        panic!("a middle node hops");
    };
    assert_eq!(frame.envelope.recipient, Recipient::node("n1"));
    assert_eq!(frame.body, b"work");
}

#[test]
fn a_middle_node_hops_even_when_it_produced_no_text() {
    // Only the chain's end produces text. Reading emptiness as completion
    // would end every sequence at the first stage.
    let result = next(&carrier(3, 1, true), &outcome("", None), &Plain);
    assert!(matches!(result, Next::Hop(_)));
}

#[test]
fn the_last_node_finishing_replies_to_whoever_asked() {
    let Next::Finish(frame) = next(&carrier(3, 2, true), &outcome("done", Some("stop")), &Plain)
    else {
        panic!("a finished sequence at the end replies");
    };
    assert_eq!(frame.envelope.target, Address::tcp("10.0.0.1", 19001));
    assert_eq!(frame.envelope.lane, QueueClass::Response);
    assert_eq!(frame.body, b"stop");
}

#[test]
fn the_last_node_still_generating_reports_a_token_and_starts_a_lap() {
    // This is the ring: one lap of the chain produces one token.
    let Next::Lap { token, lap } = next(&carrier(3, 2, true), &outcome("tok", None), &Plain) else {
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
    let frames = next(&carrier(2, 1, true), &outcome("tok", None), &Plain).frames();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].body, b"tok");
    assert_eq!(frames[1].envelope.lane, QueueClass::Decode);
}

#[test]
fn streamed_events_advance_the_wire_sequence_across_laps() {
    let mut first = carrier(1, 0, true);
    first.envelope.event_seq = 4;
    let Next::Lap { token, lap } = next(&first, &outcome("tok", None), &Plain) else {
        panic!("an unfinished sequence at the end laps");
    };
    assert_eq!(token.envelope.event_seq, 5);
    assert_eq!(lap.envelope.event_seq, 5);

    let Next::Finish(done) = next(&lap, &outcome("", Some("stop")), &Plain) else {
        panic!("the next lap produces a terminal");
    };
    assert_eq!(done.envelope.event_seq, 6);
}

#[test]
fn a_single_node_chain_laps_against_itself() {
    // vLLM and SGLang run this way, and a lap of a one-link chain is a decode
    // step on the same node.
    let Next::Lap { lap, .. } = next(&carrier(1, 0, true), &outcome("tok", None), &Plain) else {
        panic!("a one-node chain still laps");
    };
    assert_eq!(lap.envelope.recipient, Recipient::node("n0"));
    assert_eq!(lap.envelope.lane, QueueClass::Decode);
}

#[test]
fn work_nobody_is_listening_for_is_reported_rather_than_dropped() {
    assert_eq!(
        next(&carrier(1, 0, false), &outcome("tok", None), &Plain),
        Next::Unheard
    );
    assert!(
        next(&carrier(1, 0, false), &outcome("", Some("stop")), &Plain)
            .frames()
            .is_empty()
    );
}

#[test]
fn a_finished_sequence_produces_exactly_one_terminal() {
    // Two terminals on one route is a defect this shape has to make
    // impossible.
    let frames = next(&carrier(2, 1, true), &outcome("", Some("stop")), &Plain).frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].envelope.lane, QueueClass::Response);
}

/// Plain-text reporting, which is what the trait's defaults do.
struct Plain;

impl Payload for Plain {
    fn sequence(&self, _: &Frame) -> Option<p4_adapter::Sequence> {
        None
    }
}

struct Continuing;

impl Payload for Continuing {
    fn sequence(&self, _: &Frame) -> Option<p4_adapter::Sequence> {
        None
    }

    fn continue_body(&self, _: &Frame, outcome: &Outcome) -> Vec<u8> {
        format!("continue-at-{}", outcome.position).into_bytes()
    }
}
