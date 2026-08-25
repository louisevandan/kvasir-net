use super::super::{InferenceCommand, NodeRole, SessionCommand};
use p4_protocol::event::{Endpoint, Event};
use std::collections::{BTreeMap, VecDeque};

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
    pub context_size: usize,
    pub sequence_capacity: u32,
    pub next_event: u64,
    pub load_generation: u64,
    pub next_speculative_id: u64,
    verify_fence: Option<String>,
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
            context_size: 0,
            sequence_capacity: 0,
            next_event: 1,
            load_generation: 0,
            next_speculative_id: 1,
            verify_fence: None,
        }
    }
}

impl AdapterState {
    pub fn verify_fenced(&self) -> bool {
        self.verify_fence.is_some()
    }

    pub fn verify_fence_matches(&self, request_key: &str) -> bool {
        self.verify_fence.as_deref() == Some(request_key)
    }

    pub fn begin_verify_fence(&mut self, request_key: &str) -> Result<(), &'static str> {
        if request_key.is_empty() || self.verify_fence.is_some() {
            return Err("a speculative verification fence is already active");
        }
        self.verify_fence = Some(request_key.to_owned());
        Ok(())
    }

    pub fn finish_verify_fence(&mut self, request_key: &str) -> Result<(), &'static str> {
        if !self.verify_fence_matches(request_key) {
            return Err("speculative verification fence identity changed");
        }
        self.verify_fence = None;
        Ok(())
    }

    pub fn clear_verify_fence(&mut self) {
        self.verify_fence = None;
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
    fn verification_fence_is_single_owner_and_identity_bound() {
        let mut state = AdapterState::default();
        assert!(!state.verify_fenced());
        assert_eq!(state.begin_verify_fence("session\0request-a"), Ok(()));
        assert!(state.verify_fence_matches("session\0request-a"));
        assert!(!state.verify_fence_matches("session\0request-b"));
        assert!(state.begin_verify_fence("session\0request-b").is_err());
        assert!(state.finish_verify_fence("session\0request-b").is_err());
        assert_eq!(state.finish_verify_fence("session\0request-a"), Ok(()));
        assert!(!state.verify_fenced());
    }
}

pub fn request_key(session_id: &str, request_id: &str) -> String {
    format!("{session_id}\u{0}{request_id}")
}
