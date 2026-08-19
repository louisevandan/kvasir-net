//! Coordinator-owned cache transaction state machine.
//!
//! The adapter owns bytes and receipts; this module owns the multi-stage
//! barrier. A transaction prepares every stage before committing any stage,
//! and aborts every prepared stage when a later command fails.

use crate::cache_journal::{CacheJournal, JournalState};
use crate::message::{Reply, ToNode};
use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheTransactionKind {
    Persist,
    Restore,
    Discard,
}

impl CacheTransactionKind {
    fn prepare(self, sequence: String) -> ToNode {
        match self {
            Self::Persist => ToNode::PreparePersist { sequence },
            Self::Restore => ToNode::PrepareRestore { sequence },
            Self::Discard => ToNode::PrepareDiscard { sequence },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheTransactionState {
    Preparing,
    Committing,
    Aborting,
    Complete,
    Failed,
}

/// The only service-side proof that a cache-backed request may be handed to
/// the execution queue.  This is intentionally not a scheduler command: it
/// is a fail-closed admission result for the adapter/service boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheExecutionAdmission {
    Allowed,
    TransactionIncomplete,
    TransactionFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CachePhase {
    Prepare,
    Commit,
    Abort,
    Reconcile,
}

impl CachePhase {
    pub fn route_name(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Commit => "commit",
            Self::Abort => "abort",
            Self::Reconcile => "reconcile",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageCommand {
    pub stage: String,
    pub phase: CachePhase,
    pub body: ToNode,
}

pub struct CacheTransaction {
    operation_id: String,
    sequence: String,
    deployment: String,
    generation: u64,
    stages: Vec<String>,
    kind: CacheTransactionKind,
    state: CacheTransactionState,
    prepared: BTreeSet<String>,
    committed: BTreeSet<String>,
    aborted: BTreeSet<String>,
    in_flight: BTreeSet<(CachePhase, String)>,
    needs_reconcile: bool,
    reconciled: std::collections::BTreeMap<String, String>,
    journal: Option<CacheJournal>,
}

impl CacheTransaction {
    pub fn new(
        operation_id: impl Into<String>,
        sequence: impl Into<String>,
        deployment: impl Into<String>,
        generation: u64,
        stages: impl IntoIterator<Item = impl Into<String>>,
        kind: CacheTransactionKind,
    ) -> Result<Self, String> {
        let stages = stages.into_iter().map(Into::into).collect::<Vec<_>>();
        if stages.is_empty() || stages.iter().any(String::is_empty) {
            return Err("cache transaction requires non-empty stages".into());
        }
        let unique = stages.iter().collect::<BTreeSet<_>>();
        if unique.len() != stages.len() {
            return Err("cache transaction stages must be unique".into());
        }
        let operation_id = operation_id.into();
        let sequence = sequence.into();
        let deployment = deployment.into();
        if operation_id.is_empty() || sequence.is_empty() || deployment.is_empty() {
            return Err("cache transaction identity must be non-empty".into());
        }
        Ok(Self {
            operation_id,
            sequence,
            deployment,
            generation,
            stages,
            kind,
            state: CacheTransactionState::Preparing,
            prepared: BTreeSet::new(),
            committed: BTreeSet::new(),
            aborted: BTreeSet::new(),
            in_flight: BTreeSet::new(),
            needs_reconcile: false,
            reconciled: std::collections::BTreeMap::new(),
            journal: None,
        })
    }

    pub fn recover(path: impl Into<PathBuf>) -> io::Result<Option<Self>> {
        let path = path.into();
        let journal = CacheJournal::open(path)?;
        let Some(record) = journal.recover()? else {
            return Ok(None);
        };
        let kind = match record.kind.as_str() {
            "persist" => CacheTransactionKind::Persist,
            "restore" => CacheTransactionKind::Restore,
            "discard" => CacheTransactionKind::Discard,
            other => return Err(invalid(format!("unknown cache transaction kind {other}"))),
        };
        let state = match record.state.as_str() {
            "preparing" => CacheTransactionState::Preparing,
            "committing" => CacheTransactionState::Committing,
            "aborting" => CacheTransactionState::Aborting,
            "complete" => CacheTransactionState::Complete,
            "failed" => CacheTransactionState::Failed,
            other => return Err(invalid(format!("unknown cache transaction state {other}"))),
        };
        let mut transaction = Self::new(
            record.operation_id,
            record.sequence,
            record.deployment,
            record.generation,
            record.stages,
            kind,
        )
        .map_err(invalid)?;
        transaction.state = state;
        transaction.prepared = record.prepared.into_iter().collect();
        transaction.committed = record.committed.into_iter().collect();
        transaction.aborted = record.aborted.into_iter().collect();
        transaction.needs_reconcile = !transaction.is_terminal();
        transaction.journal = Some(journal);
        Ok(Some(transaction))
    }

    pub fn with_journal(mut self, path: impl Into<PathBuf>) -> io::Result<Self> {
        let journal = CacheJournal::open(path)?;
        self.journal = Some(journal);
        self.record()?;
        Ok(self)
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn sequence(&self) -> &str {
        &self.sequence
    }
    pub fn deployment(&self) -> &str {
        &self.deployment
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn stages(&self) -> &[String] {
        &self.stages
    }
    pub fn kind(&self) -> CacheTransactionKind {
        self.kind
    }
    pub fn state(&self) -> CacheTransactionState {
        self.state
    }
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            CacheTransactionState::Complete | CacheTransactionState::Failed
        )
    }

    /// Return the execution admission for this cache transaction.
    ///
    /// A restore is executable only after every stage has a committed receipt
    /// and the coordinator has durably reached `Complete`.  In particular,
    /// `prepared`, mixed `prepared/committed`, and failed states never leak a
    /// partially resident session to a Hop caller.
    pub fn execution_admission(&self) -> CacheExecutionAdmission {
        if self.state == CacheTransactionState::Failed {
            return CacheExecutionAdmission::TransactionFailed;
        }
        if self.kind != CacheTransactionKind::Restore {
            return CacheExecutionAdmission::TransactionIncomplete;
        }
        if self.state == CacheTransactionState::Complete
            && self.committed.len() == self.stages.len()
            && self.in_flight.is_empty()
        {
            CacheExecutionAdmission::Allowed
        } else {
            CacheExecutionAdmission::TransactionIncomplete
        }
    }

    pub fn start(&mut self) -> Vec<StageCommand> {
        if self.needs_reconcile {
            return self.reconcile();
        }
        let commands = match self.state {
            CacheTransactionState::Preparing => self.commands(
                CachePhase::Prepare,
                self.stages.iter().filter(|stage| {
                    !self.prepared.contains(*stage)
                        && !self
                            .in_flight
                            .contains(&(CachePhase::Prepare, (*stage).clone()))
                }),
            ),
            CacheTransactionState::Committing => self.commands(
                CachePhase::Commit,
                self.stages.iter().filter(|stage| {
                    !self.committed.contains(*stage)
                        && !self
                            .in_flight
                            .contains(&(CachePhase::Commit, (*stage).clone()))
                }),
            ),
            CacheTransactionState::Aborting => self.commands(
                CachePhase::Abort,
                self.prepared.iter().filter(|stage| {
                    !self.aborted.contains(*stage)
                        && !self
                            .in_flight
                            .contains(&(CachePhase::Abort, (*stage).clone()))
                }),
            ),
            CacheTransactionState::Complete | CacheTransactionState::Failed => Vec::new(),
        };
        for command in &commands {
            self.in_flight
                .insert((command.phase, command.stage.clone()));
        }
        commands
    }

    /// Recovered transactions must query adapter-owned receipts before any
    /// mutation is replayed. A receipt query is read-only and is deliberately
    /// not persisted as coordinator progress: if the coordinator disappears
    /// during this wave, recovery simply asks again.
    pub fn reconcile(&mut self) -> Vec<StageCommand> {
        if self.is_terminal() {
            return Vec::new();
        }
        let commands = self.commands(
            CachePhase::Reconcile,
            self.stages.iter().filter(|stage| {
                !self.reconciled.contains_key(*stage)
                    && !self
                        .in_flight
                        .contains(&(CachePhase::Reconcile, (*stage).clone()))
            }),
        );
        for command in &commands {
            self.in_flight
                .insert((command.phase, command.stage.clone()));
        }
        commands
    }

    pub fn observe_command(
        &mut self,
        command: &StageCommand,
        reply: &Reply,
    ) -> Result<Vec<StageCommand>, String> {
        if self.is_terminal() {
            return Ok(Vec::new());
        }
        if !self.stages.iter().any(|stage| stage == &command.stage) {
            return Err(format!("unknown cache stage {}", command.stage));
        }
        let in_flight = (command.phase, command.stage.clone());
        if !self.in_flight.contains(&in_flight) {
            return Err(format!(
                "cache reply is not associated with an in-flight command: phase={:?} stage={}",
                command.phase, command.stage
            ));
        }
        self.validate_reply(command, reply)?;
        self.in_flight.remove(&in_flight);
        if command.phase == CachePhase::Reconcile {
            return self.observe_reconcile(command, reply);
        }
        if matches!(reply, Reply::CacheFailed { .. } | Reply::Failed { .. }) {
            return self.fail(command.phase);
        }
        if !matches!(reply, Reply::Cached { .. }) {
            return Err("cache command did not return a cache reply".into());
        }
        match command.phase {
            // Prepare commands are issued as a wave. A failure in that wave
            // can move the transaction to Aborting before another already
            // issued prepare reply arrives. Keep that late success in the
            // prepared set so the stage is still included in the abort wave.
            CachePhase::Prepare
                if matches!(
                    self.state,
                    CacheTransactionState::Preparing | CacheTransactionState::Aborting
                ) =>
            {
                self.prepared.insert(command.stage.clone());
                if self.state == CacheTransactionState::Preparing
                    && self.prepared.len() == self.stages.len()
                {
                    self.state = CacheTransactionState::Committing;
                }
            }
            // Commit commands are also issued as a wave. Once one commit
            // fails, the remaining in-flight commit replies are stale with
            // respect to the state machine but must not be reported as a
            // protocol-order error; the abort wave is still authoritative.
            CachePhase::Commit
                if matches!(
                    self.state,
                    CacheTransactionState::Committing | CacheTransactionState::Aborting
                ) =>
            {
                self.committed.insert(command.stage.clone());
                if self.state == CacheTransactionState::Committing
                    && self.committed.len() == self.stages.len()
                {
                    self.state = CacheTransactionState::Complete;
                }
            }
            CachePhase::Abort
                if matches!(
                    self.state,
                    CacheTransactionState::Aborting | CacheTransactionState::Failed
                ) =>
            {
                self.aborted.insert(command.stage.clone());
                if self.state == CacheTransactionState::Aborting
                    && self.aborted.len() == self.prepared.len()
                {
                    self.state = CacheTransactionState::Failed;
                }
            }
            _ => {
                return Err(format!(
                    "cache command is out of transaction order: phase={:?} state={:?} stage={}",
                    command.phase, self.state, command.stage
                ));
            }
        }
        self.record().map_err(|error| error.to_string())?;
        Ok(self.start())
    }

    fn fail(&mut self, phase: CachePhase) -> Result<Vec<StageCommand>, String> {
        if phase == CachePhase::Abort || self.prepared.is_empty() {
            self.state = CacheTransactionState::Failed;
        } else {
            self.state = CacheTransactionState::Aborting;
        }
        self.record().map_err(|error| error.to_string())?;
        Ok(self.start())
    }

    fn commands<'a>(
        &self,
        phase: CachePhase,
        stages: impl Iterator<Item = &'a String>,
    ) -> Vec<StageCommand> {
        stages
            .map(|stage| StageCommand {
                stage: stage.clone(),
                phase,
                body: match phase {
                    CachePhase::Prepare => self.kind.prepare(self.sequence.clone()),
                    CachePhase::Commit => ToNode::Commit {
                        sequence: self.sequence.clone(),
                    },
                    CachePhase::Abort => ToNode::Abort {
                        sequence: self.sequence.clone(),
                    },
                    CachePhase::Reconcile => ToNode::Reconcile {
                        sequence: self.sequence.clone(),
                    },
                },
            })
            .collect()
    }

    fn validate_reply(&self, command: &StageCommand, reply: &Reply) -> Result<(), String> {
        let matches_identity = |deployment: &String,
                                stage_id: &String,
                                generation: u64,
                                operation_id: &String,
                                sequence: &String| {
            deployment == &self.deployment
                && stage_id == &command.stage
                && generation == self.generation
                && operation_id == &self.operation_id
                && sequence == &self.sequence
        };
        let valid = match reply {
            Reply::Cached {
                deployment,
                stage_id,
                generation,
                operation_id,
                sequence,
                ..
            }
            | Reply::CacheFailed {
                deployment,
                stage_id,
                generation,
                operation_id,
                sequence,
                ..
            }
            | Reply::CacheStatus {
                deployment,
                stage_id,
                generation,
                operation_id,
                sequence,
                ..
            } => matches_identity(deployment, stage_id, *generation, operation_id, sequence),
            Reply::Failed { .. } => true,
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "cache reply identity or type mismatch: phase={:?} stage={}",
                command.phase, command.stage
            ))
        }
    }

    fn observe_reconcile(
        &mut self,
        command: &StageCommand,
        reply: &Reply,
    ) -> Result<Vec<StageCommand>, String> {
        let Reply::CacheStatus { state, .. } = reply else {
            return self.fail_reconciliation();
        };
        match state.as_str() {
            "absent" | "prepared" | "committed" | "aborted" => {}
            "inconsistent" => {
                self.state = CacheTransactionState::Failed;
                self.needs_reconcile = false;
                self.record().map_err(|error| error.to_string())?;
                return Ok(Vec::new());
            }
            other => {
                self.state = CacheTransactionState::Failed;
                self.needs_reconcile = false;
                self.record().map_err(|error| error.to_string())?;
                return Err(format!("unknown cache receipt state {other}"));
            }
        }
        self.reconciled.insert(command.stage.clone(), state.clone());
        if self.reconciled.len() != self.stages.len() {
            return Ok(self.reconcile());
        }
        self.needs_reconcile = false;
        // A recovered receipt may prove progress that was not yet written to
        // the coordinator journal. Import only monotonic states. Ambiguous
        // combinations fail closed rather than pretending that cross-file
        // commit was atomic.
        let receipts = self.reconciled.values().collect::<Vec<_>>();
        if self.kind == CacheTransactionKind::Restore
            && !receipts.iter().all(|state| *state == "committed")
        {
            self.state = CacheTransactionState::Failed;
            self.record().map_err(|error| error.to_string())?;
            return Ok(Vec::new());
        }
        match self.state {
            CacheTransactionState::Preparing => {
                if receipts.iter().all(|state| *state == "committed") {
                    self.prepared = self.stages.iter().cloned().collect();
                    self.committed = self.stages.iter().cloned().collect();
                    self.state = CacheTransactionState::Complete;
                } else if receipts.iter().all(|state| *state == "prepared") {
                    self.prepared = self.stages.iter().cloned().collect();
                    self.state = CacheTransactionState::Committing;
                } else if receipts
                    .iter()
                    .all(|state| *state == "prepared" || *state == "absent")
                {
                    self.prepared = self
                        .reconciled
                        .iter()
                        .filter_map(|(stage, state)| (state == "prepared").then_some(stage.clone()))
                        .collect();
                } else {
                    self.state = CacheTransactionState::Failed;
                }
            }
            CacheTransactionState::Committing => {
                if receipts.iter().all(|state| *state == "committed") {
                    self.prepared = self.stages.iter().cloned().collect();
                    self.committed = self.stages.iter().cloned().collect();
                    self.state = CacheTransactionState::Complete;
                } else if receipts
                    .iter()
                    .all(|state| *state == "prepared" || *state == "committed")
                {
                    self.committed = self
                        .reconciled
                        .iter()
                        .filter_map(|(stage, state)| {
                            (state == "committed").then_some(stage.clone())
                        })
                        .collect();
                } else {
                    self.state = CacheTransactionState::Failed;
                }
            }
            CacheTransactionState::Aborting => {
                if receipts.iter().all(|state| *state == "aborted") {
                    self.aborted = self.prepared.clone();
                    self.state = CacheTransactionState::Failed;
                } else if receipts
                    .iter()
                    .all(|state| *state == "prepared" || *state == "aborted")
                {
                    self.aborted = self
                        .reconciled
                        .iter()
                        .filter_map(|(stage, state)| (state == "aborted").then_some(stage.clone()))
                        .collect();
                } else {
                    self.state = CacheTransactionState::Failed;
                }
            }
            CacheTransactionState::Complete | CacheTransactionState::Failed => {}
        }
        self.record().map_err(|error| error.to_string())?;
        Ok(self.start())
    }

    fn fail_reconciliation(&mut self) -> Result<Vec<StageCommand>, String> {
        self.state = CacheTransactionState::Failed;
        self.needs_reconcile = false;
        self.record().map_err(|error| error.to_string())?;
        Ok(Vec::new())
    }

    fn record(&self) -> io::Result<()> {
        let Some(journal) = &self.journal else {
            return Ok(());
        };
        journal.append(&JournalState {
            operation_id: self.operation_id.clone(),
            sequence: self.sequence.clone(),
            deployment: self.deployment.clone(),
            generation: self.generation,
            kind: match self.kind {
                CacheTransactionKind::Persist => "persist",
                CacheTransactionKind::Restore => "restore",
                CacheTransactionKind::Discard => "discard",
            }
            .into(),
            state: match self.state {
                CacheTransactionState::Preparing => "preparing",
                CacheTransactionState::Committing => "committing",
                CacheTransactionState::Aborting => "aborting",
                CacheTransactionState::Complete => "complete",
                CacheTransactionState::Failed => "failed",
            }
            .into(),
            stages: self.stages.clone(),
            prepared: self.prepared.iter().cloned().collect(),
            committed: self.committed.iter().cloned().collect(),
            aborted: self.aborted.iter().cloned().collect(),
        })
    }
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached(command: &StageCommand) -> Reply {
        Reply::Cached {
            deployment: "dep".into(),
            stage_id: command.stage.clone(),
            generation: 1,
            operation_id: "op".into(),
            sequence: "seq".into(),
            bytes: 1,
            detail: String::new(),
        }
    }

    #[test]
    fn prepare_commit_barrier_reaches_complete() {
        let mut tx = CacheTransaction::new(
            "op",
            "seq",
            "dep",
            1,
            ["s0", "s1"],
            CacheTransactionKind::Persist,
        )
        .unwrap();
        let prepare = tx.start();
        assert_eq!(prepare.len(), 2);
        let commit = tx
            .observe_command(
                &prepare[0],
                &Reply::Cached {
                    deployment: "dep".into(),
                    stage_id: "s0".into(),
                    generation: 1,
                    operation_id: "op".into(),
                    sequence: "seq".into(),
                    bytes: 1,
                    detail: String::new(),
                },
            )
            .unwrap();
        assert_eq!(commit.len(), 0);
        let commit = tx
            .observe_command(
                &prepare[1],
                &Reply::Cached {
                    deployment: "dep".into(),
                    stage_id: "s1".into(),
                    generation: 1,
                    operation_id: "op".into(),
                    sequence: "seq".into(),
                    bytes: 1,
                    detail: String::new(),
                },
            )
            .unwrap();
        assert_eq!(tx.state(), CacheTransactionState::Committing);
        assert_eq!(commit.len(), 2);
        assert_eq!(
            tx.execution_admission(),
            CacheExecutionAdmission::TransactionIncomplete
        );

        tx.observe_command(&commit[0], &cached(&commit[0])).unwrap();
        tx.observe_command(&commit[1], &cached(&commit[1])).unwrap();
        assert_eq!(tx.state(), CacheTransactionState::Complete);
        assert_eq!(
            tx.execution_admission(),
            CacheExecutionAdmission::TransactionIncomplete
        );
    }

    #[test]
    fn only_all_committed_receipts_admit_restore_execution() {
        let mut tx = CacheTransaction::new(
            "op",
            "seq",
            "dep",
            1,
            ["s0", "s1"],
            CacheTransactionKind::Restore,
        )
        .unwrap();
        let prepare = tx.start();
        let mut commit = Vec::new();
        for command in &prepare {
            let next = tx.observe_command(command, &cached(command)).unwrap();
            if !next.is_empty() {
                commit = next;
            }
        }
        for command in &commit {
            tx.observe_command(command, &cached(command)).unwrap();
        }
        assert_eq!(tx.state(), CacheTransactionState::Complete);
        assert_eq!(tx.execution_admission(), CacheExecutionAdmission::Allowed);
    }

    #[test]
    fn recovered_partial_restore_is_failed_closed() {
        let dir =
            std::env::temp_dir().join(format!("p4-cache-partial-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("transaction.journal");
        std::fs::create_dir_all(&dir).unwrap();
        let mut tx = CacheTransaction::new(
            "op",
            "seq",
            "dep",
            1,
            ["s0", "s1"],
            CacheTransactionKind::Restore,
        )
        .unwrap()
        .with_journal(&path)
        .unwrap();
        let prepare = tx.start();
        let mut commit = Vec::new();
        for command in &prepare {
            commit = tx.observe_command(command, &cached(command)).unwrap();
        }
        tx.observe_command(&commit[0], &cached(&commit[0])).unwrap();

        let mut recovered = CacheTransaction::recover(&path).unwrap().unwrap();
        let reconcile = recovered.start();
        assert_eq!(reconcile.len(), 2);
        let first = &reconcile[0];
        let second = &reconcile[1];
        recovered
            .observe_command(
                first,
                &Reply::CacheStatus {
                    deployment: "dep".into(),
                    stage_id: first.stage.clone(),
                    generation: 1,
                    operation_id: "op".into(),
                    sequence: "seq".into(),
                    state: "committed".into(),
                    bytes: 1,
                    detail: String::new(),
                },
            )
            .unwrap();
        recovered
            .observe_command(
                second,
                &Reply::CacheStatus {
                    deployment: "dep".into(),
                    stage_id: second.stage.clone(),
                    generation: 1,
                    operation_id: "op".into(),
                    sequence: "seq".into(),
                    state: "prepared".into(),
                    bytes: 1,
                    detail: String::new(),
                },
            )
            .unwrap();
        assert_eq!(recovered.state(), CacheTransactionState::Failed);
        assert_eq!(
            recovered.execution_admission(),
            CacheExecutionAdmission::TransactionFailed
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn commit_failure_emits_abort_for_prepared_stages_and_is_recoverable() {
        let dir = std::env::temp_dir().join(format!("p4-cache-test-{}", std::process::id()));
        let path = dir.join("transaction.journal");
        let mut tx = CacheTransaction::new(
            "op",
            "seq",
            "dep",
            1,
            ["s0", "s1"],
            CacheTransactionKind::Persist,
        )
        .unwrap()
        .with_journal(&path)
        .unwrap();
        let prepare = tx.start();
        let mut commit = Vec::new();
        for command in &prepare {
            commit = tx
                .observe_command(
                    command,
                    &Reply::Cached {
                        deployment: "dep".into(),
                        stage_id: command.stage.clone(),
                        generation: 1,
                        operation_id: "op".into(),
                        sequence: "seq".into(),
                        bytes: 1,
                        detail: String::new(),
                    },
                )
                .unwrap();
        }
        let abort = tx
            .observe_command(
                &commit[0],
                &Reply::CacheFailed {
                    deployment: "dep".into(),
                    stage_id: "s0".into(),
                    generation: 1,
                    operation_id: "op".into(),
                    sequence: "seq".into(),
                    detail: "failed".into(),
                },
            )
            .unwrap();
        assert_eq!(tx.state(), CacheTransactionState::Aborting);
        assert_eq!(abort.len(), 2);
        let recovered = CacheTransaction::recover(&path).unwrap().unwrap();
        assert_eq!(recovered.state(), CacheTransactionState::Aborting);
        let _ = std::fs::remove_dir_all(dir);
    }
}
