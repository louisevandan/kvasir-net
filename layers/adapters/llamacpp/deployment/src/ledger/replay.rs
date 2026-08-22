//! Stable reconnect replay views over the client-side submission ledger.

use super::{Entry, Ledger};
use crate::contract::{SubmissionId, Submit};

impl Ledger {
    /// Submissions whose cancel is still owed to the backend.
    pub fn cancels_for_replay(&self) -> Vec<SubmissionId> {
        let mut ids: Vec<SubmissionId> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.cancel_requested
                    && entry.generation == self.generation
                    && entry.state != super::SubmissionState::Done
            })
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Every current-generation submission sent but not yet terminal.
    pub fn in_flight_for_replay(&self) -> Vec<Submit> {
        let mut submissions: Vec<&Entry> = self
            .entries
            .values()
            .filter(|entry| {
                entry.generation == self.generation && entry.state != super::SubmissionState::Done
            })
            .collect();
        submissions.sort_by(|a, b| a.submit.submission_id.cmp(&b.submit.submission_id));
        submissions
            .into_iter()
            .map(|entry| entry.submit.clone())
            .collect()
    }
}
