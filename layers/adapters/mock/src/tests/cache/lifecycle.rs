use super::*;

#[test]
fn restore_keeps_the_durable_kv_copy_and_unload_only_frees_resident_state() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    mock.start(hop(1, Phase::Prefill), &recorder);
    let cache = |action| {
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: format!("op-{action:?}"),
            sequence: "s0".into(),
            action,
        })
    };

    mock.start(cache(p4_adapter::CacheAction::Persist), &recorder);
    let persisted = mock.cache_state("s0");
    assert!(!persisted.resident);
    assert!(persisted.persisted);
    assert!(persisted.bytes.is_some());

    mock.start(cache(p4_adapter::CacheAction::Restore), &recorder);
    assert_eq!(
        mock.cache_state("s0"),
        CacheState {
            resident: true,
            persisted: true,
            bytes: persisted.bytes,
        }
    );
    mock.start(
        Work::Unload(p4_adapter::Unload {
            deployment: "d".into(),
        }),
        &recorder,
    );
    assert_eq!(
        mock.cache_state("s0"),
        CacheState {
            resident: false,
            persisted: true,
            bytes: persisted.bytes,
        }
    );
}

#[test]
fn missing_cache_actions_are_refused_without_creating_resident_state() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    let cache = |action| {
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: format!("missing-{action:?}"),
            sequence: "missing".into(),
            action,
        })
    };

    mock.start(cache(p4_adapter::CacheAction::Persist), &recorder);
    assert!(matches!(
        recorder.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("nothing resident")
    ));
    mock.start(cache(p4_adapter::CacheAction::Restore), &recorder);
    assert!(matches!(
        recorder.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("nothing persisted")
    ));
    assert!(mock.resident().is_empty());
    assert!(mock.persisted().is_empty());
}

#[test]
fn a_persisted_sequence_must_restore_before_the_next_hop() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    mock.start(hop(1, Phase::Prefill), &recorder);
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "ordered-persist".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Persist,
        }),
        &recorder,
    );

    mock.start(hop(1, Phase::Decode), &recorder);
    assert!(matches!(
        recorder.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("requires Restore before Hop")
    ));
    assert!(mock.resident().is_empty());

    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "ordered-restore".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Restore,
        }),
        &recorder,
    );
    mock.start(hop(1, Phase::Decode), &recorder);
    assert!(matches!(
        recorder.events().last(),
        Some(Event::HopComplete { .. })
    ));
    assert!(mock.resident().iter().any(|id| id == "s0"));
}

#[test]
fn an_unload_is_answered() {
    let mock = Mock::terminal(0, Profile::default());
    let recorder = Recorder::default();
    mock.start(
        Work::Unload(Unload {
            deployment: "d".into(),
        }),
        &recorder,
    );
    assert!(matches!(
        recorder.events().first(),
        Some(Event::Unloaded { .. })
    ));
}

