use super::super::{InferenceCommand, NodeRole, SessionCommand};
use p4_protocol::event::{Endpoint, Event};
use std::collections::{HashMap, VecDeque};

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
    pub decode: Option<(i32, u32)>,
    pub generated: u32,
}

impl RequestState {
    pub fn phase(&self) -> Option<super::super::Phase> {
        self.sequence_id?;
        if self.prompt_cursor < self.command.tokens.len() {
            Some(super::super::Phase::Prefill)
        } else if self.decode.is_some() {
            Some(super::super::Phase::Decode)
        } else {
            None
        }
    }
}

pub struct AdapterState {
    pub sessions: HashMap<String, PipelineSession>,
    pub requests: HashMap<String, RequestState>,
    pub pending: VecDeque<String>,
    pub free_sequences: VecDeque<u32>,
    pub batch_capacity: usize,
    pub context_size: usize,
    pub sequence_capacity: u32,
    pub next_event: u64,
}

impl Default for AdapterState {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            requests: HashMap::new(),
            pending: VecDeque::new(),
            free_sequences: VecDeque::new(),
            batch_capacity: 0,
            context_size: 0,
            sequence_capacity: 0,
            next_event: 1,
        }
    }
}

impl AdapterState {
    pub fn first_session_with_work(&self) -> Option<String> {
        self.requests.values().find_map(|request| {
            let session = self.sessions.get(&request.command.session_id)?;
            (session.command.role == NodeRole::First && request.phase().is_some())
                .then(|| request.command.session_id.clone())
        })
    }
}

pub fn request_key(session_id: &str, request_id: &str) -> String {
    format!("{session_id}\u{0}{request_id}")
}
