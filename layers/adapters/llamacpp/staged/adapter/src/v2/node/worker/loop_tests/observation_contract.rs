//! Independent actual-run oracle: successful native result bytes are captured
//! only when the real head acceptance hook fires. Received telemetry cannot
//! create its own expected work or determine when the pump has completed.
use super::*;
use crate::v2::{ApprovedOutputPayload, BatchObservation, IssuedWorkProof, StageSpan};
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct AcceptedIssue {
    ordinal: u64,
    physical: CapsuleSet,
    proofs: BTreeMap<String, IssuedWorkProof>,
    scheduling: crate::v2::commands::SchedulingSnapshot,
}

pub(super) type Accepted = Arc<Mutex<Vec<AcceptedIssue>>>;

pub(super) fn accepted() -> Accepted {
    Arc::new(Mutex::new(Vec::new()))
}

pub(super) fn observer(
    accepted: Accepted,
    native: Arc<Mutex<NativeTrace>>,
    extra: Option<IssueObserver>,
) -> IssueObserver {
    let before = Mutex::new(None);
    Arc::new(move |point, state| {
        if point == "before_native_issue" {
            // Independent oracle from request state, not producer telemetry or
            // phase_within(). Order: pending slot, prompt dependency, decode
            // dependency, then known input. Native acceptance has not run yet.
            let mut counts = [0usize; 6];
            for r in state.requests.values() {
                let class = if r.sequence_id.is_none() {
                    0
                } else if r.prompt_issued < r.command.tokens.len() {
                    if r.outstanding < state.prefill_fragments.max(1) {
                        3
                    } else {
                        1
                    }
                } else if r.outstanding > 0 {
                    1
                } else {
                    match r.ready.as_ref().map(|r| r.phase) {
                        Some(Phase::Prefill) => 3,
                        Some(Phase::Decode) => 4,
                        Some(Phase::Verify | Phase::Replay) => 5,
                        None => 2,
                    }
                };
                counts[class] += 1;
            }
            // Reconstruct the normalized selection from independently counted
            // request phases; do not call the production pipeline selector.
            let pipeline = state.pipeline_policy.filter(|_| !state.equal_sequence_ubatch && counts[5] == 0)
                .map(|policy| {
                    let vacancies = state.max_open_batches - state.open_batches.len();
                    let bound = |old: usize, selected: usize| if old == 0 { selected } else { old.min(selected) };
                    let mut effective = state.ordinary_limits;
                    effective.prefill_members = bound(effective.prefill_members, counts[3].div_ceil(vacancies).max(1));
                    effective.decode_members = bound(effective.decode_members, counts[4].div_ceil(vacancies).max(1));
                    let decoding_active = state.requests.values().any(|r|
                        r.sequence_id.is_some() && r.prompt_cursor == r.command.tokens.len());
                    if decoding_active {
                        effective.prefill_rows = bound(effective.prefill_rows, policy.mixed_prefill_rows);
                    }
                    crate::v2::scheduler::pipeline::PipelineSelection {
                        window: state.max_open_batches, open: state.open_batches.len(), decoding_active,
                        mixed_prefill_rows: policy.mixed_prefill_rows, effective_limits: effective,
                        decode_coalesce_max_ms: Some(2),
                    }
                });
            *before.lock().unwrap() = Some(crate::v2::commands::SchedulingSnapshot {
                service_budget: None,
                pipeline,
                ordinary_limits: state.ordinary_limits,
                ordinary_limits_applied: (state.ordinary_limits != Default::default() || pipeline.is_some())
                    && !state.equal_sequence_ubatch
                    && counts[5] == 0,
                min_batch_rows: state.min_batch_rows,
                max_issue_rows: state.max_issue_rows,
                max_open_batches: state.max_open_batches,
                prefill_fragments: state.prefill_fragments,
                open_batches_before_issue: state.open_batches.len(),
                pending_admission: counts[0],
                blocked_outstanding: counts[1],
                no_ready_input: counts[2],
                eligible_prefill: counts[3],
                eligible_decode: counts[4],
                eligible_atomic: counts[5],
            });
        }
        if point == "after_issue_accepted" {
            let native = native.lock().unwrap();
            let raw = native
                .issued_native
                .last()
                .expect("acceptance follows a native call")
                .result
                .as_ref()
                .expect("failed native call is never accepted");
            let physical = CapsuleSet::decode(raw).unwrap();
            let mut proofs = BTreeMap::new();
            for owner in physical.0.iter().flat_map(|capsule| &capsule.owners) {
                let request = &state.requests[&owner.sequence_key];
                assert_eq!(request.sequence_id, Some(owner.sequence_id));
                assert_eq!(request.incarnation, owner.incarnation);
                let proof = request
                    .issued_work
                    .expect("accepted request needs its witness")
                    .proof();
                assert_eq!(proof.last_ordinal, state.next_open_batch - 1);
                proofs.insert(owner.request_id.clone(), proof);
            }
            let mut accepted = accepted.lock().unwrap();
            assert!(
                accepted.len() < 4096,
                "bounded independent acceptance history"
            );
            accepted.push(AcceptedIssue {
                scheduling: before.lock().unwrap().take().expect("pre-native snapshot"),
                ordinal: state.next_open_batch - 1,
                physical,
                proofs,
            });
        }
        if let Some(extra) = &extra {
            extra(point, state);
        }
    })
}

