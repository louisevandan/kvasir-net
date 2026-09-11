use super::super::{InferenceCommand, NodeRole, SessionCommand};
use p4_protocol::event::{Endpoint, Envelope, Event};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[derive(Clone)]
pub struct PipelineSession {
    pub command: SessionCommand,
    pub next: Option<Endpoint>,
    pub first: Endpoint,
    pub previous: Option<Endpoint>,
    pub last: Endpoint,
}

/// Normalized admission data. Native tokenization, when needed, finishes
/// before construction; later issue/settlement candidates only share it.
/// This owns no transport claim or resource reservation.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(Clone))]
pub struct RequestInput {
    pub command: InferenceCommand,
    pub template: Event,
    pub reply: String,
    // Last field: owned data retires before returning its storage claim.
    reservation: Option<super::request_budget::RequestReservation>,
}

/// Read-only input ownership for worker-side provenance and batch diagnostics.
/// Clone shares one allocation; it neither duplicates payload nor mints a
/// transport/resource claim. The Arc itself is not exposed to production.
#[derive(Clone)]
pub(crate) struct SharedRequestInput(Arc<RequestInput>);

impl std::ops::Deref for SharedRequestInput {
    type Target = RequestInput;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::borrow::Borrow<Event> for SharedRequestInput {
    fn borrow(&self) -> &Event {
        &self.0.template
    }
}

#[derive(Clone)]
pub struct RequestState {
    input: Arc<RequestInput>,
    pub incarnation: u64,
    pub sequence_id: Option<u32>,
    /// Prompt tokens the tail has settled. Advanced when a fragment comes
    /// back, and it is what the request has actually prefilled.
    pub prompt_cursor: usize,
    /// Prompt tokens already issued into the pipeline, settled or not.
    ///
    /// Separate from `prompt_cursor` because a prompt may have more than one
    /// fragment travelling at once: rows are cut from here on issue and the
    /// cursor catches up on settlement. Even with limit one they differ while
    /// that fragment is travelling; limit one only forbids a second fragment.
    pub prompt_issued: usize,
    pub ready: Option<ReadyRows>,
    pub after_settlement: Option<SettlementContinuation>,
    /// Fragments of this request in the pipeline right now.
    ///
    /// A decode has to be one: the next token is not known until this one has
    /// been sampled at the tail. A prompt's tokens are all known, so a long
    /// one can have several fragments in flight. This counter is
    /// neither a physical execution count nor a pending KV acknowledgement.
    /// Multi-fragment performance and edge capacity need separate validation.
    pub outstanding: u32,
    pub generated: u32,
    /// Head-approved physical work, not plan attempts or observation sends.
    /// Fixed size regardless of generation length. None until the first
    /// native result is admitted into the flight ledger. Wire completion
    /// evidence is a separate producer/consumer migration.
    pub issued_work: Option<super::super::issue_witness::IssueWitness>,
}

impl std::ops::Deref for RequestState {
    type Target = RequestInput;

    fn deref(&self) -> &Self::Target {
        &self.input
    }
}

/// Original request provenance survives resident removal. No prompt/tensor
/// payload is copied into release bookkeeping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRelease {
    pub sequence: super::super::ReleaseSequence,
    pub original: Envelope,
    pub reply: super::super::ReplySpec,
    pub dispatch: ControlDispatch,
}

/// Local effect progress is not the downstream KV acknowledgement. Pending
/// registration alone must not authorize an ACK when the effect pump yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ControlDispatchPhase {
    Queued,
    LocalApplied,
    ForwardAccepted,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ControlDispatch {
    pub load_generation: u64,
    pub session_id: String,
    pub phase: ControlDispatchPhase,
}

impl ControlDispatch {
    pub fn queued(load_generation: u64, session_id: String) -> Self {
        Self {
            load_generation,
            session_id,
            phase: ControlDispatchPhase::Queued,
        }
    }

