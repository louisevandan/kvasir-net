//! Deterministic coordinator crash-window checks.
//!
//! These tests exercise journal replay after a process disappears. They do
//! not claim physical power-loss or cross-file atomicity.

use p4_service::cache::{
    CachePhase, CacheTransaction, CacheTransactionKind, CacheTransactionState, StageCommand,
};
use p4_service::message::Reply;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn journal_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("p4-cache-recovery-{label}-{nonce}.journal"))
}

fn cached(command: &StageCommand) -> Reply {
    Reply::Cached {
        deployment: "deployment".into(),
        stage_id: command.stage.clone(),
        generation: 7,
        operation_id: "operation".into(),
        sequence: "sequence".into(),
        bytes: 1,
        detail: String::new(),
    }
}

fn failed(command: &StageCommand) -> Reply {
    Reply::CacheFailed {
        deployment: "deployment".into(),
        stage_id: command.stage.clone(),
        generation: 7,
        operation_id: "operation".into(),
        sequence: "sequence".into(),
        detail: "injected failure".into(),
    }
}

fn status(command: &StageCommand, state: &str) -> Reply {
    Reply::CacheStatus {
        deployment: "deployment".into(),
        stage_id: command.stage.clone(),
        generation: 7,
        operation_id: "operation".into(),
        sequence: "sequence".into(),
        state: state.into(),
        bytes: 1,
        detail: String::new(),
    }
}

fn stages(commands: &[StageCommand], phase: CachePhase) -> Vec<String> {
    commands
        .iter()
        .filter(|command| command.phase == phase)
        .map(|command| command.stage.clone())
        .collect()
}

#[test]
fn crash_windows_replay_only_unobserved_prepare_and_commit_work() {
    let path = journal_path("commit");
    let mut transaction = CacheTransaction::new(
        "operation",
        "sequence",
        "deployment",
        7,
        ["stage-0", "stage-1"],
        CacheTransactionKind::Persist,
    )
    .unwrap()
    .with_journal(&path)
    .unwrap();

    let prepare = transaction.start();
    assert_eq!(
        stages(&prepare, CachePhase::Prepare),
        ["stage-0", "stage-1"]
    );
    drop(transaction);

    let mut transaction = CacheTransaction::recover(&path).unwrap().unwrap();
    let reconcile = transaction.start();
    assert_eq!(
        stages(&reconcile, CachePhase::Reconcile),
        ["stage-0", "stage-1"]
    );
    let prepare = transaction
        .observe_command(&reconcile[0], &status(&reconcile[0], "absent"))
        .unwrap();
    let prepare = transaction
        .observe_command(&reconcile[1], &status(&reconcile[1], "absent"))
        .unwrap_or(prepare);
    let stage_zero = prepare
        .iter()
        .find(|command| command.stage == "stage-0")
        .unwrap();
    let commit = transaction
        .observe_command(stage_zero, &cached(stage_zero))
        .unwrap();
    assert!(commit.is_empty());

    drop(transaction);
    let mut transaction = CacheTransaction::recover(&path).unwrap().unwrap();
    let reconcile = transaction.start();
    assert_eq!(
        stages(&reconcile, CachePhase::Reconcile),
        ["stage-0", "stage-1"]
    );
    let prepare = transaction
        .observe_command(&reconcile[0], &status(&reconcile[0], "prepared"))
        .unwrap();
    let prepare = transaction
        .observe_command(&reconcile[1], &status(&reconcile[1], "absent"))
        .unwrap_or(prepare);
    let stage_one = prepare
        .iter()
        .find(|command| command.stage == "stage-1")
        .unwrap();
    let commit = transaction
        .observe_command(stage_one, &cached(stage_one))
        .unwrap();
    assert_eq!(transaction.state(), CacheTransactionState::Committing);
    assert_eq!(stages(&commit, CachePhase::Commit), ["stage-0", "stage-1"]);

    let stage_zero = commit
        .iter()
        .find(|command| command.stage == "stage-0")
        .unwrap();
    let next = transaction
        .observe_command(stage_zero, &cached(stage_zero))
        .unwrap();
    assert!(next.is_empty());
    drop(transaction);

    let mut transaction = CacheTransaction::recover(&path).unwrap().unwrap();
    let reconcile = transaction.start();
    assert_eq!(
        stages(&reconcile, CachePhase::Reconcile),
        ["stage-0", "stage-1"]
    );
    let next = transaction
        .observe_command(&reconcile[0], &status(&reconcile[0], "committed"))
        .unwrap();
    let next = transaction
        .observe_command(&reconcile[1], &status(&reconcile[1], "committed"))
        .unwrap_or(next);
    assert!(next.is_empty());
    assert_eq!(transaction.state(), CacheTransactionState::Complete);
    let _ = std::fs::remove_file(path);
}

