use super::*;

fn sequence(id: &str) -> Sequence {
    Sequence {
        sequence: id.into(),
        session_epoch: 0,
        state: None,
        prompt: Some("hello".into()),
        remaining: 8,
        options: "{}".into(),
    }
}

#[test]
fn a_hop_reports_the_width_of_its_window() {
    let hop = Hop {
        id: 1,
        deployment: "d".into(),
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
        sequences: Vec::new(),
    };
    assert!(hop.is_empty());
    assert_eq!(hop.width(), 0);
}

#[test]
fn a_later_stage_continues_from_state_rather_than_from_text() {
    let mut continuing = sequence("a");
    continuing.prompt = None;
    continuing.state = Some(vec![0, 2, 0, 0]);
    assert!(continuing.prompt.is_none());
    assert_eq!(continuing.state, Some(vec![0, 2, 0, 0]));
}

#[test]
fn a_sequence_can_carry_an_opaque_inbound_cut_set() {
    let payload = vec![0, 7, 9, 255];
    let sequence = Sequence {
        sequence: "stage-1".into(),
        session_epoch: 0,
        state: Some(payload.clone()),
        prompt: None,
        remaining: 1,
        options: "{}".into(),
    };

    assert_eq!(sequence.state, Some(payload));
}