    pub fn allows_ack(&self, load_generation: u64, session_id: &str) -> bool {
        self.load_generation == load_generation
            && self.session_id == session_id
            && self.phase == ControlDispatchPhase::ForwardAccepted
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PendingSettlement {
    pub sequence: super::super::SettlementSequence,
    pub dispatch: ControlDispatch,
}

#[derive(Clone)]
pub enum SettlementContinuation {
    Proposal { position: u32, token: i32 },
    Replay(ReadyRows),
}

#[derive(Clone, Debug)]
pub struct ReadyRows {
    pub phase: super::super::Phase,
    pub tokens: Vec<i32>,
    pub position: u32,
    pub speculative_id: u64,
}

/// Why a settlement could not be applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementRefusal {
    /// The tail settled a fragment this request never had out.
    NothingInFlight,
    /// More prompt rows came back than were issued.
    MoreRowsThanIssued,
    CursorOverflow,
}

impl SettlementRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NothingInFlight => "tail completed a request with no fragment in flight",
            Self::MoreRowsThanIssued => "tail completed more prompt rows than were issued",
            Self::CursorOverflow => "prompt cursor overflow",
        }
    }
}

impl RequestState {
    pub(crate) fn new_reserved(
        command: InferenceCommand, template: Event, reply: String,
        incarnation: u64, reservation: super::request_budget::RequestReservation,
    ) -> Self {
        let mut request = Self::new(command, template, reply, incarnation, None);
        Arc::get_mut(&mut request.input).expect("new input is unique").reservation = Some(reservation);
        request
    }

    pub(crate) fn shared_input(&self) -> SharedRequestInput {
        SharedRequestInput(Arc::clone(&self.input))
    }

    pub fn new(
        command: InferenceCommand,
        template: Event,
        reply: String,
        incarnation: u64,
        sequence_id: Option<u32>,
    ) -> Self {
        Self {
            input: Arc::new(RequestInput {
                command,
                template,
                reply,
                reservation: None,
            }),
            incarnation,
            sequence_id,
            prompt_cursor: 0,
            prompt_issued: 0,
            ready: None,
            after_settlement: None,
            outstanding: 0,
            generated: 0,
            issued_work: None,
        }
    }

    /// Test fixtures intentionally alter submitted identities/options. Make
    /// that copy-on-write explicit without granting production mutation.
    #[cfg(test)]
    pub(crate) fn input_mut_for_test(&mut self) -> &mut RequestInput {
        Arc::make_mut(&mut self.input)
    }

    #[cfg(test)]
    pub(crate) fn input_for_test(&self) -> &Arc<RequestInput> {
        &self.input
    }

    /// The original submission is the authority for the head's issued-work
    /// witness. A reply returned in a capsule cannot choose a new identity.
    pub(crate) fn issue_authority(
        &self,
    ) -> Result<super::super::issue_witness::IssueAuthority, String> {
        let envelope = &self.template.envelope;
        let outer = envelope
            .return_route
            .as_ref()
            .ok_or("issued work requires the original OUTER route")?;
        if envelope.source != Endpoint::Outer(outer.clone())
            || !matches!(envelope.target, Endpoint::Node { .. })
        {
            return Err("issued work submission source or target differs from its owner".into());
        }
        let reply: super::super::ReplySpec = serde_json::from_str(&self.reply)
            .map_err(|_| "issued work has an invalid original reply")?;
        if reply
            .ingress_agent
            .parse::<p4_protocol::Address>()
            .ok()
            .as_ref()
            != Some(&outer.ingress_agent)
            || reply.channel != outer.channel
            || reply.connection_generation != outer.connection_generation
            || reply.correlation_id != envelope.correlation_id
            || reply.deadline_unix_ms != envelope.deadline_unix_ms
        {
            return Err("issued work reply differs from the original submission".into());
        }
        Ok(super::super::issue_witness::IssueAuthority {
            head: envelope.target.clone(),
            outer: outer.clone(),
            load_generation: self.command.load_generation,
            session_id: self.command.session_id.clone(),
            request_id: self.command.request_id.clone(),
            submission_event_id: envelope.event_id.clone(),
            sequence_id: self
                .sequence_id
                .ok_or("issued work requires an admitted slot")?,
            incarnation: self.incarnation,
        })
    }

