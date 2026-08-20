use super::*;

#[test]
fn a_prepared_cache_is_not_visible_until_commit_and_abort_keeps_resident_state() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    mock.start(hop(1), &recorder);
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-abort".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::PreparePersist,
        }),
        &recorder,
    );
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-abort".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Abort,
        }),
        &recorder,
    );
    mock.start(hop(2), &recorder);
    assert!(matches!(
        recorder.events().last(),
        Some(Event::HopComplete { .. })
    ));
}

#[test]
fn a_committed_prepared_cache_frees_resident_state_for_restore() {
    let root = std::env::temp_dir().join(format!("p4-mock-transaction-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mock = Mock::internal_with_cache_dir(Profile::default(), &root);
    let recorder = Recorder::default();
    mock.start(hop(1), &recorder);
    for action in [
        p4_adapter::CacheAction::PreparePersist,
        p4_adapter::CacheAction::Commit,
    ] {
        mock.start(
            Work::Cache(p4_adapter::Cache {
                deployment: "d".into(),
                stage_id: "stage-1".into(),
                generation: 1,
                operation_id: "tx-commit".into(),
                sequence: "s0".into(),
                action,
            }),
            &recorder,
        );
    }
    let restored = Recorder::default();
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restore".into(),
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
fn a_partial_restore_receipt_refuses_hop_until_commit() {
    let mock = Mock::internal(Profile::default());
    let recorder = Recorder::default();
    mock.start(hop(1), &recorder);
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "persist".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::PreparePersist,
        }),
        &recorder,
    );
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "persist".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Commit,
        }),
        &recorder,
    );

    let restore = Recorder::default();
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "restore".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::PrepareRestore,
        }),
        &restore,
    );
    mock.start(hop(1), &restore);
    assert!(
        matches!(
            restore.events().last(),
            Some(Event::Failed { detail, .. }) if detail.contains("requires Restore before Hop")
        ),
        "events: {:?}",
        restore.events()
    );

    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "restore".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Commit,
        }),
        &restore,
    );
    mock.start(hop(1), &restore);
    assert!(matches!(
        restore.events().last(),
        Some(Event::HopComplete { .. })
    ));
}
