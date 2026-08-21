use super::*;
use crate::node::payload::Payload;
use p4_protocol::{Address, Chain, Envelope, Link, QueueClass, Recipient};

fn carrier() -> Frame {
    let link = Link {
        address: Address::tcp("127.0.0.1", 52001),
        node: "tail".into(),
        binding: "tail-b".into(),
        generation: 1,
    };
    let chain = Chain::at(vec![link.clone()], 0).unwrap();
    Frame {
        envelope: Envelope {
            target: link.address,
            recipient: Recipient::node(link.node),
            lane: QueueClass::Decode,
            route: "route".into(),
            request_id: "request".into(),
            stream_id: "stream".into(),
            origin_agent: Some(Address::tcp("10.0.0.1", 19001)),
            return_channel: Some("return".into()),
            ingress_generation: 0,
            event_seq: 0,
            deadline_unix_ms: 0,
            reply_to: Some(Address::tcp("10.0.0.1", 19001)),
            chain: Some(chain),
        },
        body: b"work".to_vec(),
    }
}

struct TerminalPayload {
    emitted: u32,
    remaining: u32,
}

impl Payload for TerminalPayload {
    fn sequence(&self, _: &Frame) -> Option<p4_adapter::Sequence> {
        Some(p4_adapter::Sequence {
            sequence: "terminal".into(),
            prompt: None,
            state: None,
            remaining: self.remaining,
            options: String::new(),
        })
    }

    fn emitted(&self, _: &Frame) -> u32 {
        self.emitted
    }

    fn finished(&self, reason: &str, generated: u32) -> Vec<u8> {
        format!("{reason}:{generated}").into_bytes()
    }
}

fn length_terminal(sequence: &str, generated: u32) -> Outcome {
    Outcome {
        sequence: sequence.into(),
        forward: None,
        text: String::new(),
        stop: Some("length".into()),
        terminal_generated: Some(generated),
    }
}

fn done(outcome: &Outcome, emitted: u32, remaining: u32) -> Frame {
    let Next::Finish(done) = next(&carrier(), outcome, &TerminalPayload { emitted, remaining })
    else {
        panic!("terminal outcome must finish");
    };
    done
}

#[test]
fn native_length_terminal_uses_its_committed_count_when_text_tally_lags() {
    assert_eq!(
        done(&length_terminal("terminal", 200), 197, 200).body,
        b"length:200"
    );
}

#[test]
fn eos_keeps_the_visible_stream_tally_even_if_terminal_count_is_present() {
    let outcome = Outcome {
        sequence: "terminal".into(),
        forward: None,
        text: "끝".into(),
        stop: Some("eos".into()),
        terminal_generated: Some(200),
    };

    assert_eq!(done(&outcome, 176, 200).body, b"eos:177");
}

#[test]
fn malformed_terminal_count_cannot_exceed_the_request_bound() {
    assert_eq!(
        done(&length_terminal("terminal", 201), 199, 200).body,
        b"length:200"
    );
}

#[test]
fn terminal_counts_remain_isolated_per_sequence_in_one_parallel_window() {
    let first = done(&length_terminal("parallel-1", 200), 197, 200);
    let second = done(&length_terminal("parallel-2", 40), 38, 40);

    assert_eq!(first.body, b"length:200");
    assert_eq!(second.body, b"length:40");
}
