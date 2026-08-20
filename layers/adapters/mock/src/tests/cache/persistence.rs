use super::*;

#[test]
fn a_file_backed_restore_preserves_the_next_decode_position_after_reload() {
    let root = std::env::temp_dir().join(format!(
        "p4-mock-next-position-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let first = Mock::internal_with_cache_dir(
        Profile {
            reserved_per_stage: 1024,
            ..Profile::default()
        },
        &root,
    );
    let events = Recorder::default();
    first.start(
        Work::Hop(Hop {
            id: 21,
            deployment: "d".into(),
            phase: Phase::Prefill,
            sequences: vec![sequence("conversation", 8)],
        }),
        &events,
    );
    first.start(
        Work::Hop(Hop {
            id: 22,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![Sequence {
                state: Some(crate::encode_state(1, None)),
                ..sequence("conversation", 8)
            }],
        }),
        &events,
    );
    first.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-0".into(),
            generation: 1,
            operation_id: "save-next-position".into(),
            sequence: "conversation".into(),
            action: p4_adapter::CacheAction::Persist,
        }),
        &events,
    );
    let saved = first.cache_state("conversation");
    assert!(!saved.resident);
    drop(first);

    let restored = Mock::internal_with_cache_dir(
        Profile {
            reserved_per_stage: 1024,
            ..Profile::default()
        },
        &root,
    );
    restored.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-0".into(),
            generation: 1,
            operation_id: "restore-next-position".into(),
            sequence: "conversation".into(),
            action: p4_adapter::CacheAction::Restore,
        }),
        &events,
    );
    restored.start(
        Work::Hop(Hop {
            id: 23,
            deployment: "d".into(),
            phase: Phase::Decode,
            sequences: vec![Sequence {
                state: Some(crate::encode_state(2, None)),
                ..sequence("conversation", 8)
            }],
        }),
        &events,
    );
    let observations = restored.hop_observations();
    let observation = observations.last().unwrap();
    assert_eq!(
        crate::decode_state(observation.outcomes[0].forward.as_ref()).0,
        3
    );
    assert_eq!(observation.outcomes[0].text, "conversation#3 ");
    assert_eq!(restored.cache_state("conversation").bytes, saved.bytes);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_file_backed_cache_survives_a_new_mock_instance() {
    let root = std::env::temp_dir().join(format!("p4-mock-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let first = Mock::internal_with_cache_dir(Profile::default(), &root);
    let recorder = Recorder::default();
    first.start(hop(1, Phase::Prefill), &recorder);
    first.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "op-persist".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Persist,
        }),
        &recorder,
    );
    assert!(matches!(
        recorder.events().last(),
        Some(Event::Cached { .. })
    ));

    let same_process_wrong_identity = Recorder::default();
    first.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "other-deployment".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "op-wrong-process".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Restore,
        }),
        &same_process_wrong_identity,
    );
    assert!(matches!(
        same_process_wrong_identity.events().last(),
        Some(Event::Failed { .. })
    ));

    let second = Mock::internal_with_cache_dir(Profile::default(), &root);
    let restored = Recorder::default();
    second.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "op-restore".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Restore,
        }),
        &restored,
    );
    assert!(matches!(
        restored.events().last(),
        Some(Event::Cached { .. })
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_tampered_cache_manifest_is_refused_before_restore() {
    let root = std::env::temp_dir().join(format!("p4-mock-cache-tamper-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let first = Mock::internal_with_cache_dir(Profile::default(), &root);
    let events = Recorder::default();
    first.start(hop(1, Phase::Prefill), &events);
    first.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "op-persist".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Persist,
        }),
        &events,
    );
    let path = root.join("7330.kv");
    let original = std::fs::read_to_string(&path).unwrap();
    let mut lines = original.lines().map(str::to_owned).collect::<Vec<_>>();
    lines[5] = "7331".into();
    std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();

    let second = Mock::internal_with_cache_dir(Profile::default(), &root);
    let restore = Recorder::default();
    second.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "op-restore".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Restore,
        }),
        &restore,
    );
    assert!(matches!(
        restore.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("checksum mismatch")
    ));

    std::fs::write(
        &path,
        original.replacen("p4-mock-kv-v2", "p4-mock-kv-v9", 1),
    )
    .unwrap();
    let version_mismatch = Recorder::default();
    second.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "op-restore-version".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Restore,
        }),
        &version_mismatch,
    );
    assert!(matches!(
        version_mismatch.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("unsupported durable cache manifest version")
    ));
    let _ = std::fs::remove_dir_all(root);
}