fn route_key(route: &OuterEndpoint) -> String {
    format!(
        "{}|{}|{}",
        route.ingress_agent, route.channel, route.connection_generation
    )
}

fn input_for<'a>(h: &'a Harness, request: &str) -> &'a Event {
    h.submissions
        .iter()
        .find(|event| {
            serde_json::from_slice::<InferenceCommand>(&event.payload)
                .unwrap()
                .request_id
                == request
        })
        .expect("every native owner comes from an original submitted request")
}

fn carriers<'a>(h: &'a Harness, issue: &AcceptedIssue) -> BTreeMap<String, &'a Event> {
    let mut carriers = BTreeMap::new();
    for owner in issue.physical.0.iter().flat_map(|capsule| &capsule.owners) {
        let original = input_for(h, &owner.request_id);
        carriers
            .entry(route_key(original.envelope.return_route.as_ref().unwrap()))
            .or_insert(original);
    }
    carriers
}

fn assert_carrier(event: &Event, original: &Event) {
    assert_eq!(event.envelope.target, original.envelope.source);
    assert_eq!(event.envelope.return_route, original.envelope.return_route);
    assert_eq!(
        event.envelope.correlation_id,
        original.envelope.correlation_id
    );
    assert_eq!(
        event.envelope.deadline_unix_ms,
        original.envelope.deadline_unix_ms
    );
}

fn phases(owners: &[&RowOwner]) -> [usize; 4] {
    let mut counts = [0; 4];
    for owner in owners {
        counts[match owner.phase {
            Phase::Prefill => 0,
            Phase::Decode => 1,
            Phase::Verify => 2,
            Phase::Replay => 3,
        }] += 1;
    }
    counts
}

