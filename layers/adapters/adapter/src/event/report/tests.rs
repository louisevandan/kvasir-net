use super::*;

#[test]
fn a_stopped_sequence_says_so_rather_than_being_inferred_from_empty_text() {
    let running = Outcome {
        sequence: "a".into(),
        forward: None,
        text: String::new(),
        stop: None,
        terminal_generated: None,
    };
    let stopped = Outcome {
        sequence: "b".into(),
        forward: None,
        text: "done".into(),
        stop: Some("stop".into()),
        terminal_generated: None,
    };
    // A middle stage produces no text and is not finished; reading emptiness
    // as completion would end every sequence at the first stage.
    assert!(!running.is_finished());
    assert!(stopped.is_finished());
}

#[test]
fn load_progress_names_the_stage_it_belongs_to() {
    let event = Event::LoadProgress {
        deployment: "d".into(),
        stage: 2,
        percent: 40,
        detail: "tensors".into(),
    };
    let Event::LoadProgress { stage, percent, .. } = event else {
        panic!("expected load progress");
    };
    assert_eq!((stage, percent), (2, 40));
}

#[test]
fn an_outcome_can_carry_an_opaque_outbound_cut_set() {
    let payload = vec![1, 2, 3, 254];
    let outcome = Outcome {
        sequence: "stage-1".into(),
        forward: Some(payload.clone()),
        text: String::new(),
        stop: None,
        terminal_generated: None,
    };

    assert_eq!(outcome.forward, Some(payload));
    assert!(!outcome.is_finished());
}
