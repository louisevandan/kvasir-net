use super::*;

#[test]
fn a_load_reports_one_stage_at_a_time_then_binds() {
    // A distributed load finishes when its slowest piece does, so progress has
    // to be visible per piece rather than as one figure.
    let mock = Mock::staged(
        0,
        Profile {
            stages: 3,
            ..Profile::default()
        },
    );
    let recorder = Recorder::default();
    mock.start(
        Work::Load(Load {
            deployment: "d".into(),
            plan: "{}".into(),
            artifact: "model".into(),
            capability_snapshot_id: String::new(),
            capability_expires_at: 0,
        }),
        &recorder,
    );

    let events = recorder.events();
    let stages: Vec<u32> = events
        .iter()
        .filter_map(|event| match event {
            Event::LoadProgress { stage, .. } => Some(*stage),
            _ => None,
        })
        .collect();
    assert_eq!(stages, vec![0, 1, 2]);
    assert!(matches!(events.last(), Some(Event::Loaded { .. })));
}

#[test]
fn a_load_reports_a_reservation_per_stage() {
    let mock = Mock::staged(
        0,
        Profile {
            stages: 2,
            reserved_per_stage: 1024,
            ..Profile::default()
        },
    );
    let recorder = Recorder::default();
    mock.start(
        Work::Load(Load {
            deployment: "d".into(),
            plan: "{}".into(),
            artifact: "m".into(),
            capability_snapshot_id: String::new(),
            capability_expires_at: 0,
        }),
        &recorder,
    );

    let Some(Event::Loaded { allocations, .. }) = recorder.events().pop() else {
        panic!("the load bound");
    };
    assert_eq!(allocations.len(), 2);
    assert!(allocations.iter().all(|entry| entry.bytes == 1024));
}

#[test]
fn a_generation_is_issued_by_the_adapter_and_advances() {
    // The one identifier an adapter issues rather than receives, because it
    // names a materialisation only the adapter witnessed.
    let mock = Mock::terminal(0, Profile::default());
    let mut generations = Vec::new();
    for _ in 0..2 {
        let recorder = Recorder::default();
        mock.start(
            Work::Load(Load {
                deployment: "d".into(),
                plan: "{}".into(),
                artifact: "m".into(),
                capability_snapshot_id: String::new(),
                capability_expires_at: 0,
            }),
            &recorder,
        );
        if let Some(Event::Loaded { generation, .. }) = recorder.events().pop() {
            generations.push(generation);
        }
    }
    assert_eq!(generations, vec![1, 2]);
}

#[test]
fn a_hop_answers_every_sequence_it_was_given() {
    let mock = Mock::terminal(0, Profile::default());
    let recorder = Recorder::default();
    mock.start(hop(5, Phase::Prefill), &recorder);

    let Some(Event::HopComplete { outcomes, .. }) = recorder.events().pop() else {
        panic!("the hop completed");
    };
    assert_eq!(outcomes.len(), 5);
}

#[test]
fn a_sequence_with_one_token_left_finishes_on_the_following_terminal_lap() {
    let mock = Mock::terminal(0, Profile::default());
    let recorder = Recorder::default();
    mock.start(
        Work::Hop(Hop {
            id: 1,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![sequence("s0", 1), sequence("s1", 9)],
        }),
        &recorder,
    );

    let Some(Event::HopComplete { outcomes, .. }) = recorder.events().pop() else {
        panic!("the hop completed");
    };
    assert!(
        !outcomes[0].is_finished(),
        "the final token is emitted first"
    );
    assert!(!outcomes[1].is_finished());

    let recorder = Recorder::default();
    mock.start(
        Work::Hop(Hop {
            id: 2,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![sequence("s0", 1), sequence("s1", 9)],
        }),
        &recorder,
    );
    let Some(Event::HopComplete { outcomes, .. }) = recorder.events().pop() else {
        panic!("the terminal lap completed");
    };
    assert!(outcomes[0].is_finished());
    assert!(!outcomes[1].is_finished());
}

#[test]
fn the_widths_it_was_given_are_recorded_rather_than_chosen() {
    // The window is the node's decision. The adapter reports what it received,
    // which is how a test proves batching happened above it.
    let mock = Mock::terminal(0, Profile::default());
    let recorder = Recorder::default();
    mock.start(hop(3, Phase::Prefill), &recorder);
    mock.start(hop(7, Phase::Decode), &recorder);
    assert_eq!(mock.widths(), vec![3, 7]);
}

#[test]
fn zero_remaining_keeps_llama_first_token_then_reports_the_terminal_tail() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    let mut request = sequence("s0", 0);
    request.prompt = Some("hello".into());
    mock.start(
        Work::Hop(Hop {
            id: 13,
            deployment: "d".into(),
            phase: Phase::Prefill,
            sequences: vec![request.clone()],
        }),
        &recorder,
    );
    mock.start(
        Work::Hop(Hop {
            id: 14,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![request],
        }),
        &recorder,
    );
    let observations = mock.hop_observations();
    assert_eq!(observations[0].outcomes[0].text, "s0#1 ");
    assert!(!observations[0].outcomes[0].is_finished());
    assert_eq!(observations[1].outcomes[0].text, "");
    assert_eq!(observations[1].outcomes[0].stop.as_deref(), Some("stop"));
}

