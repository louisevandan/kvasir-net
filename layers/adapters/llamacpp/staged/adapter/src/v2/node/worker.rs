use super::super::*;
use super::state::{AdapterState, PipelineSession, RequestState, request_key};
use crate::lifecycle::LlamaLifecycle;
use crate::process::{ProcessServerControl, ServerLaunch};
use crate::{Frame, Operation};
use p4_adapter::node_adapter::{CompletionPublisher, PublishError};
use p4_protocol::Address;
use p4_protocol::event::{Endpoint, Event, EventClass};
use serde::Serialize;
use std::ffi::OsString;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

/// How long the worker waits for room in the completion mailbox before
/// offering again. The reader is the node task on the same process, so this
/// is a drain in progress rather than a stall.
const COMPLETION_RETRY_INTERVAL: Duration = Duration::from_millis(1);

mod control;
mod drive;
mod emit;
mod observe;
mod proposal;
mod release;
mod settlement;

// One Agent owns every concrete llama.cpp node on a machine. Loading is the
// only lifecycle transition that must be admitted host-wide: each child first
// performs llama.cpp's no_alloc memory plan and then keeps the real allocation
// alive. Serializing that transition makes the next node inspect memory after
// the previous node's allocation is visible, without blocking the Agent event
// broker or any already-loaded node's inference worker.
static HOST_LOAD_GATE: OnceLock<Mutex<()>> = OnceLock::new();

fn with_host_load_gate<T>(operation: impl FnOnce() -> T) -> T {
    let gate = HOST_LOAD_GATE.get_or_init(|| Mutex::new(()));
    let _guard = gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    operation()
}

pub enum WorkerInput {
    Event(Event),
}


/// What the first node was doing between one batch and the next.
///
/// A staged pipeline is supposed to let the first node start the next batch
/// while the tail is still finishing the last one. Whether it actually does
/// is not visible from batch widths: a node that submits rarely could be
/// held by the coalescing threshold, or simply have nothing ready because
/// every sequence is still travelling. These three numbers separate those.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct BatchPacing {
    pub stage_ms: u64,
    pub idle_ms: u64,
    pub idle_gated: u64,
    pub ready_rows: usize,
    pub ready_sequences: usize,
}

pub struct Worker {
    endpoint: Endpoint,
    receiver: mpsc::Receiver<WorkerInput>,
    publisher: CompletionPublisher,
    snapshot: Arc<Mutex<String>>,
    lifecycle: LlamaLifecycle<ProcessServerControl>,
    scheduler: Scheduler,
    state: AdapterState,
    /// When this node last finished a stage call, so the next batch can
    /// report how long the node stood still before planning it.
    last_stage_done: Option<Instant>,
    /// Coalescing refusals since that moment.
    gate_refusals: u64,
    /// Set when the adapter is going away; ends a wait for mailbox room.
    shutting_down: Arc<AtomicBool>,
}

impl Worker {
    pub fn new(
        endpoint: Endpoint,
        receiver: mpsc::Receiver<WorkerInput>,
        publisher: CompletionPublisher,
        snapshot: Arc<Mutex<String>>,
        shutting_down: Arc<AtomicBool>,
    ) -> Self {
        Self {
            endpoint,
            receiver,
            publisher,
            snapshot,
            lifecycle: LlamaLifecycle::default(),
            scheduler: Scheduler::new(),
            state: AdapterState::default(),
            last_stage_done: None,
            gate_refusals: 0,
            shutting_down,
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
        // A conversation may span many requests, but one request identity may
        // not change which conversation it belongs to: that would attach this
        // turn's KV to a record another conversation owns.
        let scope = (
            self.state.load_generation,
            command.session_id.clone(),
            command.request_id.clone(),
        );
        if let Some(previous) = self.state.session_keys.get(&scope) {
            if previous.as_deref() != command.session_key.as_deref() {
                return Err("request identity reappeared under a different session key".into());
            }
        } else {
            // Traced so a run can prove the key OUTER minted is the key this
            // adapter holds. Nothing else on the wire carries it back, so
            // without this the round trip is only an absence of rejection.
            if std::env::var_os("P4_STAGED_TRACE_SESSION_KEY").is_some() {
                crate::v2::record::record(&format!(
                    "P4_SESSION_KEY_ADMITTED request={} key={}",
                    command.request_id,
                    command.session_key.as_deref().unwrap_or("-")
                ));
            }
            self.state
                .remember_session_key(scope, command.session_key.clone());
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
                prompt_issued: 0,
                ready: None,
                after_settlement: None,
                outstanding: 0,
                generated: 0,
            },
        );
        self.state.pending.push_back(key);
        self.admit_pending()?;
        Ok(())
    }

    fn physical(&mut self, event: Event) -> Result<(), String> {
        let ingress_unix_ms = observe::unix_ms();
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
        let start_unix_ms = observe::unix_ms();
        let body = self.stage_request(
            Operation::PhysicalBatch,
            Operation::PhysicalResult,
            event.payload.clone(),
        )?;
        let end_unix_ms = observe::unix_ms();
        let result = CapsuleSet::decode(&body)
            .map_err(|error| format!("invalid physical result: {error:?}"))?;
        let forwarded = match session.command.role {
            NodeRole::Middle => self.emit_bytes(
                &event,
                session.next.expect("validated middle next"),
                EventClass::Data,
                PHYSICAL_BATCH_CONTENT_TYPE,
                body,
            ),
            NodeRole::Last => self.emit_tail_results(&event, &session, result.clone(), body),
            NodeRole::First => unreachable!(),
        }
        .map_err(|_| "completion queue is full".to_owned());
        forwarded?;
        self.emit_stage_span(&event, &session_id, &result, ingress_unix_ms, start_unix_ms, end_unix_ms)
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
    Ok(Endpoint::node(agent, value.node.clone(), value.generation))
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