#[test]
fn crash_during_abort_replays_compensation_until_failed_is_durable() {
    let path = journal_path("abort");
    let mut transaction = CacheTransaction::new(
        "operation",
        "sequence",
        "deployment",
        7,
        ["stage-0", "stage-1"],
        CacheTransactionKind::Persist,
    )
    .unwrap()
    .with_journal(&path)
    .unwrap();

    let prepare = transaction.start();
    let mut commit = Vec::new();
    for command in &prepare {
        commit = transaction
            .observe_command(command, &cached(command))
            .unwrap();
    }
    let abort = transaction
        .observe_command(&commit[0], &failed(&commit[0]))
        .unwrap();
    assert_eq!(transaction.state(), CacheTransactionState::Aborting);
    assert_eq!(stages(&abort, CachePhase::Abort), ["stage-0", "stage-1"]);
    drop(transaction);

    let mut transaction = CacheTransaction::recover(&path).unwrap().unwrap();
    let reconcile = transaction.start();
    assert_eq!(
        stages(&reconcile, CachePhase::Reconcile),
        ["stage-0", "stage-1"]
    );
    let abort = transaction
        .observe_command(&reconcile[0], &status(&reconcile[0], "prepared"))
        .unwrap();
    let abort = transaction
        .observe_command(&reconcile[1], &status(&reconcile[1], "prepared"))
        .unwrap_or(abort);
    assert_eq!(stages(&abort, CachePhase::Abort), ["stage-0", "stage-1"]);
    for command in &abort {
        let _ = transaction
            .observe_command(command, &cached(command))
            .unwrap();
    }
    assert_eq!(transaction.state(), CacheTransactionState::Failed);
    let _ = std::fs::remove_file(path);
}

#[test]
fn stale_reply_and_bad_receipt_identity_fail_closed() {
    let path = journal_path("identity");
    let mut transaction = CacheTransaction::new(
        "operation",
        "sequence",
        "deployment",
        7,
        ["stage-0"],
        CacheTransactionKind::Persist,
    )
    .unwrap()
    .with_journal(&path)
    .unwrap();
    let prepare = transaction.start();
    let mut wrong = cached(&prepare[0]);
    if let Reply::Cached { operation_id, .. } = &mut wrong {
        *operation_id = "stale-operation".into();
    }
    assert!(transaction.observe_command(&prepare[0], &wrong).is_err());
    assert!(
        transaction
            .observe_command(&prepare[0], &cached(&prepare[0]))
            .is_ok()
    );
    assert!(
        transaction
            .observe_command(&prepare[0], &cached(&prepare[0]))
            .is_err()
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn malformed_journal_state_is_rejected_before_replay() {
    let path = journal_path("invalid-state");
    let journal = p4_service::cache_journal::CacheJournal::open(&path).unwrap();
    journal
        .append(&p4_service::cache_journal::JournalState {
            operation_id: "operation".into(),
            sequence: "sequence".into(),
            deployment: "deployment".into(),
            generation: 7,
            kind: "persist".into(),
            state: "committing".into(),
            stages: vec!["stage-0".into(), "stage-1".into()],
            prepared: vec!["stage-0".into()],
            committed: Vec::new(),
            aborted: Vec::new(),
        })
        .unwrap();
    let error = match CacheTransaction::recover(&path) {
        Err(error) => error,
        Ok(_) => panic!("malformed journal unexpectedly recovered"),
    };
    assert!(error.to_string().contains("does not prepare every stage"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn reconcile_failure_does_not_start_a_mutating_abort_wave() {
    let path = journal_path("reconcile-failure");
    let mut transaction = CacheTransaction::new(
        "operation",
        "sequence",
        "deployment",
        7,
        ["stage-0"],
        CacheTransactionKind::Persist,
    )
    .unwrap()
    .with_journal(&path)
    .unwrap();
    let prepare = transaction.start();
    transaction
        .observe_command(&prepare[0], &cached(&prepare[0]))
        .unwrap();
    drop(transaction);

    let mut recovered = CacheTransaction::recover(&path).unwrap().unwrap();
    let reconcile = recovered.start();
    let next = recovered
        .observe_command(
            &reconcile[0],
            &Reply::Failed {
                detail: "stage unavailable".into(),
            },
        )
        .unwrap();
    assert!(next.is_empty());
    assert_eq!(recovered.state(), CacheTransactionState::Failed);
    let _ = std::fs::remove_file(path);
}

#[test]
fn all_committed_receipts_close_a_journal_stuck_in_preparing() {
    let path = journal_path("committed-before-journal");
    let mut transaction = CacheTransaction::new(
        "operation",
        "sequence",
        "deployment",
        7,
        ["stage-0", "stage-1"],
        CacheTransactionKind::Persist,
    )
    .unwrap()
    .with_journal(&path)
    .unwrap();
    let prepare = transaction.start();
    let reconcile = transaction.reconcile();
    let _ = transaction
        .observe_command(&reconcile[0], &status(&reconcile[0], "committed"))
        .unwrap();
    let next = transaction
        .observe_command(&reconcile[1], &status(&reconcile[1], "committed"))
        .unwrap();
    assert!(next.is_empty());
    assert_eq!(transaction.state(), CacheTransactionState::Complete);
    assert_eq!(prepare.len(), 2);
    let _ = std::fs::remove_file(path);
}
