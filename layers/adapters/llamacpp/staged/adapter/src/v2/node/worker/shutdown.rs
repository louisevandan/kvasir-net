//! Shared local quiescence/exit accounting, not an end-to-end drain protocol.
//! Published mailbox events may still be in transit; neither an idle UNLOAD
//! nor shutdown claims OUTER delivery.
use super::*;

#[derive(Debug, Serialize)]
struct LocalWork {
    requests: usize,
    pending: usize,
    pending_releases: usize,
    pending_settlements: usize,
    prepared_issue: Option<String>,
    verify_fenced: bool,
    flight_batches: usize,
    flight_executions: usize,
    open_batch_view: usize,
    effects: usize,
    effects_fenced: bool,
    active_publications: usize,
    held_input: bool,
    deferred_ack_error: bool,
    active_owners: usize,
    active_frontiers: usize,
    receive: super::super::physical_receive::ReceiveWorkStatus,
}

impl LocalWork {
    fn has_pending(&self) -> bool {
        self.requests != 0
            || self.pending != 0
            || self.pending_releases != 0
            || self.pending_settlements != 0
            || self.prepared_issue.is_some()
            || self.verify_fenced
            || self.flight_batches != 0
            || self.flight_executions != 0
            || self.open_batch_view != 0
            || self.effects != 0
            || self.effects_fenced
            || self.active_publications != 0
            || self.held_input
            || self.deferred_ack_error
            || self.active_owners != 0
            || self.active_frontiers != 0
            || self.receive.has_pending()
    }
}

impl Worker {
    fn local_work(&self) -> LocalWork {
        let (flight_batches, flight_executions) = self.state.flights.active_counts();
        LocalWork {
            requests: self.state.requests.len(),
            pending: self.state.pending.len(),
            pending_releases: self.state.pending_releases.len(),
            pending_settlements: self.state.pending_settlements.len(),
            prepared_issue: self
                .state
                .prepared_issue
                .as_ref()
                .map(|issue| format!("{:?}", issue.progress)),
            verify_fenced: self.state.verify_fenced(),
            flight_batches,
            flight_executions,
            open_batch_view: self.state.open_batches.len(),
            effects: self.effects.len(),
            effects_fenced: self.effects_fenced,
            active_publications: self.active_publications,
            held_input: self.held_input.is_some(),
            deferred_ack_error: self.deferred_ack_error.is_some(),
            active_owners: self.state.stage_owners.active_slots(),
            active_frontiers: self.state.stage_frontiers.active_slots(),
            receive: self.state.physical_receives.shutdown_status(),
        }
    }

    /// Non-mutating preflight for an explicit, healthy-worker UNLOAD. Actual
    /// failure/Drop cleanup is deliberately separate: it may abandon work,
    /// but finish_run preserves that fact and never emits UNLOADED success.
    pub(super) fn require_idle_unload(&self) -> Result<(), String> {
        let work = self.local_work();
        if work.has_pending() {
            return Err(format!(
                "unload is busy;work={}",
                serde_json::to_string(&work).expect("local counts serialize")
            ));
        }
        Ok(())
    }

    pub(super) fn finish_run(&mut self, reason: &str, failed: bool) {
        // Snapshot before native cleanup. Explicit UNLOAD may clear only an
        // idle census; forced/failure cleanup must retain abandoned evidence.
        let work = self.local_work();
        let previous = self
            .snapshot
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| "poisoned".into());
        let status = if failed {
            format!("failed:{previous}")
        } else if reason == "shutdown_requested" {
            // Queued accepted input may remain; an empty local request map
            // does not prove that the receiver was drained.
            "stopped:shutdown_requested".into()
        } else if work.has_pending() {
            format!("abandoned:{reason}")
        } else {
            "closed:local_work_empty".into()
        };
        let detail = format!(
            "{status};previous={};work={}",
            serde_json::to_string(&previous).expect("snapshot text serializes"),
            serde_json::to_string(&work).expect("local counts serialize")
        );
        // Cleanup can block or fail. Do not expose the final `closed` prefix
        // until it succeeds; preserve the pre-cleanup exit and work evidence.
        self.set_snapshot(&format!("closing:{reason};exit={detail}"));
        if matches!(self.lifecycle.state(), crate::lifecycle::LoadState::Loaded)
            && let Err(error) = self.lifecycle.unload()
        {
            let failure = if failed {
                // The initiating failure remains primary, even if cleanup
                // fails independently. Neither error replaces the other.
                detail
            } else {
                format!("failed:cleanup;prior={detail}")
            };
            self.set_snapshot(&format!("{failure};unload_failed:{error:?}"));
        } else {
            self.set_snapshot(&detail);
        }
    }
}
