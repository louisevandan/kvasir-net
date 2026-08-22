use super::*;
use crate::node::payload::Payload;
use p4_protocol::{Address, Link};

fn link(node: &str, port: u16) -> Link {
    Link {
        address: Address::tcp("127.0.0.1", port),
        node: node.into(),
        binding: format!("{node}-binding"),
        generation: 7,
    }
}

fn carrier(hops: u16, position: usize) -> Frame {
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
            lane: QueueClass::Decode,
            route: "route-9".into(),
            request_id: "request-9".into(),
            stream_id: "stream-9".into(),
            origin_agent: Some(Address::tcp("10.0.0.1", 19001)),
            return_channel: Some("channel-9".into()),
            ingress_generation: 0,
            event_seq: 3,
            deadline_unix_ms: 555,
            reply_to: Some(Address::tcp("10.0.0.1", 19001)),
            chain: Some(chain),
        },
        body: b"work".to_vec(),
    }
}

/// Marks exactly which sequence and close_id a close frame's body was built
/// for, so a test can tell the seam ran rather than defaulting.
struct Marking;

impl Payload for Marking {
    fn sequence(&self, _frame: &Frame) -> Option<p4_adapter::Sequence> {
        None
    }

    fn close(&self, sequence: &str, close_id: u64, _session_epoch: u64) -> Vec<u8> {
        format!("closed:{sequence}:{close_id}").into_bytes()
    }

    fn session_closed(&self, sequence: &str, close_id: u64) -> Vec<u8> {
        format!("ack:{sequence}:{close_id}").into_bytes()
    }

    fn supports_close(&self) -> bool {
        true
    }
}

/// The gate itself: a vocabulary that never opted in must never receive a
/// close frame, whatever `next` decided.
struct NotOptedIn;

impl Payload for NotOptedIn {
    fn sequence(&self, _frame: &Frame) -> Option<p4_adapter::Sequence> {
        None
    }
}

fn finish() -> Next {
    Next::Finish(Frame {
        envelope: Envelope {
            target: Address::tcp("10.0.0.1", 19001),
            recipient: Recipient::Agent,
            lane: QueueClass::Response,
            route: "route-9".into(),
            request_id: "request-9".into(),
            stream_id: "stream-9".into(),
            origin_agent: None,
            return_channel: None,
            ingress_generation: 0,
            event_seq: 4,
            deadline_unix_ms: 0,
            reply_to: None,
            chain: None,
        },
        body: b"done".to_vec(),
    })
}

/// A close_id source for tests: sequential, starting at 1, so assertions on
/// the returned ids stay readable.
fn ids() -> impl FnMut() -> u64 {
    let mut next = 0u64;
    move || {
        next += 1;
        next
    }
}

#[test]
fn a_finish_at_the_last_node_closes_every_earlier_link() {
    let carrier = carrier(3, 2);
    let closes = session_close_frames(&carrier, &finish(), "s1", &Marking, ids());

    assert_eq!(closes.len(), 2, "two earlier links, n0 and n1");
    let targets: Vec<_> = closes
        .iter()
        .map(|(_, frame)| frame.envelope.recipient.clone())
        .collect();
    assert_eq!(
        targets,
        vec![Recipient::node("n0"), Recipient::node("n1")],
        "closed in chain order, and never the last node telling itself"
    );
    for (close_id, frame) in &closes {
        assert_eq!(frame.body, format!("closed:s1:{close_id}").into_bytes());
        // The wire requires an inference envelope (one that names a chain)
        // to carry an origin agent and return channel, so the close inherits
        // the original request's -- but `reply_to` stays unset, and the
        // acknowledgement this provokes answers straight at this node
        // instead of at `origin_agent` (see `session_closed_frame`), so
        // nothing downstream of this actually uses either field to send
        // anything back.
        assert_eq!(
            frame.envelope.origin_agent, carrier.envelope.origin_agent,
            "carried only to satisfy the wire's inference-envelope rule"
        );
        assert_eq!(frame.envelope.reply_to, None, "never used to reply");
    }
    assert_ne!(
        closes[0].0, closes[1].0,
        "each earlier link gets its own close_id, not a shared one"
    );
}

