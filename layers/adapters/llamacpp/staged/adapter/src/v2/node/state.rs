use super::super::{InferenceCommand, NodeRole, SessionCommand};
use p4_protocol::event::{Endpoint, Event};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone)]
pub struct PipelineSession {
    pub command: SessionCommand,
    pub next: Option<Endpoint>,
    pub first: Endpoint,
}

pub struct RequestState {
    pub command: InferenceCommand,
    pub sequence_id: Option<u32>,
    pub template: Event,
    pub reply: String,
    pub prompt_cursor: usize,
    pub ready: Option<ReadyRows>,
    pub after_settlement: Option<SettlementContinuation>,
    pub in_flight: bool,
    pub generated: u32,
}

pub enum SettlementContinuation {
    Proposal { position: u32 },
    Replay(ReadyRows),
}

#[derive(Clone, Debug)]
pub struct ReadyRows {
    pub phase: super::super::Phase,
    pub tokens: Vec<i32>,
    pub position: u32,
    pub speculative_id: u64,
}

impl RequestState {
    pub fn phase(&self) -> Option<super::super::Phase> {
        self.sequence_id?;
        if self.in_flight {
            return None;
        }
        if self.prompt_cursor < self.command.tokens.len() {
            Some(super::super::Phase::Prefill)
        } else {
            self.ready.as_ref().map(|value| value.phase)
        }
    }
}

pub struct AdapterState {
    pub sessions: BTreeMap<String, PipelineSession>,
    pub requests: BTreeMap<String, RequestState>,
    pub pending: VecDeque<String>,
    pub free_sequences: VecDeque<u32>,
    pub batch_capacity: usize,
    pub physical_capacity: usize,
    pub equal_sequence_ubatch: bool,
    pub max_atomic_sequences: usize,
    pub atomic_batch_exclusive: bool,
    pub context_size: usize,
    pub sequence_capacity: u32,
    pub next_event: u64,
    pub load_generation: u64,
    pub next_speculative_id: u64,
    /// Hold a plan back until this many rows are ready, so a batch stops
    /// re-forming the arrival group it was born in. 0 or 1 disables the wait.
    /// See `Worker::drive_first_batches`.
    pub min_batch_rows: usize,
    /// The conversation each request identity was admitted under, so a repeat
    /// of that identity cannot silently move to another conversation. Keyed by
    /// request_id because that is what an OUTER reuses across turns.
    pub session_keys: BTreeMap<String, Option<String>>,
    verify_fence: BTreeSet<String>,
}

impl Default for AdapterState {
    fn default() -> Self {
        Self {
            sessions: BTreeMap::new(),
            requests: BTreeMap::new(),
            pending: VecDeque::new(),
            free_sequences: VecDeque::new(),
            batch_capacity: 0,
            physical_capacity: 0,
            equal_sequence_ubatch: false,
            max_atomic_sequences: 0,
            atomic_batch_exclusive: false,
            context_size: 0,
            sequence_capacity: 0,
            next_event: 1,
            load_generation: 0,
            next_speculative_id: 1,
            session_keys: BTreeMap::new(),
            min_batch_rows: std::env::var("P4_STAGED_MIN_BATCH_ROWS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            verify_fence: BTreeSet::new(),
        }
    }
}

impl AdapterState {
    pub fn verify_fenced(&self) -> bool {
        !self.verify_fence.is_empty()
    }

    pub fn verify_fence_matches(&self, request_key: &str) -> bool {
        self.verify_fence.contains(request_key)
    }

    pub fn begin_verify_fence(&mut self, request_keys: &[String]) -> Result<(), &'static str> {
        if !self.verify_fence.is_empty() {
            return Err("a speculative verification fence is already active");
        }
        let keys: BTreeSet<_> = request_keys.iter().cloned().collect();
        if keys.is_empty()
            || keys.len() != request_keys.len()
            || keys.iter().any(|request_key| request_key.is_empty())
        {
            return Err("speculative verification fence identities are invalid");
        }
        self.verify_fence = keys;
        Ok(())
    }

    pub fn finish_verify_fence(&mut self, request_key: &str) -> Result<(), &'static str> {
        if !self.verify_fence_matches(request_key) {
            return Err("speculative verification fence identity changed");
        }
        self.verify_fence.remove(request_key);
        Ok(())
    }

    pub fn clear_verify_fence(&mut self) {
        self.verify_fence.clear();
    }

    /// Whether any admitted request is still crossing the pipeline.
    pub fn any_in_flight(&self) -> bool {
        self.requests.values().any(|request| request.in_flight)
    }

    /// Rows a plan could carry right now. Decode contributes one row per ready
    /// sequence; a pending prompt is counted as one because the scheduler
    /// gives every prompt a row before water-filling the remainder.
    pub fn ready_row_count(&self) -> usize {
        self.requests
            .values()
            .filter(|request| request.phase().is_some())
            .count()
    }

    pub fn first_session_with_work(&self) -> Option<String> {
        self.requests.values().find_map(|request| {
            let session = self.sessions.get(&request.command.session_id)?;
            (session.command.role == NodeRole::First && request.phase().is_some())
                .then(|| request.command.session_id.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AdapterState;

    #[test]
    fn verification_fence_tracks_every_owner_until_the_round_resolves() {
        let mut state = AdapterState::default();
        assert!(!state.verify_fenced());
        let keys = vec![
            "session\0request-a".to_owned(),
            "session\0request-b".to_owned(),
        ];
        assert_eq!(state.begin_verify_fence(&keys), Ok(()));
        assert!(state.verify_fence_matches("session\0request-a"));
        assert!(state.verify_fence_matches("session\0request-b"));
        assert!(
            state
                .begin_verify_fence(&["session\0request-c".to_owned()])
                .is_err()
        );
        assert_eq!(state.finish_verify_fence("session\0request-a"), Ok(()));
        assert!(state.verify_fenced());
        assert!(state.finish_verify_fence("session\0request-a").is_err());
        assert_eq!(state.finish_verify_fence("session\0request-b"), Ok(()));
        assert!(!state.verify_fenced());
    }

    #[test]
    fn verification_fence_rejects_empty_or_duplicate_owners() {
        let mut state = AdapterState::default();
        assert!(state.begin_verify_fence(&[]).is_err());
        assert!(state.begin_verify_fence(&[String::new()]).is_err());
        assert!(
            state
                .begin_verify_fence(&["request".to_owned(), "request".to_owned()])
                .is_err()
        );
        assert!(!state.verify_fenced());
    }
}

pub fn request_key(session_id: &str, request_id: &str) -> String {
    format!("{session_id}\u{0}{request_id}")
}
