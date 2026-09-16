# Proposal for accepting P4 Studio's observability requirements

2026-09-15. **This is a design proposal, not a record of implementation, default changes or performance acceptance.**
At the user's request, Studio's observability requirements were organized by additional traffic, inference cost and layer responsibility.
The current development order and resume conditions are owned by the [roadmap](distributed-batching-roadmap.md#current-status),
test and real-hardware approval by the [verification convention](distributed-batching-verification.md),
and responsibility and dependency boundaries by the [isolation contract](layer-isolation-contract.md).
The candidate order in this document is the dependency order within the observability feature, and it does not lift the existing development pause.

## 1. Analysis basis and conclusion

- Baseline used to check the relevant execution paths: P4 `dde0813fb0adbcb43aa6dccb27e9b8a2675b85f2`.
- Inputs: Studio's `F:/dev/p4studio/docs/inference-observability.md` and the implementation/verification reports provided by the user.
  The initial P4 analysis baseline of the Studio document is `434bd97fc1e3e2d73cc5a0976119aa094682fb12`.
  The diff between the two P4 baselines was compared against the current telemetry production, delivery and INSPECT paths.
- Studio deployment, screens and test results are input reports, not results re-run in this P4 review.
- Checking the source paths does not mean the deployed binaries are identical or that the whole repository was re-verified.

The acceptance direction is **always-on aggregates + a small per-request summary + limited detailed diagnostics**.
Adding detailed events per request, token or stage is excluded from the default mode.
Optional delivery of new operational metrics is kept separate from the existing OUTPUT, settlement, release and batch/span evidence obligations.
The figures below are not measured predictions but an initial cost budget proposal to be verified during implementation.

## 2. Facts confirmed on the current paths

| Item | Basis and interpretation |
| --- | --- |
| Per-request output, batch, stage | OUTPUT v5 and BatchObservation/StageSpan v4 in [commands.rs](../layers/adapters/llamacpp/staged/adapter/src/v2/commands.rs), and the per-owner production path in [observe.rs](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/observe.rs). Shared batch time is not summed as request-specific compute time |
| Snapshot just before issue | SchedulingSnapshot provides pending/eligible/outstanding and so on at the moment of selection. ready_rows also includes remaining prompt tokens and is not a count of requests that keep waiting. idle_gated=0 does not mean there are no other blockers |
| forward time | Fixed at PublicationAfter::Observed in [effects.rs](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/effects.rs). It is the time the local completion mailbox accepted the item, not socket send completion or a remote arrival ACK. end→forward is the delay until result delivery is handed off |
| Observation completeness | Follows the per-owner observation and issue evidence completeness of the [verification convention](distributed-batching-verification.md) and the [batching contract](adapter-batching-layers.md). Arbitrarily sampling the existing batch/span can break current consumer verification and delivery obligations |
| INSPECT | [inspection/mod.rs](../entrypoints/agent/src/event_runtime/control/inspection/mod.rs) runs hardware::observe via spawn_blocking on every call. The node/broker snapshot time is not assumed to equal the actual measurement time of the hardware probe that completes later |
| Resource samples | Per-provider support differences in [hardware.rs](../entrypoints/agent/src/event_runtime/control/inspection/hardware.rs) are kept. Unified memory is not double-counted as RAM plus separate VRAM |
| Native cost | With P4_STAGED_TRACE_COST=1, [physical_cost.hpp](../layers/adapters/llamacpp/staged/server/src/runtime/physical_cost.hpp) records parse/match/sample/encode, setup/decode/capture/post and so on. [0029-cost-observation.patch](../layers/adapters/llamacpp/staged/compat/451b89bae/0029-cost-observation.patch) records graph execution and n_kv and adds an explicit backend synchronize. Stays disabled by default |
| Actual transport | encode→write_frame→local retire in [transport.rs](../entrypoints/agent/src/event_runtime/transport.rs). Local write completion is separate from remote acceptance, native completion and KV reclaim, and a failed write is not replayed automatically |

The existing native cost is not a complete decomposition of GPU H2D/compute/D2H. Graph time includes dispatch, internal copies and
synchronization, and overlaps with native decode. The two times are not summed or shown as pure kernel time.
The existing [CPU cost observation](../tests/reports/release-a/20260914_050400.md) and
[550B on/off comparison](../tests/reports/release-a/20260915_011200.md) are reused, but
the latter's E2E +0.851% is a single fixed-order pair, so it is not evidence of causal overhead or of acceptance within 1%.

## 3. Acceptance scope by item

| Item | Always-on acceptance candidate | Optional diagnostic candidate |
| --- | --- | --- |
| Request lifecycle | fixed-size state, timestamps, cumulative wait time; 1 summary at termination | detailed history of every state transition |
| Wait causes | current request count, cumulative time and occurrences per cause | per-request timeline of cause changes |
| KV and engine state | per-stage aggregates of available capacity, usage, reservation and allocation failures | per-request block placement, eviction/reuse detail |
| transport | per-connection/edge cumulative bytes, frames and errors, queue count/bytes, wait-time aggregates | per-execution enqueue/write/receive timestamps |
| Native phases | host-side interval time aggregates obtainable without extra synchronization | H2D/compute/D2H, graph and kernel profiling |
| Clock and collection quality | monotonic duration, sample time, age, drops, errors, schema/build identity | precise clock correction and detailed distributed traces |
| Operational export | an external exporter converts the collected aggregates | request trace/exemplar linking |

### 3.1 Request lifecycle and waiting

The adapter keeps, in a small per-request record, the first acceptance, eligible, queue entry, issue, output and termination timestamps, the cumulative
wait time per cause, and the current cause with its entry time. The shared transport's receipt time is kept distinct from adapter admission.
native-start is provided only when observed at the actual native boundary, and is not substituted with the Worker's RPC start.
Requests can terminate midway through cancellation, rejection or failure, so a single linear lifecycle is not forced on every request.

- Wait time accumulates when the state changes. Do not re-walk all requests/history on each poll or re-run the scheduler for diagnostics.
- Causes are split into capacity, outstanding, input dependency, coalescing, downstream backpressure and unknown.
  Only causes observed by the actual owner are recorded; the detailed enum and linearization points are fixed as a contract before implementation.
- When several blockers overlap, a primary cause is distinguished from secondary causes. The primary-cause selection rule is fixed, and overlapping time is not summed into E2E.
- A node's no-input state is distinguished from a specific request's wait. Time whose cause cannot be classified is left as unknown.
- The server's first-output ready/approval time is distinguished from the client's first-receipt time. User TTFT is the actual client send → first OUTPUT receipt.
- The termination summary is a new operational observation. A delivery failure or drop lowers coverage and does not re-judge OUTPUT or settlement as failed/succeeded.
  Both the active store and the pending-completion summary store have count/byte limits, and unbounded history retention is forbidden.

### 3.2 KV and engine state

Not every backend is forced into total/used/free blocks. The adapter exposes the differences between llama.cpp/HF and attention/recurrent
as state kind, unit (bytes/cells/blocks), measurement time and support status.

- Actual allocation, logical usage and reservation are kept separate. n_kv is the KV range the graph uses, not total cache usage.
- eviction/reuse/preemption/recompute/spill are accumulated only where there is an actual implementation and success point.
- Unsupported is null/unsupported; supported but not occurred is 0. A broker receipt is not GPU KV.
- Use existing counters/caches at safe owner boundaries. The default mode does not walk KV contents, copy from the GPU, or force native queries during execution.
- Shared memory with no actual per-request attribution is not divided up per request.

### 3.3 Transport and clocks

Counters are added at the existing encoding and I/O points. Payloads are not re-encoded or copied just for instrumentation.

- Distinguish payload bytes from bytes including framing, send attempts from actual local writes, and complete receipts from decode failures.
- If the actual bytes of a partial write/read failure are unknown, leave them as unknown. Do not count the planned frame length as the amount actually sent.
- Distinguish queue count/bytes, and local queue retries from network retransmission. Instrumentation does not change the meaning of retry or replay.
- The display unit is the P4 application transfer rate. Total NIC usage including TCP retransmission and headers is measured separately.
- Intervals on the same host are recorded as monotonic durations. Monotonic timestamps from different hosts are never subtracted directly.
- Provide the wall-clock anchor, clock source, and the measurement basis for offset/uncertainty together with the sample age.
  RTT alone does not establish an exact one-way delay. Without a basis, it is unknown.
- Acceptance of detailed cross-host overlap follows the clock/coverage conditions of the existing verification convention. Negative delays are not clamped to 0 to make them pass.

### 3.4 Collection quality

emitted/dropped/sampled/overwritten/export-failed/parse-failed, the time of the last successful sample, the actual collection interval and
the collection duration are kept separate per producer, forwarder and consumer. Sampled and unintended loss are not merged.
Counters are bound to process/epoch and reset information, and restart differences are not shown as negative rates or spikes.
Clock and collection quality are included in the first bundle. An empty graph is not presumed to mean no load.

## 4. Collection and delivery structure, and cost budget

1. Each agent has exactly one collector. The Studio backend distributes the data it receives to browsers, avoiding per-browser agent polling.
2. Rarely changing capabilities are separated from runtime occupancy. The default candidate is 1-second aggregation while an operations screen is active and 10 seconds otherwise.
3. INSPECT reads the cache. A slow probe is not run twice concurrently; the previous sample is returned with sample_age/probe status.
   Per-provider timeouts and retry intervals after failure are limited. This interval does not automatically relax the real-hardware evidence collection rules.
4. Cumulative counters are sent. After missing intermediate snapshots the total increase can be recovered, but momentary peaks and time distributions are not shown as recovered.
5. New detailed traces are disabled by default. When enabled, the target, duration, sampling rule, count/bytes and send volume are fixed.
   The linking rules for the selected request/execution are kept, and a partial trace is never marked complete.
6. The current native cost flag is a static in-process setting. It is not promised as an instant on/off switch and is applied at a safe LOAD boundary of a separate measurement arm.

### Initial default-mode budget

| Item | Proposed target |
| --- | --- |
| Additional telemetry send volume | at most 16 KiB/s per agent, including envelope/framing |
| Additional traffic under load | at most 0.5% of existing non-observability P4 traffic on each shared link |
| Low traffic and idle | a 10-second interval and an absolute byte cap instead of a ratio |
| Useful generation TPS drop | within 1% |
| TTFT and ITL p95 degradation | each within 2%, and existing SLOs still met |
| Memory | fixed count/byte budget by number of requests, stages and edges; no growth proportional to run time |

Whichever of the two traffic limits is reached first applies. The total of new snapshots, request summaries and optional traces is capped,
and the cap does not grow automatically because there are many devices, nodes or edges. If it is insufficient, the detail level is lowered and reflected in coverage.
Before implementation, the profile seals the load/low-traffic distinction, measurement window, burst capacity and integer memory caps.
If a target is smaller than the noise and cannot be discriminated, it is left undecided rather than PASS.

Illustrative calculation: 8 stages × 100 executions per second × 1 KiB detailed events is 800 KiB/s for a single delivery alone.
A 1-second aggregate of 4 KiB per stage is 32 KiB/s. This compares representations of additional metrics; it is neither a replacement for the existing batch/span nor a measured saving.

Full traffic reports separate the following.

- Bytes and CPU/RSS of existing payload/control/output, existing mandatory observations, new operational observations, and probe/export.
- Sends and fan-out incurred at each actual edge/return hop. Sends and receives on the same link are not double-counted.
- agent→Studio and Studio→browser transfer. The total cost as a function of browser count is not hidden.
- The cost of termination summaries, which scales with the request arrival rate, not only with the generation rate. Concurrency 50 is not taken as 50 requests per second.

Reducing data after Studio receives it and before storing it does not reduce P4 send traffic.
If the existing mandatory observations are themselves expensive, consider a separately versioned evidence contract and compressed/bundled delivery, without bypassing current verification.

## 5. Observation saturation and responsibility boundaries

On the hot path, only small counters and timestamps are updated, and serialization and export are separated. Extra GPU synchronization, payload copies,
full-history scans and waiting on exporter responses are forbidden in the default mode.

- New snapshots may be merged into the latest value, and optional traces may skip generation before exceeding the budget.
- Skips, overwrites and delivery failures are made visible. Loss of optional observations is not interpreted as inference success or settlement success.
- Existing OUTPUT, settlement, release and mandatory batch/span are never discarded because of the observation budget.
- Events that already carry a delivery obligation follow the existing retention contract. New observation buffers and send budgets are kept separate from existing inference reservations.
- Putting more telemetry into the current queue and calling it low priority does not by itself establish isolation.
  Even with collector disconnect, slow consumption or observation buffer saturation, retention and normal progress of requests, ledger, reservations, credit and output are verified on the real path.

| Owner | Scope |
| --- | --- |
| P4 shared | transport, broker, node delivery state, clocks, collection quality |
| Concrete adapter | request acceptance/issue lifecycle, scheduler waits, KV, native semantics |
| Studio | client TTFT/E2E/ITL, SLO, long-term retention, charts, per-request joining |
| Exporter | Prometheus/OTel conversion |

The shared layer does not parse adapter state strings to decide queued/KV full.
If promotion to a shared schema/trait is needed, verify the independent semantics and cost with a mock or a second adapter that does not know llama.
New fields are not unconditionally inserted into the strict schema of the current batch/span. A new content-type or negotiated version,
producer/consumer fixtures, and unsupported handling for old producers are designed together.

Prometheus labels do not include request/session/execution IDs. node/stage/reason also limit the active set and
series count, and use histograms that can be aggregated. Request IDs are linked through trace/history/exemplar.
For guidance, see [Prometheus instrumentation](https://prometheus.io/docs/practices/instrumentation/).

## 6. Candidate implementation bundles and verification

This section is a plan that has not been run. How actual work relates to the remaining Release A work is decided in the roadmap.

1. INSPECT cache, prevention of duplicate probes, sample time/quality.
2. transport bytes, queue and error aggregates.
3. Adapter wait-cause aggregates and a small per-request summary.
4. Aggregates of KV/engine state that can be read safely.
5. Later optional features: limited request traces, native phase decomposition, Prometheus/OTel exporter.

Before implementation, fix the failure counterexamples, actual consumption points, normal-progress oracle and independent removal mutations.
Required regressions include duplicate/out-of-order/restart, partial send/receive failures, probe timeout, multiple browsers,
collector disconnect, slow consumption, observation count/byte boundary ±1, and state retention after cancellation or rejection.
They must detect fake wait times updated only on poll, counters that count planned bytes as actually sent, silent loss of mandatory observations,
and changes where an observation failure causes a native re-run.

Performance comparison compares additional instrumentation off/on with the same source, binaries, model, topology, policy and workload.
Existing mandatory observations stay on in both arms. Include short decode, long prefill and mixed continuous waves, and
report order, thermal and cache effects and confidence intervals. The repetition count and the final multi-computer verdict follow the verification convention.
Record together the token/time denominator of useful TPS, the actual client send time, normal responses, termination and reclaim, the GPU analysis window,
telemetry coverage, and the first error and cleanup errors. Do not approve an improvement rate against a failed baseline.

## 7. Verification boundary at archive time and next action

The deliverables of this work are the proposal document and the README/document map index entries. Runtime, wire, defaults and deployment were not changed.
Document format/index checks are not verification of implementation semantics or performance, nor a re-run of the full cargo/native/real-hardware tests.

The first action for follow-up development is to re-check HEAD/dirty and the roadmap pause conditions, then fix the observation profile, buffer/byte budgets,
schema/owning layer and actual consumption tests. Promotion to an always-on default is judged only after meeting the cost budget above and the existing safety and
real-hardware acceptance. At this stage, no claim is made that 16 KiB/s, 0.5%, 1% or 2% has been achieved.