#[test]
fn four_stage_commit_failure_has_no_visible_cache_and_compensates_earlier_stages() {
    let root = std::env::temp_dir().join(format!(
        "p4-mock-four-stage-abort-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let stages = [
        Mock::internal_with_cache_dir(Profile::default(), root.join("s0")),
        Mock::internal_with_cache_dir(Profile::default(), root.join("s1")),
        Mock::internal_with_cache_dir(
            Profile {
                fault: Fault::CacheCommit,
                ..Profile::default()
            },
            root.join("s2"),
        ),
        Mock::internal_with_cache_dir(Profile::default(), root.join("s3")),
    ];
    let recorders = (0..4).map(|_| Recorder::default()).collect::<Vec<_>>();
    for (stage, recorder) in stages.iter().zip(&recorders) {
        stage.start(named_hop("conversation", Phase::Prefill), recorder);
    }
    let cache = |operation_id: &str, action: p4_adapter::CacheAction, stage: usize| {
        stages[stage].start(
            Work::Cache(p4_adapter::Cache {
                deployment: "d".into(),
                stage_id: format!("stage-{stage}"),
                generation: 1,
                operation_id: operation_id.into(),
                sequence: "conversation".into(),
                action,
            }),
            &recorders[stage],
        );
    };
    for stage in 0..4 {
        cache("save-fail", p4_adapter::CacheAction::PreparePersist, stage);
    }
    for stage in 0..3 {
        cache("save-fail", p4_adapter::CacheAction::Commit, stage);
    }
    assert!(matches!(
        recorders[2].events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("commit")
    ));
    for stage in 0..4 {
        cache("save-fail", p4_adapter::CacheAction::Abort, stage);
    }
    for stage in &stages {
        let state = stage.cache_state("conversation");
        assert!(!state.persisted, "an aborted stage published durable KV");
        assert!(state.resident, "rollback must retain resident KV");
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn four_stage_save_unload_reload_restore_preserves_each_shard() {
    let root = std::env::temp_dir().join(format!(
        "p4-mock-four-stage-restore-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let make = || {
        [0, 1, 2, 3].map(|stage| {
            Mock::internal_with_cache_dir(Profile::default(), root.join(format!("s{stage}")))
        })
    };
    let stages = make();
    let recorders = (0..4).map(|_| Recorder::default()).collect::<Vec<_>>();
    for (stage, recorder) in stages.iter().zip(&recorders) {
        stage.start(named_hop("conversation", Phase::Prefill), recorder);
    }
    let run_cache =
        |stages: &[Mock], recorders: &[Recorder], op: &str, action: p4_adapter::CacheAction| {
            for (index, stage) in stages.iter().enumerate() {
                stage.start(
                    Work::Cache(p4_adapter::Cache {
                        deployment: "d".into(),
                        stage_id: format!("stage-{index}"),
                        generation: 1,
                        operation_id: op.into(),
                        sequence: "conversation".into(),
                        action: action.clone(),
                    }),
                    &recorders[index],
                );
            }
        };
    run_cache(
        &stages,
        &recorders,
        "save",
        p4_adapter::CacheAction::PreparePersist,
    );
    run_cache(&stages, &recorders, "save", p4_adapter::CacheAction::Commit);
    let saved = stages
        .iter()
        .zip(&recorders)
        .map(|(stage, recorder)| {
            assert!(
                matches!(recorder.events().last(), Some(Event::Cached { .. })),
                "save commit did not complete: {:?}",
                recorder.events()
            );
            stage.cache_state("conversation").bytes.unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        stages
            .iter()
            .all(|stage| !stage.cache_state("conversation").resident)
    );
    stages.iter().zip(&recorders).for_each(|(stage, recorder)| {
        stage.start(
            Work::Unload(p4_adapter::Unload {
                deployment: "d".into(),
            }),
            recorder,
        );
    });
    drop(stages);

    let reloaded = make();
    let reload_recorders = (0..4).map(|_| Recorder::default()).collect::<Vec<_>>();
    for (stage, recorder) in reloaded.iter().zip(&reload_recorders) {
        stage.start(
            Work::Load(p4_adapter::Load {
                deployment: "d".into(),
                plan: "{}".into(),
                artifact: "model".into(),
                capability_snapshot_id: String::new(),
                capability_expires_at: 0,
            }),
            recorder,
        );
    }
    run_cache(
        &reloaded,
        &reload_recorders,
        "restore",
        p4_adapter::CacheAction::PrepareRestore,
    );
    run_cache(
        &reloaded,
        &reload_recorders,
        "restore",
        p4_adapter::CacheAction::Commit,
    );
    let restored = reloaded
        .iter()
        .map(|stage| stage.cache_state("conversation").bytes.unwrap())
        .collect::<Vec<_>>();
    assert_eq!(restored, saved, "reload restored every stage's exact shard");
    assert!(
        reloaded
            .iter()
            .all(|stage| stage.cache_state("conversation").resident)
    );
    let _ = std::fs::remove_dir_all(root);
}
