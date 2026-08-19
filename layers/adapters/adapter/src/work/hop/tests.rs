use super::*;

fn sequence(id: &str) -> Sequence {
    Sequence {
        sequence: id.into(),
        inbound_cut_set: None,
        position: 0,
        prompt: Some("hello".into()),
        initial_tokens: None,
        remaining: 8,
        options: "{}".into(),
    }
}

#[test]
fn a_hop_reports_the_width_of_its_window() {
    let hop = Hop {
        id: 1,
        deployment: "d".into(),
        phase: Phase::Prefill,
        sequences: vec![sequence("a"), sequence("b"), sequence("c")],
    };
    assert_eq!(hop.width(), 3);
    assert!(!hop.is_empty());
}

#[test]
fn an_empty_window_is_recognised_before_a_backend_sees_it() {
    let hop = Hop {
        id: 2,
        deployment: "d".into(),
        phase: Phase::Decode,
        sequences: Vec::new(),
    };
    assert!(hop.is_empty());
    assert_eq!(hop.width(), 0);
}

#[test]
fn a_later_stage_continues_from_state_rather_than_from_text() {
    let mut continuing = sequence("a");
    continuing.prompt = None;
    continuing.position = 512;
    assert!(continuing.prompt.is_none());
    assert_eq!(continuing.position, 512);
}

#[test]
fn a_sequence_can_carry_an_opaque_inbound_cut_set() {
    let payload = vec![0, 7, 9, 255];
    let sequence = Sequence {
        sequence: "stage-1".into(),
        inbound_cut_set: Some(payload.clone()),
        position: 12,
        prompt: None,
        initial_tokens: None,
        remaining: 1,
        options: "{}".into(),
    };

    assert_eq!(sequence.inbound_cut_set, Some(payload));
}

#[test]
fn a_cut_set_continuation_keeps_the_original_sequence_context() {
    let cut_set = vec![0, 7, 9, 255];
    let original = b"execute-body";
    let encoded = encode_continuation(&cut_set, original);
    assert_eq!(
        decode_continuation(&encoded),
        Some((cut_set, original.to_vec()))
    );
    assert!(decode_continuation(b"P4CUT01\0\0").is_none());
}