/// Validates each received projection against the independent native/accept
/// transcript. Missing entries return false; conflicting/foreign data fails.
fn complete(h: &Harness) -> bool {
    let accepted = h.accepted.lock().unwrap().clone();
    let mut expected_observations = BTreeSet::new();
    let mut expected_spans = BTreeSet::new();
    for issue in &accepted {
        for route in carriers(h, issue).keys() {
            expected_observations.insert((route.clone(), issue.ordinal));
            for stage in 0..h.nodes.len() {
                for capsule in &issue.physical.0 {
                    expected_spans.insert((route.clone(), stage, capsule.execution_id));
                }
            }
        }
    }
    let mut observations = BTreeSet::new();
    let mut spans = BTreeSet::new();
    let mut observation_ids = BTreeMap::new();
    for event in &h.received {
        if event.envelope.payload_content_type == BATCH_OBSERVATION_CONTENT_TYPE {
            assert_eq!(event.envelope.source, endpoint(0));
            let body: BatchObservation = serde_json::from_slice(&event.payload).unwrap();
            assert_eq!(body.load_generation, 1);
            assert_eq!(body.session_id, "loop-session");
            let issue = accepted
                .iter()
                .find(|issue| issue.ordinal == body.logical_ordinal)
                .expect("observation invented an unaccepted logical issue");
            // Preserve the existing independent request/flight/selection
            // oracle. New cost predictions are checked by service_budget's
            // producer/consumer counterexamples, not copied into this oracle.
            let mut scheduling = body.scheduling.clone();
            if let Some(s) = &mut scheduling { s.service_budget = None; }
            assert_eq!(
                scheduling.as_ref(),
                Some(&issue.scheduling),
                "selection diagnostics must match pre-native state, not post-issue state"
            );
            let route = route_key(event.envelope.return_route.as_ref().unwrap());
            let original = carriers(h, issue)[&route];
            assert_carrier(event, original);
            let id = (route.clone(), body.observation_id.clone());
            if let Some(previous) = observation_ids.insert(id, body.clone()) {
                assert_eq!(previous, body, "observation identity changed");
                continue;
            }
            assert!(
                observations.insert((route.clone(), body.logical_ordinal)),
                "different observations repeat one accepted issue"
            );
            assert_eq!(
                body.logical_rows,
                issue
                    .physical
                    .0
                    .iter()
                    .map(|capsule| capsule.owners.len())
                    .sum::<usize>()
            );
            assert_eq!(body.physical_batches.len(), issue.physical.0.len());
            let mut ids = BTreeSet::new();
            let mut mixed = 0;
            for batch in &body.physical_batches {
                assert!(ids.insert(batch.execution_id));
                let capsule = issue
                    .physical
                    .0
                    .iter()
                    .find(|capsule| capsule.execution_id == batch.execution_id)
                    .unwrap();
                let all: Vec<_> = capsule.owners.iter().collect();
                let counts = phases(&all);
                assert_eq!(
                    [
                        batch.prefill_rows,
                        batch.decode_rows,
                        batch.verify_rows,
                        batch.replay_rows
                    ],
                    counts
                );
                assert_eq!(batch.rows, all.len());
                mixed += usize::from(counts[0] > 0 && counts[1..].iter().sum::<usize>() > 0);
                assert_eq!(
                    batch.request_count,
                    all.iter()
                        .map(|owner| &owner.request_id)
                        .collect::<BTreeSet<_>>()
                        .len()
                );
                assert_eq!(
                    batch.sequence_count,
                    all.iter()
                        .map(|owner| owner.sequence_id)
                        .collect::<BTreeSet<_>>()
                        .len()
                );
                let mut owned = BTreeMap::<String, Vec<&RowOwner>>::new();
                for owner in &capsule.owners {
                    if route_key(
                        input_for(h, &owner.request_id)
                            .envelope
                            .return_route
                            .as_ref()
                            .unwrap(),
                    ) == route
                    {
                        owned
                            .entry(owner.request_id.clone())
                            .or_default()
                            .push(owner);
                    }
                }
                assert_eq!(batch.owned_requests.len(), owned.len());
                for request in &batch.owned_requests {
                    let rows = owned
                        .remove(&request.request_id)
                        .expect("foreign or duplicate observation owner");
                    let original = input_for(h, &request.request_id);
                    assert_eq!(request.submission_event_id, original.envelope.event_id);
                    assert_eq!(
                        (request.sequence_id, request.incarnation),
                        (rows[0].sequence_id, rows[0].incarnation)
                    );
                    assert_eq!(
                        request.request_issue_index,
                        issue.proofs[&request.request_id].issue_count
                    );
                    assert_eq!(
                        [
                            request.prefill_rows,
                            request.decode_rows,
                            request.verify_rows,
                            request.replay_rows
                        ],
                        phases(&rows)
                    );
                    assert_eq!(
                        request
                            .rows
                            .iter()
                            .map(|row| (row.phase, row.position))
                            .collect::<Vec<_>>(),
                        rows.iter()
                            .map(|row| (row.phase, row.position))
                            .collect::<Vec<_>>()
                    );
                }
                assert!(owned.is_empty());
            }
            assert_eq!(body.mixed_physical_batches, mixed);
        } else if event.envelope.payload_content_type == STAGE_SPAN_CONTENT_TYPE {
            let body: StageSpan = serde_json::from_slice(&event.payload).unwrap();
            assert_eq!(body.load_generation, 1);
            assert_eq!(body.session_id, "loop-session");
            let stage = (0..h.nodes.len())
                .find(|index| event.envelope.source == endpoint(*index))
                .expect("span from undeclared stage");
            assert!(
                body.ingress_unix_ms <= body.start_unix_ms
                    && body.start_unix_ms <= body.end_unix_ms
                    && body.end_unix_ms <= body.forward_unix_ms
            );
            assert_eq!(
                body.execution_ids,
                body.executions
                    .iter()
                    .map(|execution| execution.execution_id)
                    .collect::<Vec<_>>()
            );
            assert!(!body.executions.is_empty());
            let route = route_key(event.envelope.return_route.as_ref().unwrap());
            let carrier = body
                .executions
                .iter()
                .find_map(|execution| {
                    accepted
                        .iter()
                        .flat_map(|issue| &issue.physical.0)
                        .find(|capsule| capsule.execution_id == execution.execution_id)
                        .and_then(|capsule| {
                            capsule.owners.iter().find_map(|owner| {
                                let original = input_for(h, &owner.request_id);
                                (route_key(original.envelope.return_route.as_ref().unwrap())
                                    == route)
                                    .then_some(original)
                            })
                        })
                })
                .expect("a recipient span needs at least one owned row");
            assert_carrier(event, carrier);
            let mut rows = 0;
            for execution in &body.executions {
                let issue = accepted
                    .iter()
                    .find(|issue| {
                        issue
                            .physical
                            .0
                            .iter()
                            .any(|capsule| capsule.execution_id == execution.execution_id)
                    })
                    .expect("span invented an unaccepted execution");
                let capsule = issue
                    .physical
                    .0
                    .iter()
                    .find(|capsule| capsule.execution_id == execution.execution_id)
                    .unwrap();
                rows += capsule.owners.len();
                let expected: BTreeSet<_> = capsule
                    .owners
                    .iter()
                    .filter(|owner| {
                        route_key(
                            input_for(h, &owner.request_id)
                                .envelope
                                .return_route
                                .as_ref()
                                .unwrap(),
                        ) == route
                    })
                    .map(|owner| {
                        (
                            owner.request_id.clone(),
                            owner.sequence_id,
                            owner.incarnation,
                        )
                    })
                    .collect();
                let actual: BTreeSet<_> = execution
                    .owned_requests
                    .iter()
                    .map(|owner| {
                        (
                            owner.request_id.clone(),
                            owner.sequence_id,
                            owner.incarnation,
                        )
                    })
                    .collect();
                assert_eq!(actual.len(), execution.owned_requests.len());
                assert_eq!(actual, expected);
                assert!(
                    spans.insert((route.clone(), stage, execution.execution_id)),
                    "a fresh stage execution must be observed exactly once"
                );
            }
            assert_eq!(body.rows, rows);
        }
    }
    assert!(observations.is_subset(&expected_observations));
    assert!(spans.is_subset(&expected_spans));
    observations == expected_observations && spans == expected_spans
}

