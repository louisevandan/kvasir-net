//! Current-run, owner-scoped evidence. Submitted envelopes establish authority;
//! terminal OUTPUT supplies the independently committed issued-work total.
//! Each issue is hashed once, when its per-request prefix becomes contiguous.
//! This retains run artifacts; it is not a bounded-RSS or cross-host clock proof.
use super::{RequestArtifact, StageSpanArtifact};
use p4_llamacpp_staged_adapter::v2::{
    ApprovedOutputPayload, BatchObservation, InferenceCommand, IssueAuthority, IssueWitness,
    IssuedExecution, IssuedWork, IssuedWorkProof, StageSpan,
};
use p4_protocol::event::{Endpoint, Event, EventClass, OuterEndpoint};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
#[path = "inference_evidence.rs"]
mod tests;

type Owner = (String, u32, u64);

#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
pub struct SubmittedAuthority {
    #[serde(serialize_with = "serialize_head")]
    head: Endpoint,
    #[serde(serialize_with = "serialize_outer")]
    outer: OuterEndpoint,
    load: u64,
    session: String,
    request: String,
    submission: String,
    correlation: String,
    deadline: Option<u64>,
}

fn serialize_head<S: serde::Serializer>(
    endpoint: &Endpoint,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    let Endpoint::Node {
        agent,
        node,
        generation,
    } = endpoint
    else {
        return Err(serde::ser::Error::custom("submitted head is not a node"));
    };
    serde_json::json!({"agent": agent.to_string(), "node": node, "generation": generation})
        .serialize(serializer)
}

fn serialize_outer<S: serde::Serializer>(
    outer: &OuterEndpoint,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    serde_json::json!({"ingress_agent": outer.ingress_agent.to_string(), "channel": outer.channel,
        "connection_generation": outer.connection_generation})
    .serialize(serializer)
}

impl SubmittedAuthority {
    pub(super) fn from_event(event: &Event, command: &InferenceCommand) -> Result<Self, String> {
        let Endpoint::Outer(outer) = &event.envelope.source else {
            return Err("submitted evidence authority is not an OUTER".into());
        };
        if event.envelope.return_route.as_ref() != Some(outer)
            || !matches!(event.envelope.target, Endpoint::Node { .. })
            || event.envelope.class != EventClass::Data
            || event.envelope.correlation_id != command.request_id
            || event.envelope.event_id.is_empty()
            || event.envelope.event_id.contains('\0')
        {
            return Err("submitted evidence authority differs from the actual request".into());
        }
        Ok(Self {
            head: event.envelope.target.clone(),
            outer: outer.clone(),
            load: command.load_generation,
            session: command.session_id.clone(),
            request: command.request_id.clone(),
            submission: event.envelope.event_id.clone(),
            correlation: event.envelope.correlation_id.clone(),
            deadline: event.envelope.deadline_unix_ms,
        })
    }