    /// Accepted logical issue, shared by the real worker and its model. Plan
    /// generation does not call this; acceptance advances these counters once.
    pub fn issue_fragment(
        &mut self,
        phase: super::super::Phase,
        rows: usize,
    ) -> Result<(), &'static str> {
        if rows == 0 || self.sequence_id.is_none() || self.after_settlement.is_some() {
            return Err("issue requires admitted rows without a pending KV settlement");
        }
        let outstanding = self
            .outstanding
            .checked_add(1)
            .ok_or("fragment count overflow")?;
        let issued = if phase == super::super::Phase::Prefill {
            let end = self
                .prompt_issued
                .checked_add(rows)
                .ok_or("prompt issue overflow")?;
            if end > self.command.tokens.len() {
                return Err("issue exceeds remaining prompt rows");
            }
            end
        } else {
            if self.outstanding != 0
                || self.prompt_cursor != self.command.tokens.len()
                || !self
                    .ready
                    .as_ref()
                    .is_some_and(|ready| ready.phase == phase && ready.tokens.len() == rows)
            {
                return Err("issue does not match ready decode or atomic rows");
            }
            self.prompt_issued
        };
        self.prompt_issued = issued;
        self.outstanding = outstanding;
        Ok(())
    }
    /// The bookkeeping one settled fragment does, in the one place that does it.
    ///
    /// The worker and the simulator each had their own copy of this. They were
    /// not the same: the worker refuses a settlement with nothing in flight and
    /// refuses a cursor past what was issued, and the simulator - which exists
    /// to catch exactly that class of drift - checked neither, so a settlement
    /// against an empty ledger was a violation it recorded and carried on from
    /// rather than a state it could not enter. Two implementations of one
    /// transition means the selector can agree while the execution semantics
    /// diverge, which is the thing being tested here.
    ///
    /// What stays out of this is the token the engine produced. The worker
    /// reads it from the tail's outcome and the simulator models it, and those
    /// are genuinely different jobs - so `generated`, `ready` and the
    /// speculative continuations are set by the caller, after this returns.
    /// What is shared is the part that must never disagree: how many fragments
    /// are out, and how far the prompt has actually settled.
    pub fn settle_fragment(
        &mut self,
        phase: super::super::Phase,
        rows: usize,
    ) -> Result<(), SettlementRefusal> {
        // Everything is checked before anything is written, so a refusal
        // leaves the request exactly as it was. The first version decremented
        // `outstanding` and then checked the row bound, so a refused
        // settlement still consumed a fragment - and the unit test written
        // beside it asserted the count it observed while its own comment said
        // the opposite. It took driving a real capsule through the worker to
        // notice, which is the argument for doing that.
        if self.outstanding == 0 {
            return Err(SettlementRefusal::NothingInFlight);
        }
        let cursor = if phase == super::super::Phase::Prefill {
            let cursor = self
                .prompt_cursor
                .checked_add(rows)
                .ok_or(SettlementRefusal::CursorOverflow)?;
            // Fragments return in the order they went out, so a cursor beyond
            // the issue point means the tail settled rows nobody sent.
            if cursor > self.prompt_issued || cursor > self.command.tokens.len() {
                return Err(SettlementRefusal::MoreRowsThanIssued);
            }
            Some(cursor)
        } else {
            None
        };

        self.outstanding -= 1;
        match cursor {
            Some(cursor) => self.prompt_cursor = cursor,
            // The row this request would have fed next is superseded by
            // whatever the caller derives from the outcome.
            None => self.ready = None,
        }
        Ok(())
    }

    /// What this request could contribute to the next batch, if anything.
    ///
    /// `fragment_limit` is how many fragments of one prompt may be in the
    /// pipeline at once; anything but a prompt is capped at one whatever it
    /// says, because the next decode row depends on this one's outcome.
    pub fn phase_within(&self, fragment_limit: u32) -> Option<super::super::Phase> {
        self.sequence_id?;
        if self.prompt_issued < self.command.tokens.len() {
            return (self.outstanding < fragment_limit.max(1))
                .then_some(super::super::Phase::Prefill);
        }
        if self.outstanding > 0 {
            return None;
        }
        self.ready.as_ref().map(|value| value.phase)
    }

    /// The single-fragment reading, for callers that only ask whether this
    /// request is runnable at all.
    pub fn phase(&self) -> Option<super::super::Phase> {
        self.phase_within(1)
    }
}