impl Harness {
    pub(super) fn wait_for_observations(&mut self) {
        // TokenizeChain is a causal ingress producer, not a prefilled list.
        // Only the exact Events whose actual bounded sends succeeded become
        // submission authority; the existing chain completion oracle stays.
        let causal = self.nodes[0]
            .native
            .lock()
            .unwrap()
            .generated_submissions
            .clone();
        for event in causal {
            if let Some(previous) = self
                .submissions
                .iter()
                .find(|previous| previous.envelope.event_id == event.envelope.event_id)
            {
                assert_eq!(previous, &event, "causal ingress ID changed its bytes");
            } else {
                self.submissions.push(event);
            }
        }
        self.until("all accepted issue/owner/stage observations", complete);
        assert_terminal_proofs(self);
    }
}

pub(super) fn assert_terminal_proofs(h: &Harness) {
    let accepted = h.accepted.lock().unwrap();
    for event in h
        .received
        .iter()
        .filter(|event| event.envelope.payload_content_type == OUTPUT_CONTENT_TYPE)
    {
        let output: ApprovedOutputPayload = serde_json::from_slice(&event.payload).unwrap();
        if output.outcome.stop.is_none() {
            assert!(output.issued_work.is_none());
        } else {
            let expected = accepted
                .iter()
                .rev()
                .find_map(|issue| issue.proofs.get(&output.outcome.request_id))
                .expect("terminal request must have accepted native work");
            assert_eq!(
                output.issued_work.as_ref(),
                Some(expected),
                "terminal changed/lost the real accepted witness"
            );
        }
    }
}
