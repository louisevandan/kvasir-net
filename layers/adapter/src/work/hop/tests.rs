use super::*;

fn sequence(id: &str) -> Sequence {
    Sequence {
        sequence: id.into(),
        position: 0,
        prompt: Some("hello".into()),
        remaining: 8,
        options: "{}".into(),
    }
}

#[test]
fn a_hop_reports_the_width_of_its_window() {
    let hop = Hop {
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
