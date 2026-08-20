use super::*;

#[test]
fn observations_preserve_phase_sequence_order_and_output_tail() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    let mut first = sequence("s0", 2);
    first.prompt = Some("hello".into());
    let mut second = sequence("s1", 1);
    second.prompt = None;
    mock.start(
        Work::Hop(Hop {
            id: 10,
            deployment: "d".into(),
            phase: Phase::Prefill,
            sequences: vec![first, second],
        }),
        &recorder,
    );

    let observation = &mock.hop_observations()[0];
    assert_eq!(observation.hop_id, 10);
    assert_eq!(observation.phase, Phase::Prefill);
    assert_eq!(
        observation
            .sequences
            .iter()
            .map(|sequence| sequence.sequence.as_str())
            .collect::<Vec<_>>(),
        vec!["s0", "s1"]
    );
    assert_eq!(observation.sequences[0].prompt.as_deref(), Some("hello"));
    assert_eq!(observation.sequences[1].prompt, None);
    assert_eq!(
        observation
            .sequences
            .iter()
            .map(|sequence| sequence.position)
            .collect::<Vec<_>>(),
        vec![0, 0]
    );
    assert_eq!(observation.sequences[0].options, "{}");
    assert_eq!(
        observation
            .outcomes
            .iter()
            .map(|outcome| (
                outcome.sequence.as_str(),
                crate::decode_state(outcome.forward.as_ref()).0,
                outcome.is_finished()
            ))
            .collect::<Vec<_>>(),
        vec![("s0", 1, false), ("s1", 1, false)]
    );

    mock.start(
        Work::Hop(Hop {
            id: 11,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![
                Sequence {
                    state: Some(crate::encode_state(1, None)),
                    ..sequence("s0", 2)
                },
                Sequence {
                    state: Some(crate::encode_state(1, None)),
                    ..sequence("s1", 1)
                },
            ],
        }),
        &recorder,
    );
    let tail = &mock.hop_observations()[1];
    assert_eq!(tail.phase, Phase::Decode);
    assert_eq!(tail.outcomes[0].text, "s0#2 ");
    assert_eq!(tail.outcomes[1].text, "");
    assert_eq!(tail.outcomes[1].stop.as_deref(), Some("stop"));
}

#[test]
fn staged_observations_carry_opaque_cut_sets_in_and_out() {
    let mock = Mock::staged(1, Profile::default());
    let recorder = Recorder::default();
    let inbound = b"real-stage-cut-set".to_vec();
    let mut request = sequence("s0", 2);
    request.prompt = None;
    request.state = Some(crate::encode_state(0, Some(inbound.clone())));
    mock.start(
        Work::Hop(Hop {
            id: 12,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![request],
        }),
        &recorder,
    );

    let observation = &mock.hop_observations()[0];
    assert_eq!(observation.sequences[0].inbound_cut_set, Some(inbound));
    // The mock reads its own state and finds its own cut-set in it.
    assert_eq!(observation.outcomes.len(), 1);
    assert!(
        crate::decode_state(observation.outcomes[0].forward.as_ref())
            .1
            .is_some()
    );
    assert!(observation.outcomes[0].text.is_empty());
    assert!(!observation.outcomes[0].is_finished());
}

#[test]
fn discovery_profile_has_llama_shape_and_stable_fingerprint() {
    let mock = Mock::staged(
        0,
        Profile {
            stages: 2,
            reserved_per_stage: 4096,
            ..Profile::default()
        },
    );
    let first = mock.inspect_model("model.gguf").unwrap();
    let second = mock.inspect_model("model.gguf").unwrap();
    assert_eq!(first, second);
    assert!(first.contains(r#""architecture":"llama""#), "{first}");
    assert!(first.contains(r#""fingerprint":"mock-"#), "{first}");
    assert!(first.contains(r#""kv_heads":8"#), "{first}");
}

#[test]
fn opaque_plan_and_generation_options_reach_the_mock_boundary() {
    let mock = Mock::terminal(0, Profile::default());
    let recorder = Recorder::default();
    mock.start(
        Work::Load(Load {
            deployment: "d".into(),
            plan: r#"{"split":"layer-0-31","n_gpu_layers":32}"#.into(),
            artifact: "model.gguf".into(),
            capability_snapshot_id: "cap-model-1".into(),
            capability_expires_at: 123_456,
        }),
        &recorder,
    );
    let mut request = sequence("s0", 1);
    request.options = r#"{"temperature":0.2,"top_p":0.9,"mtp":true}"#.into();
    mock.start(
        Work::Hop(Hop {
            id: 1,
            deployment: "d".into(),
            phase: Phase::Prefill,
            sequences: vec![request],
        }),
        &recorder,
    );
    assert_eq!(mock.plans().len(), 1);
    assert_eq!(
        mock.load_observations(),
        vec![LoadObservation {
            artifact: "model.gguf".into(),
            plan: r#"{"split":"layer-0-31","n_gpu_layers":32}"#.into(),
            capability_snapshot_id: "cap-model-1".into(),
            capability_expires_at: 123_456,
        }]
    );
    assert_eq!(
        mock.options_seen().last().unwrap(),
        r#"{"temperature":0.2,"top_p":0.9,"mtp":true}"#
    );
    assert_eq!(
        mock.hop_observations()[0].sequences[0].options,
        r#"{"temperature":0.2,"top_p":0.9,"mtp":true}"#
    );
    let report = mock.report();
    assert!(report.contains("llama-compatible-mock"), "{report}");
    assert!(report.contains(r#""options_seen":1"#), "{report}");
}

#[test]
fn malformed_generation_options_are_refused_per_sequence() {
    let mock = Mock::terminal(0, Profile::default());
    let recorder = Recorder::default();
    let mut request = sequence("bad", 1);
    request.options = "--temperature 0.2".into();
    mock.start(
        Work::Hop(Hop {
            id: 1,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![request],
        }),
        &recorder,
    );
    assert!(matches!(
        recorder.events().first(),
        Some(Event::Failed { sequence: Some(id), .. }) if id == "bad"
    ));
}

#[test]
fn a_decode_lap_uses_the_carried_position_and_does_not_need_the_tail_cut_set() {
    let mock = Mock::terminal(0, Profile::default());
    let recorder = Recorder::default();
    mock.start(
        Work::Hop(Hop {
            id: 20,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![Sequence {
                sequence: "conversation".into(),
                state: Some(crate::encode_state(6, None)),
                prompt: None,
                remaining: 8,
                options: r#"{"temperature":0.1}"#.into(),
            }],
        }),
        &recorder,
    );
    let Some(Event::HopComplete { outcomes, .. }) = recorder.events().pop() else {
        panic!("decode lap completed");
    };
    assert_eq!(crate::decode_state(outcomes[0].forward.as_ref()).0, 7);
    assert_eq!(outcomes[0].text, "conversation#7 ");
    assert!(!outcomes[0].is_finished());
    assert_eq!(mock.hop_observations()[0].sequences[0].position, 6);
}
