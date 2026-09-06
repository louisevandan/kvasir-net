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
    /// Prompt tokens the tail has settled. Advanced when a fragment comes
    /// back, and it is what the request has actually prefilled.
    pub prompt_cursor: usize,
    /// Prompt tokens already issued into the pipeline, settled or not.
    ///
    /// Separate from `prompt_cursor` because a prompt may have more than one
    /// fragment travelling at once: rows are cut from here on issue and the
    /// cursor catches up on settlement. With a fragment limit of one the two
    /// never diverge, which is what the pipeline did before this existed.
    pub prompt_issued: usize,
    pub ready: Option<ReadyRows>,
    pub after_settlement: Option<SettlementContinuation>,
    /// Fragments of this request in the pipeline right now.
    ///
    /// A decode has to be one: the next token is not known until this one has
    /// been sampled at the tail. A prompt does not - its tokens are all known
    /// - so a long one can have several fragments in flight and stop waiting
    /// a whole lap between chunks. Measured on a 2B run, 92 of 192 requests
    /// took two or more laps to prefill and some took eleven, while the first
    /// node stood idle for 33% of the wall clock.
    pub outstanding: u32,
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
    /// Hold a plan back while this many batches are somewhere in the pipeline.
    /// 0 disables it. See `Worker::drive_first_batches` for why this, and not
    /// a row threshold, is the lever the stage spans point at.
    pub max_open_batches: usize,
    /// Cap the rows one issued batch may carry, so a ready set becomes
    /// several batches that travel the pipeline together instead of one
    /// that occupies a single stage at a time. 0 disables it. See
    /// `Worker::drive_first_batches`.
    pub max_issue_rows: usize,
    /// Fragments of one prompt allowed in the pipeline at once. 1 is the
    /// behaviour this adapter had before the field existed: a prompt waits a
    /// full lap between chunks even though all its tokens are known.
    ///
    /// Anything above 1 is experimental and stays off by default. Not because
    /// it breaks an invariant - `simulator_tests` runs limits 1, 2 and 4 and
    /// all three hold - but because nothing downstream bounds the rows a
    /// single prompt may put on an edge. That bound is the fragment ledger the
    /// plan calls P4.5, and until it exists a raised limit lets one long
    /// prompt claim edge capacity that no component accounts for. The measured
    /// effect of raising it on the 4-node harness was -0.5%, which is to say
    /// none: prefill latency there is admission queueing, not lap pacing - a
    /// 30-row prompt and a 1232-row one both took 68-79s to first token.
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
    /// An entry is added when this node's own stage returns the physical
    /// result, and an execution id is removed by `Worker::tail` when the
    /// terminal capsule carrying it arrives; the batch goes when its last
    /// capsule does. Bounded by `max_open_batches` when the gate is on and by
    /// the active set when it is off - nothing is issued without a ready row.
    /// Cleared with the rest of the state on a new load.
    pub open_batches: BTreeMap<u64, BTreeSet<u64>>,
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
            next_open_batch: 1,
            max_issue_rows: std::env::var("P4_STAGED_MAX_ISSUE_ROWS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
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
        self.requests.values().any(|request| request.outstanding > 0)
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
                request.phase_within(self.prefill_fragments).map(|phase| match phase {
                    super::super::Phase::Prefill => {
                        request.command.tokens.len() - request.prompt_issued
                    }
                    _ => request.ready.as_ref().map_or(0, |ready| ready.tokens.len()),
                })
            })
            .sum()
    }

    /// Records a batch's capsules as outstanding and returns nothing: the
    /// gate reads `open_batches.len()`, which is now one per issued batch.
    pub fn open_batch(&mut self, executions: impl IntoIterator<Item = u64>) {
        let ordinal = self.next_open_batch;
        self.next_open_batch = self.next_open_batch.wrapping_add(1);
        self.open_batches.insert(ordinal, executions.into_iter().collect());
    }

    /// Retires one capsule, and its batch once the batch has no capsules left.
    pub fn close_execution(&mut self, execution_id: u64) {
        let emptied: Vec<u64> = self
            .open_batches
            .iter_mut()
            .filter_map(|(ordinal, executions)| {
                executions.remove(&execution_id).then_some(*ordinal)
            })
            .collect();
        for ordinal in emptied {
            if self.open_batches.get(&ordinal).is_some_and(BTreeSet::is_empty) {
                self.open_batches.remove(&ordinal);
            }
        }
    }

    pub fn first_session_with_work(&self) -> Option<String> {
        let limit = self.prefill_fragments;
        self.requests.values().find_map(|request| {
            let session = self.sessions.get(&request.command.session_id)?;
            (session.command.role == NodeRole::First
                && request.phase_within(limit).is_some())
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
        state.remember_session_key(scope(1, "pipeline-a", "req-001"), Some("sk1:owner/a".into()));
        state.remember_session_key(scope(1, "pipeline-b", "req-001"), Some("sk1:owner/b".into()));
        assert_eq!(state.session_keys.len(), 2);
    }

    #[test]
    fn a_later_load_does_not_inherit_an_earlier_load_s_conversations() {
        let mut state = AdapterState::default();
        state.remember_session_key(scope(1, "pipeline", "req-001"), Some("sk1:owner/a".into()));
        assert!(state.session_keys.get(&scope(2, "pipeline", "req-001")).is_none());
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
        assert!(state.session_keys.get(&scope(1, "pipeline", "req-0")).is_none());
        assert!(
            state
                .session_keys
                .get(&scope(1, "pipeline", &format!("req-{}", SESSION_KEY_WINDOW + 7)))
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