#[test]
fn a_load_fault_fails_instead_of_binding() {
    let mock = Mock::staged(
        0,
        Profile {
            fault: Fault::Load,
            ..Profile::default()
        },
    );
    let recorder = Recorder::default();
    mock.start(
        Work::Load(Load {
            deployment: "d".into(),
            plan: "{}".into(),
            artifact: "m".into(),
            capability_snapshot_id: String::new(),
            capability_expires_at: 0,
        }),
        &recorder,
    );
    assert!(matches!(
        recorder.events().pop(),
        Some(Event::Failed { .. })
    ));
    assert!(mock.report().contains(r#""loaded":false"#));
}

#[test]
fn a_hop_fault_fails_the_hop() {
    let mock = Mock::staged(
        0,
        Profile {
            fault: Fault::Hop,
            ..Profile::default()
        },
    );
    let recorder = Recorder::default();
    mock.start(hop(2, Phase::Prefill), &recorder);
    assert!(matches!(
        recorder.events().pop(),
        Some(Event::Failed { .. })
    ));
}

#[test]
fn silence_answers_nothing_and_leaves_the_deadline_to_do_it() {
    let mock = Mock::staged(
        0,
        Profile {
            fault: Fault::Silence,
            ..Profile::default()
        },
    );
    let recorder = Recorder::default();
    mock.start(hop(2, Phase::Prefill), &recorder);
    assert!(recorder.events().is_empty());
}

#[test]
fn an_internal_backend_declares_it_cannot_be_a_stage() {
    // vLLM and SGLang coordinate their own parallelism, so a chain over one is
    // a single node.
    assert_eq!(
        Mock::internal(Profile::default()).distribution(),
        Distribution::Internal
    );
    assert_eq!(
        Mock::staged(1, Profile::default()).distribution(),
        Distribution::Staged
    );
}

#[test]
fn only_the_last_stage_produces_a_token() {
    // Logits exist at the end of the model. If a middle stage counted, an
    // n-stage chain would emit n tokens for every lap of the ring.
    let middle = Mock::staged(1, Profile::default());
    let recorder = Recorder::default();
    middle.start(
        Work::Hop(Hop {
            id: 1,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![sequence("s0", 1)],
        }),
        &recorder,
    );

    let Some(Event::HopComplete { outcomes, .. }) = recorder.events().pop() else {
        panic!("the hop completed");
    };
    assert!(outcomes[0].text.is_empty(), "a middle stage says nothing");
    assert!(!outcomes[0].is_finished(), "and never ends a sequence");
}

#[test]
fn a_middle_stage_preserves_the_global_decode_position() {
    let middle = Mock::staged(1, Profile::default());
    let recorder = Recorder::default();
    middle.start(
        Work::Hop(Hop {
            id: 2,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![Sequence {
                sequence: "s0".into(),
                inbound_cut_set: None,
                position: 6,
                prompt: None,
                initial_tokens: None,
                remaining: 3,
                options: "{}".into(),
            }],
        }),
        &recorder,
    );
    let Some(Event::HopComplete { outcomes, .. }) = recorder.events().pop() else {
        panic!("the middle hop completed");
    };
    assert_eq!(outcomes[0].position, 6);
    assert!(outcomes[0].text.is_empty());
}

#[test]
fn a_terminal_stage_counts_across_laps_rather_than_rereading_the_request() {
    // The request does not carry its progress back down, so the backend
    // holding the sequence open is what knows how far it has got.
    let last = Mock::terminal(1, Profile::default());
    let mut finished = Vec::new();
    for _ in 0..4 {
        let recorder = Recorder::default();
        last.start(
            Work::Hop(Hop {
                id: 1,
                deployment: "d".into(),
                phase: Phase::Decode,
                sequences: vec![sequence("s0", 3)],
            }),
            &recorder,
        );
        if let Some(Event::HopComplete { outcomes, .. }) = recorder.events().pop() {
            finished.push(outcomes[0].is_finished());
        }
    }
    assert_eq!(finished, vec![false, false, false, true]);
}

#[test]
fn a_commit_cannot_rebind_a_prepared_operation_to_another_sequence() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    mock.start(hop(1, Phase::Prefill), &recorder);
    for (sequence, action) in [
        ("s0", p4_adapter::CacheAction::PreparePersist),
        ("s1", p4_adapter::CacheAction::Commit),
    ] {
        mock.start(
            Work::Cache(p4_adapter::Cache {
                deployment: "d".into(),
                stage_id: "stage-1".into(),
                generation: 1,
                operation_id: "tx-identity".into(),
                sequence: sequence.into(),
                action,
            }),
            &recorder,
        );
    }
    assert!(matches!(
        recorder.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("identity")
    ));
}
