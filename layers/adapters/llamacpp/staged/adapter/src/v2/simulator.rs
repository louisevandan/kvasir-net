//! A deterministic pipeline that runs the real scheduler and checks what it did.
//!
//! Every batching decision so far was judged by pointing four processes at two
//! GPUs and reading throughput, and that answered the wrong question four
//! separate times: a request count was read as a row count, open RPCs as GPU
//! concurrency, a bimodal latency as its mean, and a run-order drift as a
//! policy effect. None of those is a measurement problem. They happen because
//! there was nothing that could say, for a given state, what the next batch had
//! to be - so an experiment was the only way to find out, and an experiment
//! cannot distinguish a policy from the noise it is buried in.
//!
//! This is that missing thing. It drives the real `Scheduler` and the real
//! `RequestState::phase_within` through a virtual pipeline with no clock, no
//! sockets and no llama.cpp, so a run is a pure function of its inputs. What it
//! adds is the bookkeeping that a distributed pipeline needs and a single
//! in-process scheduler gets for free: which rows were issued, which came back,
//! and whether the two ever disagree.
//!
//! Deliberately not here: transport, credit accounting, timeouts,
//! cancellation, reconnection. Those belong to the fragment ledger the plan
//! calls P4.5, and modelling them before that contract exists would be the same
//! mistake at a different layer.

use super::node::state::{ReadyRows, RequestState};
use super::scheduler::{Demand, Phase, Scheduler};

/// Violations a run collects before it gives up.
const VIOLATION_CAP: usize = 16;

/// How the virtual pipeline behaves. All of it is deterministic.
#[derive(Clone, Copy, Debug)]
pub(super) struct PipelineShape {
    /// Stages a fragment passes before the tail settles it. The lap length.
    pub stages: usize,
    /// Rows one physical batch may carry.
    pub physical_capacity: usize,
    /// Rows one logical batch may carry.
    pub batch_capacity: usize,
    /// Fragments of one prompt allowed in flight. 1 is the shipped behaviour.
    pub prefill_fragments: u32,
    /// Whether the model forces equal per-sequence widths, as a recurrent or
    /// hybrid memory does.
    pub equal_sequence_ubatch: bool,
}

impl Default for PipelineShape {
    fn default() -> Self {
        Self {
            stages: 4,
            physical_capacity: 64,
            batch_capacity: 512,
            prefill_fragments: 1,
            equal_sequence_ubatch: false,
        }
    }
}

/// One issued fragment, travelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Fragment {
    pub id: u64,
    pub request: String,
    pub phase: Phase,
    pub rows: usize,
    /// Where the prompt rows came from, so overlaps and gaps are visible.
    pub token_range: Option<(usize, usize)>,
    /// Stages left before the tail settles it.
    pub remaining_stages: usize,
}

/// What one tick issued, for a golden trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct IssuedBatch {
    pub tick: usize,
    pub rows: Vec<(String, Phase, usize)>,
}

/// An invariant that did not hold, named so a failure says which.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Violation {
    pub tick: usize,
    pub rule: &'static str,
    pub detail: String,
}

pub(super) struct Simulation {
    shape: PipelineShape,
    scheduler: Scheduler,
    requests: Vec<(String, RequestState)>,
    in_flight: Vec<Fragment>,
    next_fragment: u64,
    issued_ids: Vec<u64>,
    /// Prompt ranges issued per request, in issue order.
    issued_ranges: Vec<(String, usize, usize)>,
    /// Every decode row issued, in order, as (request, input position).
    pub issued_decodes: Vec<(String, Option<u32>)>,
    /// Where each prompt is expected to resume, compared at issue time.
    expected_prompt: std::collections::HashMap<String, usize>,
    /// The position each request is expected to decode next.
    expected_decode: std::collections::HashMap<String, u32>,
    /// Fragment ids handed out, so a reuse is caught where it happens.
    seen_ids: std::collections::HashSet<u64>,
    /// Logical time, carried across calls to `run`.
    now: usize,
    settled_rows: usize,
    issued_rows: usize,
    pub trace: Vec<IssuedBatch>,
    pub violations: Vec<Violation>,
}