/// One request identity, qualified by the load and session it belongs to.
pub type SessionKeyScope = (u64, String, String);

/// How many admitted identities a node remembers for the alias check.
pub const SESSION_KEY_WINDOW: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueProgress {
    Prepared,
    AwaitingNative,
    Uncertain,
}

pub struct PreparedIssue {
    pub ordinal: u64,
    pub progress: IssueProgress,
    pub logical: super::super::logical::LogicalBatch,
    candidates: BTreeMap<String, RequestState>,
    verify_keys: Vec<String>,
}

impl PreparedIssue {
    #[cfg(test)]
    pub(crate) fn candidate_requests(&self) -> &BTreeMap<String, RequestState> {
        &self.candidates
    }
}

pub struct AdapterState {
    pub sessions: BTreeMap<String, PipelineSession>,
    pub requests: BTreeMap<String, RequestState>,
    pub(crate) request_budget: super::request_budget::RequestBudget,
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
    pub last_load_generation: u64,
    pub next_incarnation: u64,
    pub next_control_operation: u64,
    pub stage_owners: super::ownership::StageOwners,
    pub stage_frontiers: super::frontier::StageFrontiers,
    pub physical_receives: super::physical_receive::PhysicalReceiveLedger,
    pub next_speculative_id: u64,
    /// Hold a plan back until this many rows are ready, so a batch stops
    /// re-forming the arrival group it was born in. 0 or 1 disables the wait.
    /// See `Worker::drive_first_batches`.
    pub min_batch_rows: usize,
    /// Hold a plan back while this many batches are somewhere in the pipeline.
    /// 0 disables it. This is an experimental limit, not edge credit and not
    /// a measured universal optimum.
    pub max_open_batches: usize,
    /// Cap the rows one issued batch may carry, so a ready set becomes
    /// several batches that travel the pipeline together instead of one
    /// that occupies a single stage at a time. 0 disables it. See
    /// `Worker::drive_first_batches`.
    pub max_issue_rows: usize,
    /// Experimental selection only; does not enlarge resident/flight credits.
    pub ordinary_limits: super::super::scheduler::OrdinaryLimits,
    /// Opt-in until resource/real-model gates pass. Requires an explicit finite
    /// open window; ordinary attention only, and no multi-fragment promotion.
    pub pipeline_policy: Option<super::super::scheduler::pipeline::PipelinePolicy>,
    /// Fragments of one prompt allowed in the pipeline at once. 1 is the
    /// behaviour this adapter had before the field existed: a prompt waits a
    /// full lap between chunks even though all its tokens are known.
    ///
    /// Anything above 1 is experimental and stays off by default. Simulator
    /// checks are not downstream row/byte credit, native KV ordering or a
    /// performance proof. The executable roadmap's credit gate owns promotion.
    pub prefill_fragments: u32,
    /// Batches this first node has issued whose capsules have not all come
    /// back from the tail, as batch ordinal -> the execution ids it produced.
    ///
    /// Keyed by batch rather than by execution because the bound is checked
    /// before a batch is planned and llama.cpp decides afterwards how many
    /// physical ubatches it becomes: a set of execution ids could be three
    /// under a bound of four, admit a batch that split into four, and hold
    /// seven. One entry per issued batch makes the bound exact.
    ///
    /// Read-only compatibility view rebuilt from FlightLedger after authority
    /// registration or validated settlement. Partial physical receipts keep
    /// their logical fragment open. This count is not a byte-memory bound or
    /// a KV quiescence witness. Cleared with authority on a new load.
    pub open_batches: BTreeMap<u64, BTreeSet<u64>>,
    pub flights: super::flight::FlightLedger,
    pub prepared_issue: Option<PreparedIssue>,
    /// Stopped request identities awaiting the all-stage release return. A
    /// vacant numeric slot alone is never authority for a RELEASED command.
    pub pending_releases: BTreeMap<String, PendingRelease>,
    pub pending_settlements: BTreeMap<String, PendingSettlement>,
    /// The ordinal the next issued batch takes.
    pub next_open_batch: u64,
    /// The conversation each request identity was admitted under, so a repeat
    /// of that identity cannot silently move to another conversation.
    ///
    /// Scoped by `(load_generation, session_id, request_id)`: a request id is
    /// only unique inside one pipeline session of one load, and keying on the
    /// id alone bound a later load's request to an earlier load's conversation.
    /// Cleared on unload with the rest of the load's state, and bounded, because
    /// a node that serves for weeks would otherwise keep one entry per request
    /// it ever saw. What falls out of the window stops being checked - the
    /// alternative is durable authority for the alias, which belongs to the
    /// state store rather than to a node's memory.
    pub session_keys: BTreeMap<SessionKeyScope, Option<String>>,
    /// Admission order, so the oldest entry is the one the window drops.
    session_key_order: VecDeque<SessionKeyScope>,
    verify_fence: BTreeSet<String>,
}

