//! Admission-permit and live-index release, kept together so every terminal
//! pump path drops capacity exactly once.

use super::Pump;
use std::sync::atomic::Ordering;

impl Pump {
    pub(super) fn release_outstanding(&self) {
        self.outstanding_submissions.fetch_sub(1, Ordering::SeqCst);
    }

    pub(super) fn release_submission(&self, submission_id: &str) {
        if self.remove_live(submission_id) {
            self.release_outstanding();
        }
    }

    pub(super) fn remove_live(&self, submission_id: &str) -> bool {
        self.live
            .lock()
            .expect("live submissions lock")
            .remove(submission_id)
    }
}