#[test]
fn each_close_frame_is_positioned_at_its_own_link_not_the_senders() {
    let carrier = carrier(3, 2);
    let closes = session_close_frames(&carrier, &finish(), "s1", &Marking, ids());

    let (_, n0) = &closes[0];
    let chain = n0.envelope.chain.as_ref().expect("close carries a chain");
    assert_eq!(chain.current().node, "n0");
    assert_eq!(chain.current().binding, "n0-binding");
    assert_eq!(n0.envelope.target, Address::tcp("127.0.0.1", 52001));

    let (_, n1) = &closes[1];
    let chain = n1.envelope.chain.as_ref().expect("close carries a chain");
    assert_eq!(chain.current().node, "n1");
    assert_eq!(n1.envelope.target, Address::tcp("127.0.0.1", 52002));
}

#[test]
fn a_vocabulary_that_never_opted_in_receives_no_close_frame() {
    let carrier = carrier(3, 2);
    assert!(
        session_close_frames(&carrier, &finish(), "s1", &NotOptedIn, ids()).is_empty(),
        "a payload with no lifecycle arm for Close must never be sent one"
    );
}

#[test]
fn a_single_link_chain_has_nobody_else_to_close() {
    let carrier = carrier(1, 0);
    assert!(session_close_frames(&carrier, &finish(), "s1", &Marking, ids()).is_empty());
}

#[test]
fn unheard_also_closes_the_rest_of_the_chain() {
    let carrier = carrier(2, 1);
    let closes = session_close_frames(&carrier, &Next::Unheard, "s1", &Marking, ids());
    assert_eq!(
        closes.len(),
        1,
        "nobody listening is still a dead end for the chain behind it"
    );
}

#[test]
fn a_hop_still_in_progress_closes_nothing() {
    let carrier = carrier(3, 0);
    let onward = Next::Hop(carrier.clone());
    assert!(session_close_frames(&carrier, &onward, "s1", &Marking, ids()).is_empty());
}

#[test]
fn a_lap_still_in_progress_closes_nothing() {
    let carrier = carrier(2, 1);
    let lap = Next::Lap {
        token: carrier.clone(),
        lap: Box::new(carrier.clone()),
    };
    assert!(session_close_frames(&carrier, &lap, "s1", &Marking, ids()).is_empty());
    let muted = Next::LapWithoutToken {
        lap: carrier.clone(),
    };
    assert!(session_close_frames(&carrier, &muted, "s1", &Marking, ids()).is_empty());
}

#[test]
fn no_chain_at_all_closes_nothing() {
    let mut carrier = carrier(3, 2);
    carrier.envelope.chain = None;
    assert!(session_close_frames(&carrier, &finish(), "s1", &Marking, ids()).is_empty());
}

/// The receiving side of the round trip: a node that just processed a close
/// answers straight back at whoever's link sits last in the chain the close
/// itself carried -- which is the tail that sent it, whatever position this
/// node's own copy of that chain was repositioned to.
#[test]
fn the_acknowledgement_targets_the_chain_s_last_link_not_this_node() {
    let carrier = carrier(3, 2);
    let received = &session_close_frames(&carrier, &finish(), "s1", &Marking, ids())[0].1;

    let ack = session_closed_frame(received, "s1", 42, &Marking).expect("chain is present");
    assert_eq!(
        ack.envelope.recipient,
        Recipient::node("n2"),
        "n2 is the last link -- the tail that sent this close"
    );
    assert_eq!(ack.envelope.target, Address::tcp("127.0.0.1", 52003));
    assert_eq!(ack.body, b"ack:s1:42");
}

#[test]
fn the_acknowledgement_never_carries_the_origin_reply_path() {
    let carrier = carrier(2, 1);
    let received = &session_close_frames(&carrier, &finish(), "s1", &Marking, ids())[0].1;
    assert!(
        received.envelope.origin_agent.is_some(),
        "the close itself still carries it, only to satisfy the wire's rule"
    );

    let ack = session_closed_frame(received, "s1", 1, &Marking).expect("chain is present");
    assert_eq!(
        ack.envelope.origin_agent, None,
        "the ack answers a peer node, not OUTER, so it must not reuse origin_agent"
    );
    assert_eq!(ack.envelope.return_channel, None);
    assert_eq!(ack.envelope.reply_to, None);
    assert_eq!(ack.envelope.chain, None);
}

#[test]
fn no_chain_means_no_acknowledgement_either() {
    let mut carrier = carrier(2, 0);
    carrier.envelope.chain = None;
    assert!(session_closed_frame(&carrier, "s1", 1, &Marking).is_none());
}