impl Default for AdapterState {
    fn default() -> Self {
        Self {
            sessions: BTreeMap::new(),
            requests: BTreeMap::new(),
            request_budget: super::request_budget::RequestBudget::default(),
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
            session_key_order: VecDeque::new(),
            min_batch_rows: std::env::var("P4_STAGED_MIN_BATCH_ROWS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            max_open_batches: std::env::var("P4_STAGED_MAX_OPEN_BATCHES")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            open_batches: BTreeMap::new(),
            flights: super::flight::FlightLedger::default(),
            prepared_issue: None,
            pending_releases: BTreeMap::new(),
            pending_settlements: BTreeMap::new(),
            last_load_generation: 0,
            next_incarnation: 1,
            next_control_operation: 1,
            stage_owners: super::ownership::StageOwners::default(),
            stage_frontiers: super::frontier::StageFrontiers::default(),
            physical_receives: super::physical_receive::PhysicalReceiveLedger::default(),
            next_open_batch: 1,
            max_issue_rows: std::env::var("P4_STAGED_MAX_ISSUE_ROWS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            ordinary_limits: super::super::scheduler::OrdinaryLimits {
                prefill_members: std::env::var("P4_STAGED_PREFILL_MEMBERS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                decode_members: std::env::var("P4_STAGED_DECODE_MEMBERS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                prefill_rows: std::env::var("P4_STAGED_PREFILL_ROWS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                prefill_rows_per_request: std::env::var("P4_STAGED_PREFILL_ROWS_PER_REQUEST")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            },
            pipeline_policy: (std::env::var("P4_STAGED_PIPELINE_BATCHING").ok().as_deref() == Some("1"))
                .then(|| super::super::scheduler::pipeline::PipelinePolicy {
                    mixed_prefill_rows: std::env::var("P4_STAGED_MIXED_PREFILL_ROWS")
                        .ok().and_then(|v| v.parse().ok()).unwrap_or(128),
                }),
            prefill_fragments: std::env::var("P4_STAGED_PREFILL_FRAGMENTS")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|value| *value >= 1)
                .unwrap_or(1),
            verify_fence: BTreeSet::new(),
        }
    }
}

impl AdapterState {
    pub fn prepare_issue(
        &mut self,
        logical: super::super::logical::LogicalBatch,
    ) -> Result<(), String> {
        if self.prepared_issue.is_some() {
            return Err("a native issue remains unresolved".into());
        }
        if self.next_open_batch == 0 {
            return Err("logical issue identity is zero".into());
        }
        self.next_open_batch
            .checked_add(1)
            .ok_or_else(|| "logical issue identity exhausted".to_owned())?;
        if logical.0.is_empty() {
            return Err("logical issue has no rows".into());
        }
        let mut grouped = BTreeMap::<String, Vec<&super::super::logical::LogicalRow>>::new();
        for row in &logical.0 {
            if row.owner.load_generation != self.load_generation {
                return Err("logical issue belongs to a different load".into());
            }
            grouped
                .entry(row.owner.sequence_key.clone())
                .or_default()
                .push(row);
        }
        let mut candidates = BTreeMap::new();
        let mut verify_keys = Vec::new();
        for (key, rows) in grouped {
            let mut candidate = self
                .requests
                .get(&key)
                .ok_or_else(|| "issue request is missing".to_owned())?
                .clone();
            super::flight::validate_planned_request(&candidate, &rows)?;
            let phase = rows[0].owner.phase;
            candidate
                .issue_fragment(phase, rows.len())
                .map_err(str::to_owned)?;
            if phase == super::super::Phase::Verify {
                verify_keys.push(key.clone());
            }
            candidates.insert(key, candidate);
        }
        if !verify_keys.is_empty() && self.verify_fenced() {
            return Err("a speculative verification fence is already active".into());
        }
        self.prepared_issue = Some(PreparedIssue {
            ordinal: self.next_open_batch,
            progress: IssueProgress::Prepared,
            logical,
            candidates,
            verify_keys,
        });
        Ok(())
    }

    pub fn begin_native_issue(&mut self) -> Result<(), String> {
        let prepared = self
            .prepared_issue
            .as_mut()
            .ok_or("native issue has no prepared plan")?;
        if prepared.progress != IssueProgress::Prepared {
            return Err("native issue was already attempted".into());
        }
        prepared.progress = IssueProgress::AwaitingNative;
        Ok(())
    }

    // State-machine tests can abandon a candidate before native starts. The
    // production drive enters begin_native_issue immediately after successful
    // prepare_issue, without yielding; this is not an operational Cancel.
    #[cfg(test)]
    pub fn cancel_prepared_issue(&mut self) -> Result<(), String> {
        let prepared = self
            .prepared_issue
            .as_ref()
            .ok_or("there is no prepared issue")?;
        if prepared.progress != IssueProgress::Prepared {
            return Err("an attempted native issue requires reconciliation".into());
        }
        self.prepared_issue = None;
        Ok(())
    }

    pub fn mark_issue_uncertain(&mut self) {
        if let Some(prepared) = self.prepared_issue.as_mut()
            && prepared.progress == IssueProgress::AwaitingNative
        {
            prepared.progress = IssueProgress::Uncertain;
        }
    }

    pub fn accept_prepared_issue(
        &mut self,
        set: &super::super::capsule::CapsuleSet,
    ) -> Result<(), String> {
        let prepared = self
            .prepared_issue
            .as_ref()
            .ok_or_else(|| "physical result has no prepared issue".to_owned())?;
        if prepared.progress != IssueProgress::AwaitingNative {
            return Err("physical result has no pending native attempt".into());
        }
        super::flight::validate_split(&prepared.logical, set)?;
        if prepared.ordinal != self.next_open_batch {
            return Err("prepared issue identity changed".into());
        }
        // All fallible witness work precedes the flight commit. Group only
        // this approved result's rows; neither prompt/tensors nor previous
        // execution history are copied into these small candidates.
        use super::super::issue_witness::{IssueWitness, IssuedExecution, IssuedRow, IssuedWork};
        let mut by_request = BTreeMap::<String, BTreeMap<u64, Vec<IssuedRow>>>::new();
        for capsule in &set.0 {
            for owner in &capsule.owners {
                by_request
                    .entry(owner.sequence_key.clone())
                    .or_default()
                    .entry(capsule.execution_id)
                    .or_default()
                    .push(IssuedRow {
                        phase: owner.phase,
                        position: owner.position,
                    });
            }
        }
        if by_request.len() != prepared.candidates.len() {
            return Err("issued work does not cover the prepared request set".into());
        }
        let mut witnesses = BTreeMap::new();
        for (key, request) in &prepared.candidates {
            let authority = request.issue_authority()?;
            let previous = match request.issued_work {
                Some(witness) => witness,
                None => IssueWitness::new(&authority).map_err(str::to_owned)?,
            };
            let executions = by_request
                .remove(key)
                .ok_or("issued work is missing a prepared request")?
                .into_iter()
                .map(|(execution_id, rows)| IssuedExecution { execution_id, rows })
                .collect();
            let work = IssuedWork {
                logical_ordinal: prepared.ordinal,
                executions,
            };
            let witness = previous
                .advanced(&authority, &work)
                .map_err(str::to_owned)?;
            witnesses.insert(key.clone(), witness);
        }
        self.register_issued_batch(set)?;
        let prepared = self
            .prepared_issue
            .take()
            .expect("prepared issue was validated");
        for (key, mut request) in prepared.candidates {
            request.issued_work = Some(
                witnesses
                    .remove(&key)
                    .expect("all witness candidates validated"),
            );
            self.requests.insert(key, request);
        }
        if !prepared.verify_keys.is_empty() {
            self.begin_verify_fence(&prepared.verify_keys)
                .expect("issue fence validated before native execution");
        }
        Ok(())
    }

    pub fn register_issued_batch(
        &mut self,
        set: &super::super::capsule::CapsuleSet,
    ) -> Result<u64, String> {
        let ordinal = self.next_open_batch;
        let next = ordinal
            .checked_add(1)
            .ok_or_else(|| "logical issue identity exhausted".to_owned())?;
        self.flights.register(ordinal, self.load_generation, set)?;
        self.next_open_batch = next;
        self.open_batches = self.flights.open_batches();
        Ok(ordinal)
    }

    pub fn commit_flight_return(&mut self, plan: super::flight::ReturnPlan) {
        self.flights.commit_return(plan);
        self.open_batches = self.flights.open_batches();
    }

    pub fn clear_flights(&mut self) {
        self.flights = super::flight::FlightLedger::default();
        self.open_batches.clear();
        self.prepared_issue = None;
        self.pending_releases.clear();
        self.pending_settlements.clear();
        self.stage_owners = super::ownership::StageOwners::default();
        self.stage_frontiers = super::frontier::StageFrontiers::default();
        self.physical_receives = super::physical_receive::PhysicalReceiveLedger::default();
    }
    /// Records the conversation this identity was admitted under, dropping the
    /// oldest once the window is full.
    pub fn remember_session_key(&mut self, scope: SessionKeyScope, key: Option<String>) {
        if self.session_keys.insert(scope.clone(), key).is_none() {
            self.session_key_order.push_back(scope);
        }
        while self.session_key_order.len() > SESSION_KEY_WINDOW {
            if let Some(oldest) = self.session_key_order.pop_front() {
                self.session_keys.remove(&oldest);
            }
        }
    }

    /// Forgets every admitted identity. Called on unload: the load generation
    /// they were scoped to is over.
    pub fn forget_session_keys(&mut self) {
        self.session_keys.clear();
        self.session_key_order.clear();
    }

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
        self.requests
            .values()
            .any(|request| request.outstanding > 0)
    }

    /// Rows a plan could carry right now. Decode contributes one row per ready
    /// sequence; a pending prompt is counted as one because the scheduler
    /// gives every prompt a row before water-filling the remainder.
    pub fn ready_row_count(&self) -> usize {
        self.requests
            .values()
            .filter(|request| request.phase_within(self.prefill_fragments).is_some())
            .count()
    }

    /// Rows the ready set actually holds: every remaining prompt token and
    /// every ready decode, verify or replay token. `ready_row_count` counts
    /// requests, which is what the coalescing gate wants; this is what a
    /// report wants when it asks whether the scheduler left rows behind, and
    /// the two were confused once - a report subtracted planned rows from
    /// the request count and read the negative result as a clean sweep.
    pub fn available_row_count(&self) -> usize {
        self.requests
            .values()
            .filter_map(|request| {
                request
                    .phase_within(self.prefill_fragments)
                    .map(|phase| match phase {
                        super::super::Phase::Prefill => {
                            request.command.tokens.len() - request.prompt_issued
                        }
                        _ => request.ready.as_ref().map_or(0, |ready| ready.tokens.len()),
                    })
            })
            .sum()
    }

    pub fn first_session_with_work(&self) -> Option<String> {
        let limit = self.prefill_fragments;
        self.requests.values().find_map(|request| {
            let session = self.sessions.get(&request.command.session_id)?;
            (session.command.role() == NodeRole::First && request.phase_within(limit).is_some())
                .then(|| request.command.session_id.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AdapterState, SESSION_KEY_WINDOW, SessionKeyScope};

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

    fn scope(load: u64, session: &str, request: &str) -> SessionKeyScope {
        (load, session.to_owned(), request.to_owned())
    }

    #[test]
    fn one_request_identity_keeps_the_conversation_it_was_admitted_under() {
        let mut state = AdapterState::default();
        let first = scope(1, "pipeline", "req-001");
        state.remember_session_key(first.clone(), Some("sk1:owner/conv-a".to_owned()));
        assert_eq!(
            state.session_keys.get(&first).and_then(Option::as_deref),
            Some("sk1:owner/conv-a")
        );
    }

    #[test]
    fn the_same_request_id_in_another_session_is_a_different_identity() {
        // Keyed on the id alone, these two collided, and the second turn of
        // one conversation could be refused because an unrelated session had
        // used the same request id.
        let mut state = AdapterState::default();
        state.remember_session_key(
            scope(1, "pipeline-a", "req-001"),
            Some("sk1:owner/a".into()),
        );
        state.remember_session_key(
            scope(1, "pipeline-b", "req-001"),
            Some("sk1:owner/b".into()),
        );
        assert_eq!(state.session_keys.len(), 2);
    }

    #[test]
    fn a_later_load_does_not_inherit_an_earlier_load_s_conversations() {
        let mut state = AdapterState::default();
        state.remember_session_key(scope(1, "pipeline", "req-001"), Some("sk1:owner/a".into()));
        assert!(
            state
                .session_keys
                .get(&scope(2, "pipeline", "req-001"))
                .is_none()
        );
    }

    #[test]
    fn unload_forgets_the_generation_it_was_scoped_to() {
        let mut state = AdapterState::default();
        state.remember_session_key(scope(1, "pipeline", "req-001"), Some("sk1:owner/a".into()));
        state.forget_session_keys();
        assert!(state.session_keys.is_empty());
    }

    #[test]
    fn the_window_bounds_what_a_long_lived_node_remembers() {
        // A node that serves for weeks would otherwise keep one entry per
        // request it ever saw.
        let mut state = AdapterState::default();
        for index in 0..SESSION_KEY_WINDOW + 8 {
            state.remember_session_key(
                scope(1, "pipeline", &format!("req-{index}")),
                Some("sk1:owner/conv".to_owned()),
            );
        }
        assert_eq!(state.session_keys.len(), SESSION_KEY_WINDOW);
        assert!(
            state
                .session_keys
                .get(&scope(1, "pipeline", "req-0"))
                .is_none()
        );
        assert!(
            state
                .session_keys
                .get(&scope(
                    1,
                    "pipeline",
                    &format!("req-{}", SESSION_KEY_WINDOW + 7)
                ))
                .is_some()
        );
    }

    #[test]
    fn re_admitting_one_identity_does_not_age_the_window_twice() {
        let mut state = AdapterState::default();
        let only = scope(1, "pipeline", "req-001");
        state.remember_session_key(only.clone(), Some("sk1:owner/a".into()));
        state.remember_session_key(only.clone(), Some("sk1:owner/a".into()));
        assert_eq!(state.session_keys.len(), 1);
        state.forget_session_keys();
        state.remember_session_key(only, Some("sk1:owner/a".into()));
        assert_eq!(state.session_keys.len(), 1);
    }
}

pub fn request_key(session_id: &str, request_id: &str) -> String {
    format!("{session_id}\u{0}{request_id}")
}