impl Simulation {
    pub fn new(shape: PipelineShape) -> Self {
        Self {
            shape,
            scheduler: Scheduler::new(),
            requests: Vec::new(),
            in_flight: Vec::new(),
            next_fragment: 1,
            issued_ids: Vec::new(),
            issued_ranges: Vec::new(),
            issued_decodes: Vec::new(),
            expected_prompt: std::collections::HashMap::new(),
            expected_decode: std::collections::HashMap::new(),
            seen_ids: std::collections::HashSet::new(),
            now: 0,
            settled_rows: 0,
            issued_rows: 0,
            trace: Vec::new(),
            violations: Vec::new(),
        }
    }

    /// Admits a request with `prompt` tokens that will generate `generate` of
    /// its own. Sequence ids are handed out in admission order.
    pub fn admit(&mut self, id: &str, prompt: usize, generate: u32) {
        let sequence = self.requests.len() as u32;
        let mut state = crate::v2::tests::request_state(vec![7; prompt]);
        state.command.request_id = id.to_owned();
        state.command.max_tokens = generate;
        state.sequence_id = Some(sequence);
        self.requests.push((id.to_owned(), state));
    }

    /// Runs until every request has finished or `budget` ticks have passed.
    /// Returns the number of ticks used.
    /// The logical time is cumulative: a second call continues where the
    /// first stopped. It used to restart at zero, so splitting a run to admit
    /// a request midway rewound the clock while the fragments in flight and
    /// the request states carried on - `run(20); run(20)` traced ticks
    /// 0,4,8,12,16,0,4,8,12,16. Nothing asserted on a tick value, so no test
    /// could see it, and every arrival time, wait and deadline built on it
    /// afterwards would have been wrong.
    pub fn run(&mut self, budget: usize) -> usize {
        let start = self.now;
        for tick in start..start + budget {
            self.now = tick + 1;
            self.advance(tick);
            self.issue(tick);
            self.check(tick);
            // A broken model stops here rather than spending its whole budget
            // re-deriving the same failure. A settlement that finds nothing
            // outstanding never advances its request, so `finished` never
            // becomes true, and a run that should have reported one defect in
            // a millisecond instead ran to its budget and had to be killed -
            // a checker that hangs on what it exists to report.
            if self.violations.len() >= VIOLATION_CAP {
                return tick + 1 - start;
            }
            if self.finished() {
                return tick + 1 - start;
            }
        }
        budget
    }

    fn finished(&self) -> bool {
        self.in_flight.is_empty()
            && self.requests.iter().all(|(_, request)| {
                request.prompt_cursor == request.command.tokens.len()
                    && request.generated >= request.command.max_tokens
            })
    }

    /// Moves every fragment one stage, and settles the ones that reach the tail.
    fn advance(&mut self, tick: usize) {
        let mut arrived = Vec::new();
        for fragment in &mut self.in_flight {
            fragment.remaining_stages -= 1;
            if fragment.remaining_stages == 0 {
                arrived.push(fragment.clone());
            }
        }
        self.in_flight.retain(|fragment| fragment.remaining_stages > 0);

        for fragment in arrived {
            let Some((_, request)) = self
                .requests
                .iter_mut()
                .find(|(id, _)| *id == fragment.request)
            else {
                self.violations.push(Violation {
                    tick,
                    rule: "settled fragment belongs to a live request",
                    detail: format!("{} is not admitted", fragment.request),
                });
                continue;
            };
            if request.outstanding == 0 {
                self.violations.push(Violation {
                    tick,
                    rule: "a settlement matches an outstanding fragment",
                    detail: format!("{} settled with nothing in flight", fragment.request),
                });
                continue;
            }
            request.outstanding -= 1;
            self.settled_rows += fragment.rows;
            match fragment.phase {
                Phase::Prefill => {
                    request.prompt_cursor += fragment.rows;
                    // The prompt is complete: the tail hands back the first
                    // generated token, exactly as the real settlement does.
                    if request.prompt_cursor == request.command.tokens.len() {
                        request.generated += 1;
                    }
                }
                _ => request.generated += 1,
            }
            // One rule for both, because there is one: a settlement produces a
            // token, and the next input row is that token at its own position.
            //
            // Writing it twice got both halves wrong. The prefill arm set a
            // ready row without asking whether the limit was already reached,
            // so `max_tokens = 1` generated two; and the decode arm derived
            // the position as `prompt + generated`, which is one past the
            // token just produced - a 20-token prompt decoded at 20 and then
            // at 22, skipping 21. Neither showed up in a trace that carries no
            // positions and a check that counted no tokens.
            request.ready = if request.generated < request.command.max_tokens {
                Some(ready_decode(next_input_position(request)))
            } else {
                None
            };
        }
    }

