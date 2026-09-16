# llama adapter batching plan — micro-batching proposal compared with the code

Code review of 2026-09-13. The base HEAD is `d122125bafeaa6d32790761669f1bfa5868d8078`.
Its status is **execution-path review and improvement candidates**; it is not an implementation or performance promotion document.
The reviewed `staged/adapter` and `staged/server` had no uncommitted changes at the start.
Separate OUTER model batching policy and roadmap changes in the working tree are not part of this review.
During the review, separate work moved HEAD to `484b856ee7e53aea5b850b654c45da53cb0724a6`.
The adapter/server paths above have no diff between the two HEADs, and the same sources in the final working tree were identical.

The question is: “Is it better than the current implementation to split prefill into small pieces to cut node idle gaps, batch decode requests widely,
and schedule prefill into the remaining budget?” The attached figure is a comparison target, and
the explanations inside it were not treated as implementation instructions.
Contracts are owned by [batching layers](adapter-batching-layers.md) and [isolation](layer-isolation-contract.md);
real-hardware acceptance is owned by the [verification protocol](distributed-batching-verification.md).
Execution order follows the [roadmap](distributed-batching-roadmap.md#current-status).
The review priorities below are not instructions to start new development, deployment or experiments.

## 1. Verdict

**D-first with residual P allocation is already implemented. But describing the default batcher, the optional pipeline policy
and native internal UBATCH splitting as one feature misses the real differences.**

| User proposal | Actual code | Difference and verdict |
| --- | --- | --- |
| Split prefill finely and continue it stage by stage | Logical issues can repeat, but native internal UBATCHes are returned together after one call finishes | Shrinking `n_ubatch` alone does not produce the per-UBATCH node overlap shown in the figure. The logical issue unit and the delivery unit differ a lot |
| Grow the decode batch to spread memory-read cost | Default ordinary selects ready D up to capacity. The pipeline policy limits participation width by active decode population/window | Width control exists, but it is population arithmetic, not cost optimization. Joint selection of width and number of independent groups is still open |
| Put decode in first, then prefill into the remaining budget | Both ordinary paths allocate D first. The optional total budget applies to D+P as a whole | The principle matches. Setting only `mixed_prefill_rows` adds D on top of that number, so it differs from a total budget |
| Adjust prefill size automatically to load | Default/profile settings are fixed row caps. A separate online service predictor shrinks P and replans | Some automatic reselection exists. There is no D width/window selection by execution cost, and actual KV cost and request deadlines are not used |
| Fill the pipeline with several small pieces even when only one long request exists | RequestState has a fragment limit, but the pipeline policy allows only fragment1 | Unsupported combination. A simple setting change cannot solve it |

So the conclusion “just shrink the prefill chunk a little more” is not enough. Within the current scope the closest
comparison target is **the optional pipeline policy's logical issue volume, D participation width and window**. Immediate UBATCH delivery is
a separate execution contract change and must be kept apart from scheduler tuning within the current contract.

## 2. Actual consumption path and code evidence

Line numbers in the links below refer to the HEAD above. File paths and function names are kept alongside so they can still be found after later moves.
`W` is shorthand used only for adapter `v2/node/worker`, and `S` only for adapter `v2/scheduler`.

| Stage | Actual code reference | Confirmed behaviour |
| --- | --- | --- |
| LOAD capacity | [control.rs:76–91](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/control.rs#L76), `Worker::load` | after readiness validation, stores `n_batch`, `n_ubatch` and equal-width/atomic capability in state |
| Configuration | [state.rs:522–579](../layers/adapters/llamacpp/staged/adapter/src/v2/node/state.rs#L522), `AdapterState::default` | existing row/window limits, optional pipeline policy, fragment limit |
| Configuration acceptance | [worker.rs:431–461](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker.rs#L431), `Worker::prefill` | validates finite window, positive budget and fragment1; rejects using the total budget together with the online controller |
| Execution loop | [worker.rs:309–378](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker.rs#L309), `Worker::run_to_exit` | after bounded ingress processing, `drive_one_batch`, then re-entry through the coalescing timer's `recv_timeout` |
| Eligibility | [state.rs:368–379](../layers/adapters/llamacpp/staged/adapter/src/v2/node/state.rs#L368), `RequestState::phase_within` | limits on unissued prompt and outstanding; decode is eligible only when the previous outstanding is 0 |
| Demand/population collection | [drive.rs:81–133](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L81), `Worker::drive_one_batch` | tallies one session's ready demand and the admitted ready/inflight/waiting population separately |
| Group selection | [pipeline.rs:71–115](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/pipeline.rs#L71), `PipelinePolicy::select` | computes P/D participation caps from the active population. Applies the P quantum when active decode exists |
| Total and mode selection | [drive.rs:217–244](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L217), [scheduler.rs:171–205](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L171) | after the total issue cap, branches into bounded ordinary / legacy ordinary / equal / atomic |
| Actual row allocation | [scheduler.rs:588–682](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L588), `plan_bounded_ordinary`; [686–758](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L686), `plan_ordinary` | D first, 1 row each, then the remaining P assigned in rotation. The starvation-prevention difference between the two paths is in §5 |
| Time prediction and replanning | [worker/service.rs:57–115](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/service.rs#L57), `prepare_generation_service_plan`; [scheduler/service.rs:298–321](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/service.rs#L298), `project_tail` | projects open issue order and stage RPC cost; halves P and reselects / defers / probes |
| Join wait | [drive.rs:252–277](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L252) | waits briefly for D-only candidates only. Target width is limited by group width and row capacity |
| native issue | [drive.rs:417–461](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L417) | after issue preparation, a synchronous `stage_request(LogicalBatch, PhysicalResult)` |
| UBATCH capture | [llama_stage_runtime_physical.cpp:92–128](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_physical.cpp#L92), `capture_execution`; [131–235](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_physical.cpp#L131), `execute_first_batch` | accumulates callback results into a vector. Returns everything after `llama_decode` completes and row preservation is verified |
| native result packaging | [server_physical.cpp:196–301](../layers/adapters/llamacpp/staged/server/src/server/server_physical.cpp#L196), `Session::handle_logical_batch` | maps owners onto the entire capture and encodes it as one PhysicalResult |
| Transfer after approval | [drive.rs:528–583](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L528) | after the accepted issue, frontier and fairness are committed, keeps the exact result bytes in `ForwardObserved` and forwards them |
| Next stage | [worker/physical.rs:7–103](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/physical.rs#L7), `Worker::physical` | validates the fresh members of the CapsuleSet and calls PhysicalBatch. The next stage does not rearrange D/P on its own |

Batching policy is owned by the concrete adapter. This improvement is not turned into work that puts a llama-specific scheduler
into the P4 common transport.

## 3. Scope of the implemented policies

### 3.1 Default ordinary and optional pipeline are different

`PipelinePolicy` is created only when `P4_STAGED_PIPELINE_BATCHING` is exactly `1`.
The default `OrdinaryLimits` is 0 on every axis; here 0 means keeping the legacy limit, not “process 0 tokens”.
Ordinary's logical capacity is `n_batch`, and `n_ubatch` is the native split cap.
Bounded ordinary also checks physical capacity, but it does not always cut the allocation down to one UBATCH or less.
Evidence: `Scheduler::prepare_plan_with_limits` lines 181–197.

With pipeline window `W` and admitted active D population `N_D`:

```text
target D group count = min(N_D, W)
D participation width cap = ceil(N_D / max(target D group count, 1)), minimum 1
P participation width cap = ceil((unissued prompt population + population waiting for the last prompt return) / W)
explicit user limits and the number of eligible requests also apply to the actual participation width.
```

This does not reserve fixed group identities. On every issue it builds a **width cap** from the population and
picks rotated ready requests within it. `decode_groups`/`prefill_groups` are not the actual number of concurrent GPU executions.
`select` has no per-stage time, bandwidth or KV cost input.

While decode is in progress, `mixed_prefill_rows` applies as the cap on all P. It applies even when ready D is 0 because D is currently at another
stage. This limit does not apply to the initial pure prefill before decode starts.
The optional `mixed_batch_rows` is a D+P total cap while decode is active, and both caps are enforced together.

### 3.2 An online cost policy exists, but it is a separate experimental path

`P4_STAGED_PREFILL_SERVICE_MS` is read in `configured_budget` and is disabled by default.
When enabled, it uses `ServiceShape(P rows, D rows, request count, max input position)` and per-stage RPC samples.
`project_tail` estimates the completion of existing open batches and of the candidate as `max(arrival from the previous stage, availability of this stage)+cost`.
Candidates that exceed the budget or have unknown cost are replanned with P halved each time, and can turn into a minimal probe or D-only.

So the claim that “there is no code at all that re-cuts prefill based on time” is wrong.
But this path does not optimize D participation width or `max_open_batches`, and combining it with the total row budget setting
is rejected. A fixed `mixed_batch_rows` is not a value the runtime finds from a profile and changes automatically.
Code evidence: [service.rs:7–29](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/service.rs#L7),
[reselection loop](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/service.rs#L70),
[rejection of combined settings](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker.rs#L458).

## 4. How much the result differs for the same input

Below are **row allocation results from calling the current source's selector directly** with the local probe in §7.
They are not measurements of GPU/TPS, actual worker arrival patterns or native UBATCH membership.
Input: 32 ready D requests, 8 ready P requests (4096 rows each), logical capacity 512, physical cap 128,
and, when pipeline is used, window 4, P cap 128 and no extra user limit.

| Path | Selected D rows | Selected P rows | Logical total | Participating requests |
| --- | ---: | ---: | ---: | ---: |
| default ordinary | 32 | 480 | 512 | 40 |
| pipeline, P cap 128 only | 8 | 128 | 136 | 10 |
| pipeline, plus total cap 128 | 8 | 120 | 128 | 10 |

The second and third differ by 8 P rows. The first and third differ in that 24 D requests are left for the next independent
issue opportunity and this issue's P is 360 rows smaller. For this input, 136 rows cannot fit into the physical cap of 128,
so native splitting is needed, but the exact physical P/D mix must be confirmed from callback results.
These numbers are not used to compute a TPS improvement rate.

Whether the MB1 result goes to the next node immediately, as in the figure, differs as follows.

```text
Current case where one logical issue is split into several UBATCHes:
head: [UB1 compute/capture][UB2 compute/capture]…[full validation/packaging] → CapsuleSet delivered to next

Case where separate independent logical issues A/B are possible:
head: [A native done/approve/forward][B native done/approve/forward]…
next:                                [A execute]…
```

Today the overlap opportunity lies **between independent logical issues**, as in the second case. Raising the number of UBATCHes in the first case
does not turn it into a path where next starts as soon as the first UBATCH finishes at head.

## 5. Improvement candidates and required counterexamples

### G1 — Prefill progress guarantee differs in default ordinary: review first

`plan_ordinary` lines 705–724 allocate P after D has used up capacity. With capacity 8 and ready D8 plus
P1 offered identically at every opportunity, 32 accepted selections gave **D256/P0**. Giving bounded ordinary
`decode_members=8, prefill_members=1, prefill_rows=128` gave **D224/P32**.
When P exists, the bounded path limits D capacity to `capacity-1`, and at capacity1 it uses patience.

This is a saturation counterexample for the default selector. This probe does not prove that real arrivals/returns keep D8 every time,
so we do not claim to have observed unbounded starvation of the whole service. The wording of the initial answer,
“the current scheduler has prefill starvation prevention”, must distinguish these two paths.

The improvement candidate is to apply an explicit P progress-opportunity contract to default ordinary as well, or to make policy selection per supported combination
explicit. **This is not a proposal to switch the default to pipeline ON before verification.**
A real worker counterexample must combine sustained returns with a ready D count below/equal to/above capacity and waiting P,
and check the first, consecutive and last selection intervals of each P as well as normal D progress.
On rejection, preservation of fairness, reservations and native calls, and a guard-removal mutation, are also needed.

### G2 — UBATCH splitting and the transfer boundary: a high-impact structural difference

`capture_execution` accumulates results in `captured_executions_`, and `execute_first_batch`
returns after `llama_decode` ends. `handle_logical_batch` and `drive_one_batch` forward once, after full row validation and
approval. **A per-UBATCH streaming pipeline is not implemented.**

A small improvement candidate within the current contract is to tune the P cap and participant count of a logical issue so that independent requests remain.
True early UBATCH delivery is a separate candidate. It first needs a design for the KV effects already sent downstream after a partial native failure,
partial result identity, storage pre-reservation, backpressure, and cancellation/settlement authority.
It must not be treated as a small fix that sends directly from inside the callback.

The discriminating counterexample makes one logical call produce at least 2 UBATCHes and measures the UB1 completion→first send time and
the UB2 completion time. An early-delivery candidate must also exactly preserve the case where UB2 fails after downstream has received UB1.
The current behaviour was confirmed from source; the native early-delivery test is not run.

### G3 — Decode width and window are not cost-optimal values

`PipelinePolicy::select` computes D width from population and window alone. With D32/window 4 it is 8; with window 8 it is 4.
It does not judge whether each stage needs a wider D because of memory reads or per-batch fixed cost.
Conversely, a selection that gathers every ready request can reduce the supply of independent flights.
The 2ms coalescing is likewise a feature that waits within the currently legal group width, not an optimal-width search.

The improvement candidate is to select D width and logical window candidates together, using prior measurements bound to model, backend, context and topology.
Several stages on one device are not counted as independent GPUs either. When comparing, fix row width and
window as separate axes, and judge stage gaps, time per batch, correct generation TPS and ITL together.
Even if normalized cost input is added to `select`, KV/flight approval authority stays in the existing ledger.

### G4 — Gap between fixed row budgets and actual context cost

The default/fixed-profile path applies the same P row cap across the whole context range. The online path has cost samples per base-2 bucket of the maximum
position, but `ServiceShape::last_position` is not the actual backend
`n_kv`. [ServiceShape definition](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/service.rs#L13),
[prediction input filter](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/service.rs#L219).
The positions of the selected requests alone cannot explain the shared KV range and mask cost taken up by other requests.

The improvement candidate is to first measure native's actual KV access range, mask creation/copy, kernel and RPC wait costs separately,
and pass only the needed normalized observations into the adapter's cost samples. Next, compare the cost curves of D width and P quantum
for short and long contexts of the same model. The meaning of the cost input must be verified before simply merging the automatic predictor
with the fixed budget.
The current review cannot decide whether memory bandwidth is the main bottleneck, or how much GPU utilization would change.

### G5 — Several prefill pieces of one request: an intentionally closed combination

`RequestState::phase_within` can represent several outstanding prompt pieces, but
the pipeline policy rejects `prefill_fragments != 1` at entry and in drive.
So a configuration with only one request cannot be filled with several logical flights even if the P chunk is made smaller.

If a shortage of independent requests is confirmed as the real bottleneck, review a separate candidate that supports multi-fragment for ordinary attention
together with the pipeline policy. The required counterexamples are per-stage ordering and duplicate positions of chunks from the same request,
mid-way cancellation/failure, early decode of the last prompt, and slot reuse. Do not simply delete the two guards.
recurrent/hybrid keep the [equal-width path](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L328),
and Verify/Replay keep the [atomic path](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L305);
the ordinary mixed batch contract is not applied to them as is.

### G6 — Latency targets and the observation scope of cost optimization

`Demand` has no request deadline, wait time or actual KV cost. The online service budget is a completion prediction for the candidate's
pipeline RPC; it is not a deadline scheduler that directly controls the token-receive ITL seen by OUTER.
`drive_one_batch` leaves some reasons for unissued waiting in a snapshot, but it does not fully report cumulative wait time per policy or
per-request latency budget consumption.

The improvement candidate is to first bind issue-block reasons/durations and per-request TTFT/ITL to real observations.
After that, if it is confirmed that P progress opportunity and D latency must be bounded together, add per-request age/deficit to the candidate input.
Rejected plans must not consume this state.
Keep the existing `prepare/validate/commit` and the source/binary binding mutation protocol.

## 6. Scope of this verification

Before running, we confirmed that the reviewed sources had no uncommitted differences. This change covers documents and indexes only and
did not modify product code, expected values or test inputs. Existing tests were re-run as follows.

| Command (`F:/dev/p4`) | Result | Proof scope |
| --- | --- | --- |
| `cargo test -p p4-llamacpp-staged-adapter --lib v2::scheduler -- --nocapture` | 33 passed / 0 failed / 0 ignored, 524 filtered | local tests of the selector, pipeline, service model and mixed planner |
| `cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy -- --nocapture` | 10 passed / 0 failed / 0 ignored, 547 filtered | real Worker loop, Frame boundary, settlement; fake native |
| `cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::service_budget -- --nocapture` | 2 passed / 0 failed / 0 ignored, 555 filtered | real Worker path for cost feedback, reselection and probe; fake native |

The total is 45 non-overlapping selected tests. It is not a full `cargo test --workspace --no-fail-fast` tally;
the remaining tests, mutations for new improvements, native/GPU real-hardware runs and performance comparisons were not run in this review.
The existing test `profiled_pipeline_actual_loop_spends_only_residual_tokens_on_prefill` is at
[bounded_strategy.rs:6](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/loop_tests/bounded_strategy.rs#L6),
independent D groups are at [line 36](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/loop_tests/bounded_strategy.rs#L36),
and the initial P width is at [line 78](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/loop_tests/bounded_strategy.rs#L78).
Their fake-native passes do not count as proof of physical batching cost on CUDA/Metal.

The raw logs and probe output are kept locally in `target/batching-code-review-20260913/`.
This is not a long-term real-hardware evidence bundle. §7 records the procedure so the core selector comparison can be reproduced
even after target is deleted. Document link/EOL/index verification is recorded in the closing record below.

## 7. Reproducing the selector comparison

This probe uses the public `Scheduler` and the current `PipelinePolicy` source without any source change.
Create a temporary Rust project and use the following manifest. In another checkout, change both absolute paths together.

```toml
[package]
name = "p4-batching-review-probe"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
serde = { version = "=1.0.229", features = ["derive"] }
p4-llamacpp-staged-adapter = { path = "F:/dev/p4/layers/adapters/llamacpp/staged/adapter" }
```

`src/main.rs`:

```rust
#![allow(dead_code)]
use p4_llamacpp_staged_adapter::v2::{Demand, Phase, Scheduler, OrdinaryLimits, SchedulerError};
#[path = "F:/dev/p4/layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/pipeline.rs"]
mod pipeline;
use pipeline::{PipelinePolicy, PipelinePopulation, PhasePopulation};

fn demands(d: usize, p: usize) -> Vec<Demand> {
    (0..d+p).map(|i| Demand {
        request_id: format!("r{i}"), sequence_id: i as u32,
        compatibility: "review-session".into(),
        phase: if i < d { Phase::Decode } else { Phase::Prefill },
        available_rows: if i < d { 1 } else { 4096 }, atomic: false,
    }).collect()
}
fn case(label: &str, total: Option<usize>, pipeline: bool) {
    let input = demands(32, 8);
    let mut limits = OrdinaryLimits::default();
    let mut capacity = 512;
    if pipeline {
        let selected = PipelinePolicy { mixed_batch_rows: total, mixed_prefill_rows: 128 }
            .select(&input, limits, 4, 0, PipelinePopulation {
                decode: PhasePopulation { ready: 32, ..Default::default() },
                prefill: PhasePopulation { ready: 8, ..Default::default() },
                ..Default::default()
            }).unwrap();
        limits = selected.effective_limits;
        if let Some(t) = total { capacity = capacity.min(t); }
    }
    let s = Scheduler::new();
    let plan = s.prepare_plan_with_limits(&input, capacity, 128.min(capacity), false,
        usize::MAX, false, limits).unwrap();
    let rows = |phase| plan.allocations().iter().filter(|a| a.phase == phase).map(|a| a.rows).sum::<usize>();
    println!("{label}: D={} P={} total={} members={} limits={limits:?}", rows(Phase::Decode),
        rows(Phase::Prefill), rows(Phase::Decode)+rows(Phase::Prefill), plan.allocations().len());
}
fn main() {
    case("legacy", None, false);
    case("pipeline-prefill-cap", None, true);
    case("pipeline-total-cap", Some(128), true);
    for (label, limits) in [("legacy-saturated", OrdinaryLimits::default()),
        ("bounded-saturated", OrdinaryLimits { decode_members: 8, prefill_members: 1,
            prefill_rows: 128, prefill_rows_per_request: 0 })] {
        let input = demands(8, 1);
        let mut s = Scheduler::new();
        let mut p = 0;
        let mut d = 0;
        for _ in 0..32 {
            let plan = s.prepare_plan_with_limits(&input, 8, 8, false, usize::MAX, false, limits).unwrap();
            for a in s.commit_plan(plan).unwrap() {
                if a.phase == Phase::Prefill { p += a.rows; } else { d += a.rows; }
            }
        }
        println!("{label}: 32 accepted selections, D={d} P={p}");
    }
}
```

Run command:

```powershell
cargo run --offline --manifest-path target/batching-code-review-20260913/probe/Cargo.toml --target-dir target/batching-code-review-20260913/probe-build
```

This example creates selector approval opportunities directly. In the saturation comparison it re-offers demand every time, so
it does not imitate actual generated content, request termination, KV changes or network behaviour.
The first probe build read `scheduler.rs` directly as an external `#[path]` module, could not find its submodules and
failed with E0583. Only the execution entry point was fixed, by using the public crate dependency and referencing `pipeline.rs` directly.
Product sources were not changed, and this build error is not counted as a batcher defect.

## 8. Closing record

- Code review and selector comparison done. The 45 existing selected tests were re-run with 0 failures; the probe's five results match §4/G1.
- Product code and existing tests were not changed. The new review document was registered in the README and the document map.
- `node tools/scripts/docs-lint.mjs`: 97 Git-tracked documents clean. This does not mean new untracked documents were checked.
- The 97 tracked Markdown files plus the new review and 2 existing separate drafts were copied verbatim into `target/batching-code-review-20260913/docs-snapshot/`
  and the same lint was run with `--all`: 99 clean. The scope of the source/link/EOL checks is stated as this set.
- The original workspace-wide `node tools/scripts/docs-lint.mjs --all` failed with 366 errors out of 465.
  The output consisted of errors asking to register the upstream Markdown in the existing `.cache/llama-pipeline-upstream/` in the document map.
  The cache was not deleted and the lint rules were not relaxed. This full-disk check is not recorded as GREEN.
- For the 33 code/document links in the review document, target file existence and code line ranges were checked.
  The Rust probe body included in the document matched the file actually run. The diff whitespace check of the README/document map passed.
- The source baseline was confirmed by no adapter/server difference between the two HEADs above and no difference in the final working tree.
  Existing document changes from concurrent work were preserved, and this document and index were left in the working tree. No commit/push was done.
- Performance and GPU utilization improvements are not measured. If this leads to an implementation request, first prepare the real consumption counterexamples and
  cost observations for the relevant G item, and link execution order and promotion to the roadmap/verification protocol.

<a id="review-correction"></a>

## 9. Correction from the final integrated-plan review (2026-09-13)

The 45/45 in §6/§8 is the original author's run record. A later re-run on the same source paths gave
scheduler 33/33, bounded_strategy 9/10, service_budget 2/2, i.e. **44 passed / 1 failed / 0 ignored**.
The test re-review baseline is `2ff8cc703511aa23b72c08af61da45c557b4c278`, and the OUTER changes after 484b856ee
did not modify the Rust/native execution path. This correction does not claim that the earlier successful run never happened; it
adds a failure that is reproducible now. We also confirmed that the later OUTER module move in 245d6b785 has no Rust/native difference. This is not a full workspace test result.

The failing test is `v2::node::worker::loop_tests::bounded_strategy::phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input`.
Running it alone also gave exit 101/0 passed/1 failed. The timer-wait authority preservation assertion at `bounded_strategy.rs:283` showed
the differences `next_event 57→58` and `free_sequences []→[1]`. It is not yet determined whether this is a test synchronization issue in which a normal RELEASE
was processed between snapshots, or a real change of authority in the timer. Do not make it pass by deleting the assertion or extending the timeout;
check the event order and the actual consumption path.

```powershell
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy::phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input -- --exact --nocapture
```

- Log: `target/external-slide-review-20260913/review-20260913/phase-pacing-alone.log`, SHA-256 `4d3e2a4979ac853b91998c3832d45603af615188b726d63d9a1a472cadb471ec`.
- Test source SHA-256: `c68a5e55ab9c1cd39908efa5e683db00dc251cd8f8d56e9578693a5a110121d1`.
- Executed binary `target/debug/deps/p4_llamacpp_staged_adapter-9aafc450c15b9e1d.exe`, SHA-256 `b262d1ad70beb632cbfd394a0e9126b63e0743545b65e0e5b6123a5b1632eda6`.
- The selector probe was reproduced, but it is not evidence of real GPU or continuous-service performance. Product/test sources were not changed.

The product adoption/deferral reasons for G1–G6 are linked to [development plan §5](external-analysis-improvement-plan.md#batch-decisions),
and the Release A counterexamples/real-hardware acceptance to the [verification protocol](distributed-batching-verification.md#release-a-contract).
The progress/observation contracts of G1/G3/G4/G6 are kept distinct from the additional state contracts of G2/G5, and this is not to be read as
a plan to build all 6 algorithms. The first regression task for a new session is the timer RED above.

At final document closing, the same standalone test was re-run at 245d6b785 and gave exit 101/0 passed/1 failed.
The additional log is `target/external-slide-review-20260913/review-20260913/phase-pacing-final-review.log`,
SHA-256 `322d563772b3131459750b8d56c7cfc4c116a658c9e687cc03a4f1bc8b38d94d`.
