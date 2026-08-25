use super::super::*;
use super::state::{AdapterState, PipelineSession, RequestState, request_key};
use crate::lifecycle::LlamaLifecycle;
use crate::process::{ProcessServerControl, ServerLaunch};
use crate::{Frame, Operation};
use p4_adapter::node_adapter::CompletionPublisher;
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event, EventClass};
use serde::Serialize;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

mod control;
mod drive;
mod observe;
mod release;
mod settlement;

pub enum WorkerInput {
    Event(Event),
}

pub struct Worker {
    endpoint: Endpoint,
    receiver: mpsc::Receiver<WorkerInput>,
    publisher: CompletionPublisher,
    snapshot: Arc<Mutex<String>>,
    lifecycle: LlamaLifecycle<ProcessServerControl>,
    scheduler: Scheduler,
    state: AdapterState,
}

impl Worker {
    pub fn new(
        endpoint: Endpoint,
        receiver: mpsc::Receiver<WorkerInput>,
        publisher: CompletionPublisher,
        snapshot: Arc<Mutex<String>>,
    ) -> Self {
        Self {
            endpoint,
            receiver,
            publisher,
            snapshot,
            lifecycle: LlamaLifecycle::default(),
            scheduler: Scheduler::new(),
            state: AdapterState::default(),
        }
    }