    /// Plans one batch from whatever is ready and sends it into the pipeline.
    fn issue(&mut self, tick: usize) {
        let limit = self.shape.prefill_fragments;
        let mut demands = Vec::new();
        for (id, request) in &self.requests {
            let Some(phase) = request.phase_within(limit) else {
                continue;
            };
            let available_rows = match phase {
                Phase::Prefill => request.command.tokens.len() - request.prompt_issued,
                _ => request.ready.as_ref().map_or(0, |ready| ready.tokens.len()),
            };
            if available_rows == 0 {
                continue;
            }
            demands.push(Demand {
                request_id: id.clone(),
                sequence_id: request.sequence_id.expect("admitted"),
                compatibility: "sim".into(),
                phase,
                available_rows,
                atomic: false,
            });
        }
        if demands.is_empty() {
            return;
        }
        let Ok(allocations) = self.scheduler.plan_with_physical_capacity(
            &demands,
            self.shape.batch_capacity,
            self.shape.physical_capacity,
            self.shape.equal_sequence_ubatch,
            16,
            false,
        ) else {
            self.violations.push(Violation {
                tick,
                rule: "the scheduler plans whatever is ready",
                detail: "planning failed with demands present".into(),
            });
            return;
        };

        let mut rows = Vec::new();
        let mut issue_faults = Vec::new();
        let expected_prompt = &mut self.expected_prompt;
        let expected_decode = &mut self.expected_decode;
        let seen_ids = &mut self.seen_ids;
        for allocation in allocations {
            if allocation.rows == 0 {
                continue;
            }
            let stages = self.shape.stages;
            let id = self.next_fragment;
            self.next_fragment += 1;
            // Incremental, because sorting every id ever issued on every tick
            // was the same rescan as the two below it.
            if !seen_ids.insert(id) {
                issue_faults.push(Violation {
                    tick,
                    rule: "a fragment id is never reused",
                    detail: format!("fragment {id} issued twice"),
                });
            }
            let (_, request) = self
                .requests
                .iter_mut()
                .find(|(name, _)| *name == allocation.request_id)
                .expect("allocation names an admitted request");
            let token_range = (allocation.phase == Phase::Prefill).then(|| {
                let from = request.prompt_issued;
                request.prompt_issued += allocation.rows;
                (from, request.prompt_issued)
            });
            // Compared here rather than in `check`, because here it is one
            // comparison and there it was a rescan of every row ever issued,
            // per request, per tick.
            if allocation.phase == Phase::Prefill {
                let from = token_range.expect("a prefill carries a range").0;
                let expected = expected_prompt
                    .entry(allocation.request_id.clone())
                    .or_insert(0);
                if from != *expected {
                    issue_faults.push(Violation {
                        tick,
                        rule: "prompt fragments tile the prompt in order",
                        detail: format!(
                            "{}: expected {expected}, got {from}",
                            allocation.request_id
                        ),
                    });
                }
                *expected = request.prompt_issued;
            } else {
                let position = request.ready.as_ref().map(|ready| ready.position);
                let expected = expected_decode
                    .entry(allocation.request_id.clone())
                    .or_insert(request.command.tokens.len() as u32);
                if position != Some(*expected) {
                    issue_faults.push(Violation {
                        tick,
                        rule: "decode positions advance by one from the prompt",
                        detail: format!(
                            "{}: expected {expected}, got {position:?}",
                            allocation.request_id
                        ),
                    });
                }
                *expected += 1;
                self.issued_decodes
                    .push((allocation.request_id.clone(), position));
            }
            request.outstanding += 1;
            self.issued_rows += allocation.rows;
            self.issued_ids.push(id);
            if let Some((from, to)) = token_range {
                self.issued_ranges
                    .push((allocation.request_id.clone(), from, to));
            }
            rows.push((
                allocation.request_id.clone(),
                allocation.phase,
                allocation.rows,
            ));
            self.in_flight.push(Fragment {
                id,
                request: allocation.request_id,
                phase: allocation.phase,
                rows: allocation.rows,
                token_range,
                remaining_stages: stages,
            });
        }
        self.violations.extend(issue_faults);
        if !rows.is_empty() {
            self.trace.push(IssuedBatch { tick, rows });
        }
    }

