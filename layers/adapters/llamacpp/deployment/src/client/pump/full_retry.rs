use crate::contract::SubmissionId;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const INITIAL_FULL_RETRY_DELAY: Duration = Duration::from_millis(20);
const MAX_FULL_RETRY_DELAY: Duration = Duration::from_secs(1);

struct Retry {
    due: Option<Instant>,
    next_delay: Duration,
}

#[derive(Default)]
pub(super) struct FullRetries {
    retries: HashMap<SubmissionId, Retry>,
}

impl FullRetries {
    pub(super) fn schedule(&mut self, submission_id: SubmissionId) {
        self.schedule_at(submission_id, Instant::now());
    }

    fn schedule_at(&mut self, submission_id: SubmissionId, now: Instant) {
        let retry = self.retries.entry(submission_id).or_insert(Retry {
            due: None,
            next_delay: INITIAL_FULL_RETRY_DELAY,
        });
        if retry.due.is_some() {
            return;
        }
        let delay = retry.next_delay;
        retry.due = Some(now + delay);
        retry.next_delay = delay.saturating_mul(2).min(MAX_FULL_RETRY_DELAY);
    }

    pub(super) fn remove(&mut self, submission_id: &str) {
        self.retries.remove(submission_id);
    }

    pub(super) fn next_wait(&self) -> Option<Duration> {
        self.retries
            .values()
            .filter_map(|retry| retry.due)
            .min()
            .map(|due| due.saturating_duration_since(Instant::now()))
    }

    pub(super) fn take_due(&mut self) -> Vec<SubmissionId> {
        self.take_due_at(Instant::now())
    }

    fn take_due_at(&mut self, now: Instant) -> Vec<SubmissionId> {
        let mut ids = Vec::new();
        for (submission_id, retry) in &mut self.retries {
            if retry.due.is_some_and(|due| due <= now) {
                retry.due = None;
                ids.push(submission_id.clone());
            }
        }
        ids
    }
}

pub(super) fn deadline_expired(deadline_unix_ms: u64) -> bool {
    deadline_unix_ms != 0
        && SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_millis() as u64 > deadline_unix_ms)
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_retries_back_off_exponentially_and_stop_growing_at_one_second() {
        let base = Instant::now();
        let mut retries = FullRetries::default();
        let id = "submission".to_owned();
        let mut now = base;

        for expected in [20, 40, 80, 160, 320, 640, 1_000, 1_000] {
            retries.schedule_at(id.clone(), now);
            let due = retries.retries[&id].due.expect("retry must be scheduled");
            assert_eq!(due.duration_since(now), Duration::from_millis(expected));
            now = due;
            assert_eq!(retries.take_due_at(now), vec![id.clone()]);
        }
    }
}