    pub fn run(mut self) {
        let mut failed = false;
        'worker: while let Ok(WorkerInput::Event(event)) = self.receiver.recv() {
            if self.handle(event).is_err() {
                failed = true;
                break;
            }
            while let Ok(WorkerInput::Event(event)) = self.receiver.try_recv() {
                if self.handle(event).is_err() {
                    failed = true;
                    break 'worker;
                }
            }
            if self.drive_first_batches().is_err() {
                failed = true;
                break;
            }
        }
        if matches!(self.lifecycle.state(), crate::lifecycle::LoadState::Loaded) {
            let _ = self.lifecycle.unload();
        }
        if !failed {
            self.set_snapshot("closed");
        }
    }

    fn handle(&mut self, event: Event) -> Result<(), ()> {
        let content_type = event.envelope.payload_content_type.as_str();
        let result = match content_type {
            LOAD_CONTENT_TYPE => self.load(event.clone()),
            UNLOAD_CONTENT_TYPE => self.unload(event.clone()),
            SESSION_CONTENT_TYPE => self.session(event.clone()),
            PREFILL_CONTENT_TYPE => self.prefill(event.clone()),
            PHYSICAL_BATCH_CONTENT_TYPE => self.physical(event.clone()),
            TAIL_BATCH_CONTENT_TYPE => self.tail(event.clone()),
            RELEASE_CONTENT_TYPE => self.release(event.clone()),
            RELEASED_CONTENT_TYPE => self.released(event.clone()),
            SETTLE_CONTENT_TYPE => self.settle(event.clone()),
            SETTLED_CONTENT_TYPE => self.settled(event.clone()),
            _ => Err(format!(
                "unsupported llama adapter content type {content_type}"
            )),
        };
        if let Err(detail) = result {
            self.set_snapshot(&format!("failed:{detail}"));
            self.emit_error(&event, "LLAMA_ADAPTER_EVENT_REJECTED", detail)?;
        }
        Ok(())
    }

    fn prefill(&mut self, event: Event) -> Result<(), String> {
        let mut command: InferenceCommand = serde_json::from_slice(&event.payload)
            .map_err(|error| format!("invalid inference payload: {error}"))?;
        command.validate().map_err(str::to_owned)?;
        if command.load_generation != self.state.load_generation {
            return Err("inference load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&command.session_id)
            .ok_or_else(|| "inference session is not configured".to_owned())?;
        if session.command.role != NodeRole::First {
            return Err("prefill must target the first node".into());
        }
        let key = request_key(&command.session_id, &command.request_id);
        if self.state.requests.contains_key(&key) {
            return Err("request identity is already active".into());
        }
        let route = event
            .envelope
            .return_route
            .as_ref()
            .ok_or_else(|| "inference requires an OUTER return route".to_owned())?;
        let reply = serde_json::to_string(&ReplySpec {
            ingress_agent: route.ingress_agent.to_string(),
            channel: route.channel.clone(),
            connection_generation: route.connection_generation,
            correlation_id: event.envelope.correlation_id.clone(),
            deadline_unix_ms: event.envelope.deadline_unix_ms,
        })
        .map_err(|error| format!("cannot encode reply specification: {error}"))?;
        if let Some(prompt) = command.prompt.take() {
            command.tokens = self.tokenize(prompt)?;
        }
        if command
            .tokens
            .len()
            .checked_add(command.max_tokens as usize)
            .is_none_or(|total| total > self.state.context_size)
        {
            return Err("prompt plus max_tokens exceeds loaded per-sequence context".into());
        }
        self.state.requests.insert(
            key.clone(),
            RequestState {
                command,
                sequence_id: None,
                template: event,
                reply,
                prompt_cursor: 0,
                ready: None,
                after_settlement: None,
                in_flight: false,
                generated: 0,
            },
        );
        self.state.pending.push_back(key);
        self.admit_pending()?;
        Ok(())
    }

    fn physical(&mut self, event: Event) -> Result<(), String> {
        let input = CapsuleSet::decode(&event.payload)
            .map_err(|error| format!("invalid physical capsule: {error:?}"))?;
        let session_id = single_session(&input)?;
        if input
            .0
            .iter()
            .flat_map(|capsule| &capsule.owners)
            .any(|owner| owner.load_generation != self.state.load_generation)
        {
            return Err("physical batch load generation is stale".into());
        }
        let session = self
            .state
            .sessions
            .get(&session_id)
            .ok_or_else(|| "physical batch session is not configured".to_owned())?
            .clone();
        if session.command.role == NodeRole::First {
            return Err("physical cut-set cannot target the first node".into());
        }
        if input
            .0
            .iter()
            .any(|capsule| capsule.terminal || !capsule.outcomes.is_empty())
        {
            return Err("terminal capsule cannot be replayed".into());
        }
        let body = self.stage_request(
            Operation::PhysicalBatch,
            Operation::PhysicalResult,
            event.payload.clone(),
        )?;
        let result = CapsuleSet::decode(&body)
            .map_err(|error| format!("invalid physical result: {error:?}"))?;
        match session.command.role {
            NodeRole::Middle => self.emit_bytes(
                &event,
                session.next.expect("validated middle next"),
                EventClass::Data,
                PHYSICAL_BATCH_CONTENT_TYPE,
                body,
            ),
            NodeRole::Last => self.emit_tail_results(&event, &session, result, body),
            NodeRole::First => unreachable!(),
        }
        .map_err(|_| "completion queue is full".to_owned())
    }

    fn stage_request(
        &mut self,
        operation: Operation,
        expected: Operation,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let request = Frame::new(operation, body).map_err(|error| error.to_string())?;
        let response = self
            .lifecycle
            .request(request)
            .map_err(|error| format!("stage request failed: {error:?}"))?;
        if response.header.operation == Operation::Error {
            return Err(String::from_utf8_lossy(&response.body).into_owned());
        }
        if response.header.operation != expected {
            return Err(format!(
                "stage returned unexpected {:?}",
                response.header.operation
            ));
        }
        Ok(response.body)
    }

    fn tokenize(&mut self, prompt: String) -> Result<Vec<i32>, String> {
        let body = self.stage_request(
            Operation::Tokenize,
            Operation::Tokenized,
            prompt.into_bytes(),
        )?;
        if body.len() < 4 {
            return Err("tokenized response is truncated".into());
        }
        let count = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        let expected = 4usize
            .checked_add(
                count
                    .checked_mul(4)
                    .ok_or_else(|| "tokenized response length overflow".to_owned())?,
            )
            .ok_or_else(|| "tokenized response length overflow".to_owned())?;
        if body.len() != expected || count == 0 {
            return Err("tokenized response length is invalid".into());
        }
        Ok(body[4..]
            .chunks_exact(4)
            .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap()))
            .collect())
    }

    fn set_snapshot(&self, value: &str) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            *snapshot = value.to_owned();
        }
    }
}

fn node_endpoint(value: &NodeAddress) -> Result<Endpoint, String> {
    let agent = Address::from_str(&value.agent).map_err(|error| error.to_string())?;
    if value.node.is_empty() {
        return Err("node identity is empty".into());
    }
    Ok(Endpoint::node(agent, value.node.clone()))
}

fn reply_target(event: &Event) -> Endpoint {
    event
        .envelope
        .return_route
        .clone()
        .map(Endpoint::Outer)
        .unwrap_or_else(|| event.envelope.source.clone())
}

fn single_session(capsules: &CapsuleSet) -> Result<String, String> {
    let mut session = None;
    for capsule in &capsules.0 {
        for owner in &capsule.owners {
            if session
                .as_ref()
                .is_some_and(|value| value != &owner.session_id)
            {
                return Err("physical capsule mixes pipeline sessions".into());
            }
            session = Some(owner.session_id.clone());
        }
    }
    session.ok_or_else(|| "physical capsule has no session".into())
}