    /// Everything that must hold after every tick.
    fn check(&mut self, tick: usize) {
        let mut found = Vec::new();

        // A settled row was issued, and an issued row is settled or travelling.
        let travelling: usize = self.in_flight.iter().map(|fragment| fragment.rows).sum();
        if self.issued_rows != self.settled_rows + travelling {
            found.push(Violation {
                tick,
                rule: "issued = settled + travelling",
                detail: format!(
                    "issued {} settled {} travelling {travelling}",
                    self.issued_rows, self.settled_rows
                ),
            });
        }


        for (id, request) in &self.requests {
            // The settled cursor trails the issued one and neither passes the prompt.
            if request.prompt_cursor > request.prompt_issued
                || request.prompt_issued > request.command.tokens.len()
            {
                found.push(Violation {
                    tick,
                    rule: "cursor <= issued <= prompt",
                    detail: format!(
                        "{id}: cursor {} issued {} prompt {}",
                        request.prompt_cursor,
                        request.prompt_issued,
                        request.command.tokens.len()
                    ),
                });
            }
            // A decode has one fragment out at most, whatever the limit says,
            // because its next row is the tail's answer to this one. Counting
            // every outstanding fragment instead was this check's own first
            // bug: two prompt fragments travelling together is the feature,
            // not a violation, and the simulator reported it as one.
            let decodes = self
                .in_flight
                .iter()
                .filter(|fragment| fragment.request == *id && fragment.phase != Phase::Prefill)
                .count();
            if decodes > 1 {
                found.push(Violation {
                    tick,
                    rule: "a decode has at most one fragment in flight",
                    detail: format!("{id}: {decodes} decode fragments"),
                });
            }

            // The request's own counter against the fragments that exist.
            //
            // `issued = settled + travelling` is a whole-simulation sum, and
            // the same code maintains all three terms, so a per-request ledger
            // that drifts cancels out of it. Dropping an `outstanding += 1`
            // and issuing again left two fragments travelling against a count
            // of one and every check passed - which is the exact class of
            // defect this file exists to catch.
            let mine = self
                .in_flight
                .iter()
                .filter(|fragment| fragment.request == *id)
                .count();
            if request.outstanding as usize != mine {
                found.push(Violation {
                    tick,
                    rule: "outstanding counts the fragments in flight",
                    detail: format!(
                        "{id}: counter {} against {mine} travelling",
                        request.outstanding
                    ),
                });
            }

            // The fragment limit is a limit, not a hint.
            let prefills = self
                .in_flight
                .iter()
                .filter(|fragment| fragment.request == *id && fragment.phase == Phase::Prefill)
                .count();
            if prefills > self.shape.prefill_fragments as usize {
                found.push(Violation {
                    tick,
                    rule: "prompt fragments in flight stay within the limit",
                    detail: format!(
                        "{id}: {prefills} travelling against a limit of {}",
                        self.shape.prefill_fragments
                    ),
                });
            }

            // A request stops at the tokens it asked for. The real server
            // raises `stop=\"length\"` at the limit and the worker releases the
            // sequence; nothing here may generate past it.
            if request.generated > request.command.max_tokens {
                found.push(Violation {
                    tick,
                    rule: "generated never passes max_tokens",
                    detail: format!(
                        "{id}: {} generated against a limit of {}",
                        request.generated, request.command.max_tokens
                    ),
                });
            }


        }

        self.violations.extend(found);
    }
}

/// The position of the row a request feeds next.
///
/// The prompt occupies `0 .. prompt`, so the token the prefill produced sits
/// at `prompt` and is the first decode input. Each settled decode advances by
/// exactly one. `generated` counts tokens produced, so the last one is at
/// `prompt + generated - 1`, and that is what goes back in.
fn next_input_position(request: &RequestState) -> u32 {
    request.command.tokens.len() as u32 + request.generated - 1
}

fn ready_decode(position: u32) -> ReadyRows {
    ReadyRows {
        phase: Phase::Decode,
        tokens: vec![11],
        position,
        speculative_id: 0,
    }
}
