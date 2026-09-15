use super::super::*;
use super::state::{AdapterState, PipelineSession, RequestState, request_key};
use crate::lifecycle::LlamaLifecycle;
use crate::process::{ProcessServerControl, ServerLaunch};
use crate::{Frame, Operation};
use p4_adapter::node_adapter::{
    CompletionPublisher, CompletionReservation, CompletionReservationGroup, GroupReserveError,
    PublishError, ReservedPublishReason, RetainedCompletion, retained_event_bytes,
};
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
mod coalescing;
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
mod reservation;
mod service;
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

#[derive(Debug)]
pub enum WorkerInput {
    Event(Event),
    Retained(RetainedCompletion),
}

impl WorkerInput {
    pub(super) fn event(&self) -> &Event {
        match self {
            Self::Event(event) => event,
            Self::Retained(completion) => completion.event(),
        }
    }
}

/// Preserves stopped owned-worker input, semantic state and unpublished
/// effects until the adapter owner explicitly drops it. Not replay authority.
pub(super) struct WorkerRemainder {
    pub(super) failed_input: Option<WorkerInput>,
    pub(super) held_input: Option<WorkerInput>,
    pub(super) deferred_ack_error: Option<(WorkerInput, String)>,
    pub(super) receiver: mpsc::Receiver<WorkerInput>,
    effects: std::collections::VecDeque<effects::CommittedEffect>,
    pub(super) state: AdapterState,
}

impl WorkerRemainder {
    #[cfg(test)]
    pub(super) fn effect_count(&self) -> usize {
        self.effects.len()
    }
}

