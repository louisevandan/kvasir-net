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

/// Actor service bound, not a throughput-tuned batch width or an environment
/// knob. Count the event received by blocking recv in this same turn budget.
const INGRESS_EVENT_QUANTUM: usize = 32;

mod ack_service;
mod control;
mod control_dispatch;
#[cfg(test)]
mod control_dispatch_effect_tests;
#[cfg(test)]
mod control_progress_tests;
#[cfg(test)]
mod direct_emission_tests;
mod drive;
#[cfg(test)]
mod effect_representation_tests;
mod effects;
mod emit;
#[cfg(test)]
mod incarnation_tests;
#[cfg(test)]
mod loop_tests;
mod obligations;
mod observe;
#[cfg(test)]
mod observe_tests;
mod outcome;
mod physical;
#[cfg(test)]
mod physical_replay_tests;
mod proposal;
#[cfg(test)]
mod publication_tests;
mod release;
#[cfg(test)]
mod release_notification_tests;
#[cfg(test)]
mod release_tests;
#[cfg(test)]
mod session_tests;
mod settlement;
mod shutdown;
#[cfg(test)]
mod stage_tests;
#[cfg(test)]
mod turn_tests;

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

#[cfg(test)]
type IssueObserver = Arc<dyn Fn(&'static str, &AdapterState) + Send + Sync>;

pub struct Worker {
    endpoint: Endpoint,
    receiver: mpsc::Receiver<WorkerInput>,
    publisher: CompletionPublisher,
    snapshot: Arc<Mutex<String>>,
    lifecycle: LlamaLifecycle<Box<dyn crate::process::ServerControl + Send>>,
    scheduler: Scheduler,
    state: AdapterState,
    effects: std::collections::VecDeque<effects::CommittedEffect>,
    effects_fenced: bool,
    // Full servicing may retire existing ACKs, but never admits another
    // command. One FIFO obstruction and one diagnostic are the only new
    // input retention slots; neither is a general-purpose side queue.
    held_input: Option<Event>,
    deferred_ack_error: Option<(p4_protocol::event::Envelope, String)>,
    active_publications: usize,
    active_effect_ids: u64,
    /// When this node last finished a stage call, so the next batch can
    /// report how long the node stood still before planning it.
    last_stage_done: Option<Instant>,
    /// Coalescing refusals since that moment.
    gate_refusals: u64,
    /// Set when the adapter is going away; ends a wait for mailbox room.
    shutting_down: Arc<AtomicBool>,
    #[cfg(test)]
    issue_observer: Option<IssueObserver>,
}

impl Worker {
    /// Transport routing does not establish an adapter stage's semantic role.
    /// Expected endpoints are installed from SESSION, never from this event.
    fn require_stage_source(
        &self,
        event: &Event,
        expected: &Endpoint,
        operation: &str,
    ) -> Result<(), String> {
        if &event.envelope.source != expected || event.envelope.target != self.endpoint {
            return Err(format!(
                "{operation} route does not match the declared pipeline"
            ));
        }
        Ok(())
    }

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
            effects: std::collections::VecDeque::new(),
            effects_fenced: false,
            held_input: None,
            deferred_ack_error: None,
            active_publications: 0,
            active_effect_ids: 0,
            last_stage_done: None,
            gate_refusals: 0,
            shutting_down,
            #[cfg(test)]
            issue_observer: None,
        }
    }

    /// Inject at the existing native Frame boundary, preserving lifecycle and
    /// worker execution. This does not exercise LOAD event parsing/capacity
    /// validation; that remains a separate integration obligation.
    #[cfg(test)]
    fn with_stage_for_test(
        mut self,
        stage: Box<dyn crate::process::ServerControl + Send>,
    ) -> Result<Self, crate::lifecycle::LifecycleError> {
        self.lifecycle.load(stage, Duration::from_millis(100))?;
        Ok(self)
    }

    /// Test-only handles on the parts a settlement touches.
    ///
    /// `tail` and `state` are `pub(super)` and the tests live in this module,
    /// but a worker cannot be built from outside without them being reachable
    /// by name - and the alternative, making the fields public, would widen
    /// them for everyone.
    #[cfg(test)]
    pub(super) fn state_for_test(&mut self) -> &mut AdapterState {
        &mut self.state
    }

    #[cfg(test)]
    pub(super) fn tail_for_test(&mut self, event: Event) -> Result<(), String> {
        self.tail(event)
    }

    #[cfg(test)]
    pub(super) fn tail_commit_for_test(&mut self, event: Event) -> Result<(), String> {
        self.tail_without_flush(event)
    }

    #[cfg(test)]
    pub(super) fn flush_for_test(&mut self) -> Result<(), String> {
        self.flush_effects()
    }

    #[cfg(test)]
    pub(super) fn handle_for_test(&mut self, event: Event) -> Result<(), ()> {
        self.handle(event)
    }

    #[cfg(test)]
    pub(super) fn emit_tail_results_for_test(
        &mut self,
        base: &Event,
        session: &PipelineSession,
        result: CapsuleSet,
        body: Vec<u8>,
    ) -> Result<(), ()> {
        self.emit_tail_results(base, session, result, body)
    }

    #[cfg(test)]
    pub(super) fn effects_for_test(&self) -> (usize, bool) {
        (self.effects.len(), self.effects_fenced)
    }

    #[cfg(test)]
    pub(super) fn effect_intents_for_test(&self) -> String {
        format!("{:?}", self.effects)
    }

    #[cfg(test)]
    pub(super) fn request_for_test(&self) -> &super::state::RequestState {
        self.state
            .requests
            .values()
            .next()
            .expect("the test inserted one request")
    }

    #[cfg(test)]
    fn observe_issue_state(&self, point: &'static str) {
        if let Some(observer) = &self.issue_observer {
            observer(point, &self.state);
        }
    }

    pub fn run(mut self) {
        let mut failed = false;
        let mut issued = false;
        let reason = 'worker: loop {
            if self.shutting_down.load(Ordering::Acquire) {
                break "shutdown_requested";
            }
            let mut handled = 0;
            if !issued {
                // No self-generated progress remains. recv also catches input
                // arriving after the preceding Empty check without a lost wake.
                let input = self
                    .held_input
                    .take()
                    .map(WorkerInput::Event)
                    .map(Ok)
                    .unwrap_or_else(|| self.receiver.recv());
                let Ok(WorkerInput::Event(event)) = input else {
                    break "input_closed";
                };
                if self.shutting_down.load(Ordering::Acquire) {
                    break "shutdown_requested";
                }
                if self.handle(event).is_err() {
                    failed = true;
                    break "failed";
                }
                handled = 1;
            }
            while handled < INGRESS_EVENT_QUANTUM {
                if self.shutting_down.load(Ordering::Acquire) {
                    break 'worker "shutdown_requested";
                }
                let input = self
                    .held_input
                    .take()
                    .map(WorkerInput::Event)
                    .map(Ok)
                    .unwrap_or_else(|| self.receiver.try_recv());
                match input {
                    Ok(WorkerInput::Event(event)) => {
                        if self.handle(event).is_err() {
                            failed = true;
                            break 'worker "failed";
                        }
                        handled += 1;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    // Closed is cancellation of this worker's input, not
                    // permission to start another irreversible native issue.
                    Err(mpsc::TryRecvError::Disconnected) => break 'worker "input_closed",
                }
            }
            issued = match self.drive_one_batch() {
                Ok(issued) => issued,
                Err(()) => {
                    failed = true;
                    break "failed";
                }
            };
            // A successful non-output prefill is progress too. Continue even
            // with an empty inbox, but reconsider input before the next issue.
        };
        #[cfg(test)]
        self.observe_issue_state("run_stopping");
        self.finish_run(reason, failed);
    }

    fn handle(&mut self, event: Event) -> Result<(), ()> {
        if self.effects_fenced
            || self
                .state
                .prepared_issue
                .as_ref()
                .is_some_and(|issue| issue.progress == super::state::IssueProgress::Uncertain)
        {
            return Err(());
        }
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
        if self.effects_fenced {
            return Err(());
        }
        self.enqueue_deferred_ack_error()?;
        self.flush_effects().map_err(|_| ())?;
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
        if session.command.role() != NodeRole::First {
            return Err("prefill must target the first node".into());
        }
        let submitted_route = event
            .envelope
            .return_route
            .as_ref()
            .ok_or("inference requires an OUTER return route")?;
        if event.envelope.source != Endpoint::Outer(submitted_route.clone()) {
            return Err("inference source does not match its OUTER return route".into());
        }
        if event.envelope.target != self.endpoint {
            return Err("inference target does not name this worker endpoint".into());
        }
        // A generic event may carry strings that the approved-output/issue
        // identity cannot encode. Reject them before Tokenize, admission
        // records or native KV, not by stopping the worker after execution.
        super::super::issue_witness::validate_submission_identity(
            &event.envelope.target,
            submitted_route,
            command.load_generation,
            &command.session_id,
            &command.request_id,
            &event.envelope.event_id,
        )
        .map_err(str::to_owned)?;
        let reply = serde_json::to_string(&ReplySpec {
            ingress_agent: submitted_route.ingress_agent.to_string(),
            channel: submitted_route.channel.clone(),
            connection_generation: submitted_route.connection_generation,
            correlation_id: event.envelope.correlation_id.clone(),
            deadline_unix_ms: event.envelope.deadline_unix_ms,
        })
        .map_err(|error| format!("cannot encode reply specification: {error}"))?;
        // Both codecs keep the same byte limit. Refuse before session-key
        // records, Tokenize, slot/incarnation admission or native execution.
        super::super::capsule::validate_reply_options(&reply, &command.options)
            .map_err(str::to_owned)?;
        let key = request_key(&command.session_id, &command.request_id);
        if self.state.requests.contains_key(&key)
            || self.state.pending_releases.contains_key(&key)
            || self.state.pending.contains(&key)
        {
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
        let remember_key = !self.state.session_keys.contains_key(&scope);
        if let Some(previous) = self.state.session_keys.get(&scope) {
            if previous.as_deref() != command.session_key.as_deref() {
                return Err("request identity reappeared under a different session key".into());
            }
        }
        // Validate the future pending prefix WITHOUT inserting this request.
        // A rejection must not remember a key, consume an incarnation, or
        // leave a request behind for the scheduler to execute later.
        let incarnation = self.state.next_incarnation;
        let next_incarnation = incarnation
            .checked_add(1)
            .filter(|_| incarnation != 0)
            .ok_or("request incarnation exhausted")?;
        let admission_count = self.prepare_prefill_admission(&key)?;
        if let Some(prompt) = command.prompt.take() {
            // Tokenize is a synchronous native query, not KV issue.
            // Its failure still must leave all request admission state alone.
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
        let admission_record = (remember_key
            && std::env::var_os("P4_STAGED_TRACE_SESSION_KEY").is_some())
        .then(|| {
            format!(
                "P4_SESSION_KEY_ADMITTED request={} key={}",
                command.request_id,
                command.session_key.as_deref().unwrap_or("-")
            )
        });
        // First admission write. This is still the sole worker mutator: no
        // handler, publication or yield intervenes before commit_admission.
        // Future request-storage reservation must also precede this line;
        // these validation checks do not establish a count/byte memory budget.
        if remember_key {
            self.state
                .remember_session_key(scope, command.session_key.clone());
        }
        self.state.requests.insert(
            key.clone(),
            RequestState {
                command,
                incarnation,
                sequence_id: None,
                template: event,
                reply,
                prompt_cursor: 0,
                prompt_issued: 0,
                ready: None,
                after_settlement: None,
                outstanding: 0,
                generated: 0,
                issued_work: None,
            },
        );
        self.state.next_incarnation = next_incarnation;
        self.state.pending.push_back(key);
        self.commit_admission(admission_count);
        if let Some(record) = admission_record {
            // This record says ADMITTED, not merely parsed or attempted.
            crate::v2::record::record(&record);
        }
        Ok(())
    }

    fn stage_request(
        &mut self,
        operation: Operation,
        expected: Operation,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let request = Frame::new(operation, body).map_err(|error| error.to_string())?;
        // Drop may be signalled while planning or decoding a command. This is
        // the final observation before entering native; it cannot interrupt a
        // synchronous call that has already begun.
        if self.shutting_down.load(Ordering::Acquire) {
            return Err("native request refused after shutdown was observed".into());
        }
        let mutating = matches!(
            operation,
            Operation::LogicalBatch
                | Operation::PhysicalBatch
                | Operation::PhysicalSettle
                | Operation::PhysicalRelease
        );
        if mutating && self.effects_fenced {
            return Err("native mutation is fenced".into());
        }
        let response = self.lifecycle.request(request).map_err(|error| {
            // The native wire does not distinguish rejection-before-execution
            // from a response lost after execution. Do not issue more work or
            // automatically retry an ambiguous mutating operation.
            if mutating {
                self.effects_fenced = true;
            }
            format!("stage request failed: {error:?}")
        })?;
        if response.header.operation == Operation::Error {
            if mutating {
                self.effects_fenced = true;
            }
            return Err(String::from_utf8_lossy(&response.body).into_owned());
        }
        if response.header.operation != expected {
            if mutating {
                self.effects_fenced = true;
            }
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
