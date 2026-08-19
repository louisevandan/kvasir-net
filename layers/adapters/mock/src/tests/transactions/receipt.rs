use super::*;

#[test]
fn a_prepared_transaction_journal_survives_restart_and_can_be_committed() {
    let root =
        std::env::temp_dir().join(format!("p4-mock-prepared-journal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let first = Mock::internal_with_cache_dir(Profile::default(), &root);
    let first_events = Recorder::default();
    first.start(hop(1, Phase::Prefill), &first_events);
    first.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restart".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::PreparePersist,
        }),
        &first_events,
    );
    assert!(root.join("74782d72657374617274.txn").exists());
    drop(first);

    let second = Mock::internal_with_cache_dir(Profile::default(), &root);
    let second_events = Recorder::default();
    second.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restart".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Commit,
        }),
        &second_events,
    );
    assert!(matches!(
        second_events.events().last(),
        Some(Event::Cached { .. })
    ));
    assert!(root.join("74782d72657374617274.txn").exists());
    assert_eq!(second.persisted(), vec!["s0"]);
    drop(second);

    let third = Mock::internal_with_cache_dir(Profile::default(), &root);
    let third_events = Recorder::default();
    third.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restart".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Commit,
        }),
        &third_events,
    );
    assert!(matches!(
        third_events.events().last(),
        Some(Event::Cached { detail, .. }) if detail.contains("already applied")
    ));
    third.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restart".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Abort,
        }),
        &third_events,
    );
    assert!(matches!(
        third_events.events().last(),
        Some(Event::Cached { detail, .. }) if detail.contains("aborted")
    ));
    third.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restart".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Abort,
        }),
        &third_events,
    );
    assert!(matches!(
        third_events.events().last(),
        Some(Event::Cached { detail, .. }) if detail.contains("already applied")
    ));
    assert!(root.join("74782d72657374617274.txn").exists());
    drop(third);

    let fourth = Mock::internal_with_cache_dir(Profile::default(), &root);
    let fourth_events = Recorder::default();
    fourth.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restart".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Abort,
        }),
        &fourth_events,
    );
    assert!(matches!(
        fourth_events.events().last(),
        Some(Event::Cached { detail, .. }) if detail.contains("already applied")
    ));
    fourth.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-restart".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Commit,
        }),
        &fourth_events,
    );
    assert!(matches!(
        fourth_events.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("already aborted")
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_identity_mismatch_does_not_destroy_a_prepared_journal() {
    let root =
        std::env::temp_dir().join(format!("p4-mock-prepared-mismatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mock = Mock::internal_with_cache_dir(Profile::default(), &root);
    let events = Recorder::default();
    mock.start(hop(1, Phase::Prefill), &events);
    for (sequence, action) in [
        ("s0", p4_adapter::CacheAction::PreparePersist),
        ("s1", p4_adapter::CacheAction::Commit),
        ("s0", p4_adapter::CacheAction::Abort),
    ] {
        mock.start(
            Work::Cache(p4_adapter::Cache {
                deployment: "d".into(),
                stage_id: "stage-1".into(),
                generation: 1,
                operation_id: "tx-retain".into(),
                sequence: sequence.into(),
                action,
            }),
            &events,
        );
    }
    assert!(matches!(events.events().last(), Some(Event::Cached { .. })));
    assert!(root.join("74782d72657461696e.txn").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_malformed_prepared_journal_is_reported_instead_of_dropped() {
    let root = std::env::temp_dir().join(format!(
        "p4-mock-malformed-journal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("626roken.txn"), "not a transaction\n").unwrap();
    let mock = Mock::internal_with_cache_dir(Profile::default(), &root);
    let events = Recorder::default();
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-malformed".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::PreparePersist,
        }),
        &events,
    );
    assert!(matches!(
        events.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("cache journal recovery failed")
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_tampered_transaction_receipt_is_refused_by_checksum() {
    let root = std::env::temp_dir().join(format!(
        "p4-mock-tampered-journal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let first = Mock::internal_with_cache_dir(Profile::default(), &root);
    let first_events = Recorder::default();
    first.start(hop(1, Phase::Prefill), &first_events);
    first.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-tampered".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::PreparePersist,
        }),
        &first_events,
    );
    drop(first);
    let receipt = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("txn"))
        .unwrap();
    let mut bytes = std::fs::read(&receipt).unwrap();
    bytes[0] = b'X';
    std::fs::write(receipt, bytes).unwrap();

    let second = Mock::internal_with_cache_dir(Profile::default(), &root);
    let second_events = Recorder::default();
    second.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-tampered".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Commit,
        }),
        &second_events,
    );
    assert!(matches!(
        second_events.events().last(),
        Some(Event::Failed { detail, .. }) if detail.contains("checksum")
    ));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reconciliation_reports_the_committed_receipt_without_mutating_it() {
    let root = std::env::temp_dir().join(format!(
        "p4-mock-reconcile-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mock = Mock::internal_with_cache_dir(Profile::default(), &root);
    let events = Recorder::default();
    mock.start(hop(1, Phase::Prefill), &events);
    for action in [
        p4_adapter::CacheAction::PreparePersist,
        p4_adapter::CacheAction::Commit,
        p4_adapter::CacheAction::Reconcile,
    ] {
        mock.start(
            Work::Cache(p4_adapter::Cache {
                deployment: "d".into(),
                stage_id: "stage-1".into(),
                generation: 1,
                operation_id: "tx-reconcile".into(),
                sequence: "s0".into(),
                action,
            }),
            &events,
        );
    }
    assert!(matches!(
        events.events().last(),
        Some(Event::CacheStatus {
            state: p4_adapter::CacheReceiptState::Committed,
            operation_id,
            sequence,
            ..
        }) if operation_id == "tx-reconcile" && sequence == "s0"
    ));
    assert!(mock.cache_state("s0").persisted);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn reconciliation_reports_an_inconsistent_receipt_when_the_manifest_is_tampered() {
    let root = std::env::temp_dir().join(format!(
        "p4-mock-reconcile-manifest-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mock = Mock::internal_with_cache_dir(Profile::default(), &root);
    let events = Recorder::default();
    mock.start(hop(1, Phase::Prefill), &events);
    for action in [
        p4_adapter::CacheAction::PreparePersist,
        p4_adapter::CacheAction::Commit,
    ] {
        mock.start(
            Work::Cache(p4_adapter::Cache {
                deployment: "d".into(),
                stage_id: "stage-1".into(),
                generation: 1,
                operation_id: "tx-manifest-tampered".into(),
                sequence: "s0".into(),
                action,
            }),
            &events,
        );
    }
    let manifest = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("kv"))
        .unwrap();
    std::fs::write(manifest, "tampered\n").unwrap();
    mock.start(
        Work::Cache(p4_adapter::Cache {
            deployment: "d".into(),
            stage_id: "stage-1".into(),
            generation: 1,
            operation_id: "tx-manifest-tampered".into(),
            sequence: "s0".into(),
            action: p4_adapter::CacheAction::Reconcile,
        }),
        &events,
    );
    assert!(matches!(
        events.events().last(),
        Some(Event::CacheStatus {
            state: p4_adapter::CacheReceiptState::Inconsistent,
            detail,
            ..
        }) if detail.contains("manifest")
    ));
    let _ = std::fs::remove_dir_all(root);
}