/// What the first node was doing between one batch and the next.
///
/// A staged pipeline is supposed to let the first node start the next batch
/// while the tail is still finishing the last one. Whether it actually does
/// is not visible from batch widths: a node that submits rarely could be
/// held by the coalescing threshold, or simply have nothing ready because
/// every sequence is still travelling. These three numbers separate those.
#[derive(Clone, Debug, Default)]
pub(super) struct BatchPacing {
    pub scheduling: Option<super::super::commands::SchedulingSnapshot>,
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
    runtime_resource_probe: Option<super::super::resource_profile::RuntimeResourceProbe>,
    snapshot: Arc<Mutex<String>>,
    lifecycle: LlamaLifecycle<Box<dyn crate::process::ServerControl + Send>>,
    scheduler: Scheduler,
    state: AdapterState,
    effects: std::collections::VecDeque<effects::CommittedEffect>,
    effects_fenced: bool,
    // Full servicing may retire existing ACKs, but never admits another
    // command. One FIFO obstruction and one diagnostic are the only new
    // input retention slots; neither is a general-purpose side queue.
    held_input: Option<WorkerInput>,
    failed_input: Option<WorkerInput>,
    deferred_ack_error: Option<(WorkerInput, String)>,
    owned_completions: bool,
    active_publications: usize,
    active_effect_ids: u64,
    /// When this node last finished a stage call, so the next batch can
    /// report how long the node stood still before planning it.
    last_stage_done: Option<Instant>,
    /// Coalescing refusals since that moment.
    gate_refusals: u64,
    decode_coalescer: coalescing::DecodeCoalescer,
    service_budget: super::super::scheduler::service::ServiceBudget,
    service_configuration_error: Option<String>,
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
        let (service_budget, service_configuration_error) = service::configured_budget();
        Self {
            endpoint,
            receiver,
            publisher,
            runtime_resource_probe: None,
            snapshot,
            lifecycle: LlamaLifecycle::default(),
            scheduler: Scheduler::new(),
            state: AdapterState::default(),
            effects: std::collections::VecDeque::new(),
            effects_fenced: false,
            held_input: None,
            failed_input: None,
            deferred_ack_error: None,
            owned_completions: false,
            active_publications: 0,
            active_effect_ids: 0,
            last_stage_done: None,
            gate_refusals: 0,
            decode_coalescer: Default::default(),
            service_budget,
            service_configuration_error,
            shutting_down,
            #[cfg(test)]
            issue_observer: None,
        }
    }

    pub(super) fn with_runtime_resource_probe(
        mut self,
        probe: super::super::resource_profile::RuntimeResourceProbe,
    ) -> Self {
        self.runtime_resource_probe = Some(probe);
        self
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
        self.state.max_physical_result_bytes = self
            .lifecycle
            .ready_info()
            .map_or(0, |ready| ready.max_physical_result_bytes);
        // These tests deliberately inject an already-READY native stage and
        // therefore bypass LOAD admission. Mirror the former test budget and
        // install a matching profile so request-path tests still exercise the
        // production per-request checks without pretending to cover LOAD.
        let request_limit = super::request_budget::RequestCost {
            requests: 4096,
            bytes: 512 * 1024 * 1024,
            prompt_tokens: 16 * 1024 * 1024,
            output_tokens: 16 * 1024 * 1024,
        };
        self.state.request_budget = super::request_budget::RequestBudget::new(request_limit);
        self.state.resource_profile = Some(
            super::super::resource_profile::worker_fixture_resource_profile(
                self.state.max_physical_result_bytes,
            ),
        );
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

    pub fn run(self) {
        // Legacy owner lifetime: final state is destroyed after the existing
        // shutdown census. The owned adapter retains this remainder instead.
        drop(self.run_to_exit());
    }

    pub(super) fn run_owned(mut self) -> WorkerRemainder {
        self.owned_completions = true;
        self.run_to_exit()
    }

    fn run_to_exit(mut self) -> WorkerRemainder {
        let mut failed = false;
        let mut issued = false;
        let reason = 'worker: loop {
            if self.shutting_down.load(Ordering::Acquire) {
                break "shutdown_requested";
            }
            let mut handled = 0;
            if !issued {
                // A decode coalescing deadline is local runnable work, even
                // when no more events arrive. A hard issue blocker disarms
                // this timer in drive_one_batch; expiry never grants credit.
                let input = if let Some(event) = self.held_input.take() {
                    Some(event)
                } else if let Some(deadline) = self.decode_coalescer.wake_at() {
                    match self
                        .receiver
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    {
                        Ok(input) => Some(input),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break "input_closed",
                    }
                } else {
                    match self.receiver.recv() {
                        Ok(input) => Some(input),
                        Err(_) => break "input_closed",
                    }
                };
                if self.shutting_down.load(Ordering::Acquire) {
                    self.failed_input = input;
                    break "shutdown_requested";
                }
                if let Some(input) = input {
                    if self.handle(input.event()).is_err() {
                        self.failed_input = Some(input);
                        failed = true;
                        break "failed";
                    }
                    handled = 1;
                }
            }
            while handled < INGRESS_EVENT_QUANTUM {
                if self.shutting_down.load(Ordering::Acquire) {
                    break 'worker "shutdown_requested";
                }
                let input = self
                    .held_input
                    .take()
                    .map(Ok)
                    .unwrap_or_else(|| self.receiver.try_recv());
                match input {
                    Ok(input) => {
                        if self.handle(input.event()).is_err() {
                            self.failed_input = Some(input);
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
        WorkerRemainder {
            failed_input: self.failed_input,
            held_input: self.held_input,
            deferred_ack_error: self.deferred_ack_error,
            receiver: self.receiver,
            effects: self.effects,
            state: self.state,
        }
    }

    fn handle(&mut self, event: impl std::borrow::Borrow<Event>) -> Result<(), ()> {
        let event = event.borrow();
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
            LOAD_CONTENT_TYPE => self.load(event),
            UNLOAD_CONTENT_TYPE => self.unload(event),
            SESSION_CONTENT_TYPE => self.session(event),
            PREFILL_CONTENT_TYPE => self.prefill(event),
            PHYSICAL_BATCH_CONTENT_TYPE => self.physical(event),
            TAIL_BATCH_CONTENT_TYPE => self.tail(event),
            SERVICE_SAMPLE_CONTENT_TYPE => self.service_sample(event),
            RELEASE_CONTENT_TYPE => self.release(event),
            RELEASED_CONTENT_TYPE => self.released(event),
            SETTLE_CONTENT_TYPE => self.settle(event),
            SETTLED_CONTENT_TYPE => self.settled(event),
            _ => Err(format!(
                "unsupported llama adapter content type {content_type}"
            )),
        };
        if let Err(detail) = result {
            self.set_snapshot(&format!("failed:{detail}"));
            self.emit_error(event, "LLAMA_ADAPTER_EVENT_REJECTED", detail)?;
        }
        if self.effects_fenced {
            return Err(());
        }
        self.enqueue_deferred_ack_error()?;
        self.flush_effects().map_err(|_| ())?;
        Ok(())
    }

    fn prefill(&mut self, event: impl std::borrow::Borrow<Event>) -> Result<(), String> {
        let event = event.borrow();
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
        if let Some(error) = &self.service_configuration_error {
            return Err(error.clone());
        }
        self.service_budget.validate(
            session.command.stages.len(),
            self.state.max_open_batches,
            self.state.pipeline_policy.is_some()
                && self.state.prefill_fragments == 1
                && !self.state.equal_sequence_ubatch
                && !self.state.atomic_batch_exclusive,
        )?;
        if let Some(policy) = self.state.pipeline_policy {
            if self.state.max_open_batches == 0
                || policy.mixed_prefill_rows == 0
                || self.state.prefill_fragments != 1
                || policy.mixed_batch_rows == Some(0)
            {
                return Err("pipeline policy requires a finite open window, positive mixed quantum and fragment limit one".into());
            }
            if policy.mixed_batch_rows.is_some() && self.service_budget.enabled() {
                return Err("profiled mixed token budget cannot use the experimental online service controller".into());
            }
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
        let reply = serde_json::to_string(&ReplySpec::from_envelope(&event.envelope)?)
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
        // Reserve pending/active input before identity, incarnation or slot
        // writes. Tokenization above is read-only; no KV/native issue occurred.
        // The claim follows the shared immutable input through late effects.
        let cost = super::request_budget::RequestCost::input(&command, event, &reply)?;
        let profile = self
            .state
            .resource_profile
            .as_ref()
            .ok_or("loaded resource profile is missing")?;
        if cost.bytes
            > usize::try_from(profile.max_request_bytes)
                .map_err(|_| "resource profile request bytes exceed platform range")?
            || command.max_tokens > profile.max_output_tokens_per_request
        {
            return Err("request exceeds the loaded resource profile".into());
        }
        let reservation = self.state.request_budget.reserve(cost)?;
        // First admission write. This is still the sole worker mutator: no
        // handler, publication or yield intervenes before commit_admission.
        if remember_key {
            self.state
                .remember_session_key(scope, command.session_key.clone());
        }
        self.state.requests.insert(
            key.clone(),
            RequestState::new_reserved(command, event.clone(), reply, incarnation, reservation),
        );
        self.state.next_incarnation = next_incarnation;
        self.state.pending.push_back(key);
        self.commit_admission(admission_count);
        if std::env::var_os("P4_STAGED_TRACE_REQUEST_STORAGE").is_some() {
            crate::v2::record::record(&format!(
                "P4_REQUEST_STORAGE node={:?} load={} held={:?}",
                self.endpoint,
                self.state.load_generation,
                self.state.request_budget.used()
            ));
        }
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

fn reply_target(event: &Event) -> Result<Endpoint, String> {
    event
        .envelope
        .reply_target()
        .map_err(|error| error.to_string())
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