    fn issue(&self, slot: u32, incarnation: u64) -> IssueAuthority {
        IssueAuthority {
            head: self.head.clone(),
            outer: self.outer.clone(),
            load_generation: self.load,
            session_id: self.session.clone(),
            request_id: self.request.clone(),
            submission_event_id: self.submission.clone(),
            sequence_id: slot,
            incarnation,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Progress {
    owner: Option<(u32, u64)>,
    witness: Option<IssueWitness>,
    terminal: Option<IssuedWorkProof>,
    first_position: Option<usize>,
    rows: [usize; 4],
    max_index: u64,
    approved: bool,
}

impl Progress {
    fn complete(&self) -> bool {
        self.approved
            && self
                .terminal
                .is_some_and(|proof| self.witness.is_some_and(|witness| witness.proof() == proof))
            && self.first_position == Some(self.rows[0])
            && self.rows[0] != 0
    }
}

#[derive(Debug)]
struct RequestEvidence {
    authority: SubmittedAuthority,
    progress: Progress,
    pending: BTreeMap<u64, IssuedWork>,
}

#[derive(Clone, Debug, Default)]
struct ExecutionEvidence {
    rows: Option<usize>,
    expected: Option<BTreeSet<Owner>>,
    stages: BTreeMap<usize, BTreeSet<Owner>>,
    groups: BTreeMap<usize, Vec<u64>>,
}

#[derive(Clone, Copy, Debug)]
struct SpanRows {
    unknown: usize,
    known: usize,
    total: usize,
}

impl SpanRows {
    fn validate(&self) -> Result<(), String> {
        if self.known > self.total || (self.unknown == 0 && self.known != self.total) {
            return Err("stage span physical rows differ from head physical totals".into());
        }
        Ok(())
    }
}

impl ExecutionEvidence {
    fn missing(&self, stage_count: usize) -> usize {
        match &self.expected {
            Some(owners) if !owners.is_empty() => stage_count - self.stages.len(),
            // Even an empty owner projection that actually arrived claims
            // physical work and width. Its head dimensions must be joined;
            // this does not demand foreign-only events we never received.
            None => usize::from(!self.stages.is_empty()),
            _ => 0,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(expected) = &self.expected {
            if self.stages.values().any(|actual| actual != expected) {
                return Err(
                    "stage span owned membership differs from head-issued execution".into(),
                );
            }
        }
        Ok(())
    }
}

struct RequestUpdate {
    progress: Progress,
    added: Option<(u64, IssuedWork)>,
    consumed: Vec<u64>,
}

/// Invalid inputs return Err before any state change. Missing is retryable
/// until the existing overall run deadline, not an approval or a reset timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EvidenceStatus {
    Missing {
        requests: usize,
        stage_executions: usize,
    },
    Complete,
}

#[derive(Debug)]
pub(super) struct EvidenceLedger {
    stage_count: usize,
    requests: BTreeMap<String, RequestEvidence>,
    executions: BTreeMap<u64, ExecutionEvidence>,
    observations: BTreeMap<String, BatchObservation>,
    spans: BTreeMap<(usize, Vec<u64>), StageSpanArtifact>,
    span_rows: BTreeMap<(usize, Vec<u64>), SpanRows>,
    complete_requests: usize,
    missing_coverage: usize,
}

impl EvidenceLedger {
    pub(super) fn new(stage_count: usize) -> Self {
        assert!(stage_count > 0);
        Self {
            stage_count,
            requests: BTreeMap::new(),
            executions: BTreeMap::new(),
            observations: BTreeMap::new(),
            spans: BTreeMap::new(),
            span_rows: BTreeMap::new(),
            complete_requests: 0,
            missing_coverage: 0,
        }
    }

    pub(super) fn register(&mut self, authority: SubmittedAuthority) {
        // send_wave checks both request maps before either infallible install.
        let prior = self.requests.insert(
            authority.request.clone(),
            RequestEvidence {
                authority,
                progress: Progress::default(),
                pending: BTreeMap::new(),
            },
        );
        assert!(prior.is_none(), "submitted authority was prevalidated");
    }

    fn carrier(&self, event: &Event) -> Result<(), String> {
        let request = self
            .requests
            .get(&event.envelope.correlation_id)
            .ok_or("evidence carrier is not an actually submitted request")?;
        let sent = &request.authority;
        if event.envelope.target != Endpoint::Outer(sent.outer.clone())
            || event.envelope.return_route.as_ref() != Some(&sent.outer)
            || event.envelope.correlation_id != sent.correlation
            || event.envelope.deadline_unix_ms != sent.deadline
        {
            return Err("evidence carrier differs from the submitted reply route".into());
        }
        Ok(())
    }

    fn bind(
        progress: &mut Progress,
        request: &RequestEvidence,
        slot: u32,
        incarnation: u64,
    ) -> Result<(), String> {
        if incarnation == 0
            || progress
                .owner
                .is_some_and(|owner| owner != (slot, incarnation))
        {
            return Err("evidence changed the submitted request's slot or incarnation".into());
        }
        if progress.owner.is_none() {
            progress.owner = Some((slot, incarnation));
            progress.witness = Some(
                IssueWitness::new(&request.authority.issue(slot, incarnation))
                    .map_err(str::to_owned)?,
            );
        }
        Ok(())
    }

    fn advance(request: &RequestEvidence, update: &mut RequestUpdate) -> Result<(), String> {
        let Some((slot, incarnation)) = update.progress.owner else {
            return Ok(());
        };
        let authority = request.authority.issue(slot, incarnation);
        loop {
            let witness = update.progress.witness.expect("bound witness");
            let Some(next) = witness.issue_count().checked_add(1) else {
                break;
            };
            let work = update
                .added
                .as_ref()
                .filter(|(index, _)| *index == next)
                .map(|(_, work)| work)
                .or_else(|| request.pending.get(&next));
            let Some(work) = work else {
                break;
            };
            let advanced = witness.advanced(&authority, work).map_err(str::to_owned)?;
            for execution in &work.executions {
                for row in &execution.rows {
                    let phase = match row.phase {
                        p4_llamacpp_staged_adapter::v2::Phase::Prefill => 0,
                        p4_llamacpp_staged_adapter::v2::Phase::Decode => 1,
                        p4_llamacpp_staged_adapter::v2::Phase::Verify => 2,
                        p4_llamacpp_staged_adapter::v2::Phase::Replay => 3,
                    };
                    update.progress.rows[phase] = update.progress.rows[phase]
                        .checked_add(1)
                        .ok_or("request row count overflow")?;
                }
            }
            update.progress.witness = Some(advanced);
            update.consumed.push(next);
        }
        if update
            .progress
            .first_position
            .is_some_and(|first| update.progress.rows[0] > first)
        {
            return Err(
                "first output does not follow request prefill evidence: too many prefill rows"
                    .into(),
            );
        }
        if let Some(proof) = update.progress.terminal {
            let witness = update.progress.witness.expect("bound witness");
            if proof.authority_digest != witness.authority_digest()
                || update.progress.max_index > proof.issue_count
                || witness.issue_count() > proof.issue_count
            {
                return Err(
                    "terminal issued-work authority or count differs from submitted evidence"
                        .into(),
                );
            }
            if witness.issue_count() == proof.issue_count {
                if witness.proof() != proof {
                    return Err(
                        "terminal issued-work digest or ordinal differs from observed issue chain"
                            .into(),
                    );
                }
                if update.progress.rows[0] == 0
                    || update.progress.first_position != Some(update.progress.rows[0])
                {
                    return Err("first output does not follow request prefill evidence".into());
                }
            }
        }
        Ok(())
    }

    fn commit_request(&mut self, id: &str, update: RequestUpdate) {
        let request = self
            .requests
            .get_mut(id)
            .expect("whole candidate request exists");
        self.complete_requests -= usize::from(request.progress.complete());
        request.progress = update.progress;
        if let Some((index, work)) = update.added {
            request.pending.insert(index, work);
        }
        for index in update.consumed {
            request.pending.remove(&index);
        }
        self.complete_requests += usize::from(request.progress.complete());
    }

    fn commit_execution(&mut self, id: u64, candidate: ExecutionEvidence) {
        if let Some(previous) = self.executions.get(&id) {
            self.missing_coverage -= previous.missing(self.stage_count);
        }
        self.missing_coverage += candidate.missing(self.stage_count);
        self.executions.insert(id, candidate);
    }

    pub(super) fn output(
        &mut self,
        event: &Event,
        output: &ApprovedOutputPayload,
    ) -> Result<(), String> {
        self.carrier(event)?;
        output.validate().map_err(str::to_owned)?;
        let id = &output.outcome.request_id;
        let request = self
            .requests
            .get(id)
            .ok_or("output evidence references an unknown request")?;
        if output.submission_event_id != request.authority.submission {
            return Err("output evidence differs from the actual submitted attempt".into());
        }
        let mut update = RequestUpdate {
            progress: request.progress,
            added: None,
            consumed: Vec::new(),
        };
        Self::bind(
            &mut update.progress,
            request,
            output.outcome.sequence_id,
            output.incarnation,
        )?;
        update.progress.approved = true;
        update
            .progress
            .first_position
            .get_or_insert(output.outcome.position as usize);
        if let Some(proof) = output.issued_work {
            update.progress.terminal = Some(proof);
        }
        Self::advance(request, &mut update)?;
        self.commit_request(id, update);
        Ok(())
    }

    pub(super) fn observation(
        &mut self,
        event: &Event,
        observation: BatchObservation,
    ) -> Result<(), String> {
        self.carrier(event)?;
        if let Some(previous) = self.observations.get(&observation.observation_id) {
            return if previous == &observation {
                Ok(())
            } else {
                Err("duplicate observation identity changed its payload".into())
            };
        }
        let mut groups = BTreeMap::<String, (u64, u32, u64, Vec<IssuedExecution>)>::new();
        let mut executions = BTreeMap::new();
        let mut span_rows = BTreeMap::new();
        for physical in &observation.physical_batches {
            let mut candidate = self
                .executions
                .get(&physical.execution_id)
                .cloned()
                .unwrap_or_default();
            if candidate.expected.is_some() {
                return Err("physical execution appears in more than one observation".into());
            }
            candidate.rows = Some(physical.rows);
            for (node, group) in &candidate.groups {
                let key = (*node, group.clone());
                let rows = span_rows.entry(key.clone()).or_insert(self.span_rows[&key]);
                rows.unknown -= 1;
                rows.known = rows
                    .known
                    .checked_add(physical.rows)
                    .ok_or("span row count overflow")?;
                rows.validate()?;
            }
            let mut owners = BTreeSet::new();
            for owner in &physical.owned_requests {
                let request = self
                    .requests
                    .get(&owner.request_id)
                    .ok_or("batch observation references an unknown request")?;
                if owner.submission_event_id != request.authority.submission {
                    return Err(
                        "batch observation submission differs from the actual sent attempt".into(),
                    );
                }
                owners.insert((
                    owner.request_id.clone(),
                    owner.sequence_id,
                    owner.incarnation,
                ));
                let group = groups.entry(owner.request_id.clone()).or_insert_with(|| {
                    (
                        owner.request_issue_index,
                        owner.sequence_id,
                        owner.incarnation,
                        Vec::new(),
                    )
                });
                if (group.0, group.1, group.2)
                    != (
                        owner.request_issue_index,
                        owner.sequence_id,
                        owner.incarnation,
                    )
                {
                    return Err("one logical observation changed request issue identity".into());
                }
                group.3.push(IssuedExecution {
                    execution_id: physical.execution_id,
                    rows: owner.rows.clone(),
                });
            }
            candidate.expected = Some(owners);
            candidate.validate()?;
            executions.insert(physical.execution_id, candidate);
        }
        let mut updates = BTreeMap::new();
        for (id, (index, slot, incarnation, executions)) in groups {
            let request = &self.requests[&id];
            let count = request
                .progress
                .witness
                .map_or(0, |witness| witness.issue_count());
            if index == 0 || index <= count || request.pending.contains_key(&index) {
                return Err("request issue index is repeated or invalid".into());
            }
            let mut update = RequestUpdate {
                progress: request.progress,
                added: Some((
                    index,
                    IssuedWork {
                        logical_ordinal: observation.logical_ordinal,
                        executions,
                    },
                )),
                consumed: Vec::new(),
            };
            update.progress.max_index = update.progress.max_index.max(index);
            Self::bind(&mut update.progress, request, slot, incarnation)?;
            Self::advance(request, &mut update)?;
            updates.insert(id, update);
        }
        for (id, update) in updates {
            self.commit_request(&id, update);
        }
        for (id, candidate) in executions {
            self.commit_execution(id, candidate);
        }
        self.span_rows.extend(span_rows);
        self.observations
            .insert(observation.observation_id.clone(), observation);
        Ok(())
    }

    pub(super) fn span(
        &mut self,
        event: &Event,
        node: usize,
        mut span: StageSpan,
    ) -> Result<(), String> {
        self.carrier(event)?;
        if node >= self.stage_count {
            return Err("span stage is outside the configured pipeline".into());
        }
        span.execution_ids.sort_unstable();
        span.executions
            .sort_by_key(|execution| execution.execution_id);
        for execution in &mut span.executions {
            execution.owned_requests.sort_by(|a, b| {
                (&a.request_id, a.sequence_id, a.incarnation).cmp(&(
                    &b.request_id,
                    b.sequence_id,
                    b.incarnation,
                ))
            });
        }
        let key = (node, span.execution_ids.clone());
        if let Some(previous) = self.spans.get(&key) {
            return if previous.span == span {
                Ok(())
            } else {
                Err("duplicate stage span identity changed its payload".into())
            };
        }
        let mut executions = BTreeMap::new();
        let mut updates = BTreeMap::<String, RequestUpdate>::new();
        let mut span_rows = SpanRows {
            unknown: 0,
            known: 0,
            total: span.rows,
        };
        for execution in &span.executions {
            let mut candidate = self
                .executions
                .get(&execution.execution_id)
                .cloned()
                .unwrap_or_default();
            if candidate.stages.contains_key(&node) {
                return Err("stage execution appears in overlapping span groups".into());
            }
            if let Some(rows) = candidate.rows {
                span_rows.known = span_rows
                    .known
                    .checked_add(rows)
                    .ok_or("span row count overflow")?;
            } else {
                span_rows.unknown += 1;
            }
            candidate.groups.insert(node, key.1.clone());
            let mut owners = BTreeSet::new();
            for owner in &execution.owned_requests {
                let request = self
                    .requests
                    .get(&owner.request_id)
                    .ok_or("stage span references an unknown request")?;
                let update =
                    updates
                        .entry(owner.request_id.clone())
                        .or_insert_with(|| RequestUpdate {
                            progress: request.progress,
                            added: None,
                            consumed: Vec::new(),
                        });
                Self::bind(
                    &mut update.progress,
                    request,
                    owner.sequence_id,
                    owner.incarnation,
                )?;
                owners.insert((
                    owner.request_id.clone(),
                    owner.sequence_id,
                    owner.incarnation,
                ));
            }
            candidate.stages.insert(node, owners);
            candidate.validate()?;
            executions.insert(execution.execution_id, candidate);
        }
        span_rows.validate()?;
        for (id, update) in updates {
            self.commit_request(&id, update);
        }
        for (id, candidate) in executions {
            self.commit_execution(id, candidate);
        }
        self.span_rows.insert(key.clone(), span_rows);
        self.spans.insert(key, StageSpanArtifact { node, span });
        Ok(())
    }

    pub(super) fn status(&self) -> EvidenceStatus {
        let requests = self.requests.len() - self.complete_requests;
        if requests == 0 && self.missing_coverage == 0 {
            EvidenceStatus::Complete
        } else {
            EvidenceStatus::Missing {
                requests,
                stage_executions: self.missing_coverage,
            }
        }
    }

    pub(super) fn apply_counts(&self, requests: &mut BTreeMap<String, RequestArtifact>) {
        assert_eq!(self.status(), EvidenceStatus::Complete);
        for (id, request) in requests {
            [
                request.prefill_rows,
                request.decode_rows,
                request.verify_rows,
                request.replay_rows,
            ] = self.requests[id].progress.rows;
        }
    }

    pub(super) fn into_artifacts(self) -> (Vec<BatchObservation>, Vec<StageSpanArtifact>) {
        (
            self.observations.into_values().collect(),
            self.spans.into_values().collect(),
        )
    }
}
