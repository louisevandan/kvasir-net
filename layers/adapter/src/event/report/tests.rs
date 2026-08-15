use super::*;

#[test]
fn a_stopped_sequence_says_so_rather_than_being_inferred_from_empty_text() {
    let running = Outcome {
        sequence: "a".into(),
        text: String::new(),
        position: 4,
        stop: None,
    };
    let stopped = Outcome {
        sequence: "b".into(),
        text: "done".into(),
        position: 9,
        stop: Some("stop".into()),
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
