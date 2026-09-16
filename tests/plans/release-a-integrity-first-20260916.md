# Test Plan: Release A Qwen122B integrity before performance

## Created

2026-09-16 KST.

## Goal

Prove that the current Qwen3.5-122B-A10B product source is a complete service before changing a
batching or GPU policy. The service must answer normal short, medium, and long requests; converge
under bounded continuous arrivals; reject overload without side effects; preserve cancellation and
failure ownership; recover on the same load; and release every owned resource. This gate records a
performance baseline but does not claim an improvement.

The executable test contract is
`test/benchmarks/cluster-inference/release-a/integrity-test-spec-qwen122b-i0-v2.json`. The contract
validator and the artifact judge must pass their own baseline and weakening mutations before any
model is loaded.

This suite is a proof procedure, not an algorithm-discovery loop. Product behavior must already be
defined as a deterministic state machine for every admitted, rejected, canceled, failed, stale, and
released request. Tests use fixed identities, fixed schedules, fixed fault boundaries, and a sealed
seed to apply severe load and exceptions. Runtime random recovery, timing-dependent retry, or a
different terminal classification on exact replay fails the arm.

## Environment

- P4 runtime source and every binary/library hash are bound by the next H0 revision; no arm may
  replace them after the first LOAD.
- H0 v4 binds runtime `19f2b1afaa5c4243a59b8bc1edb76d9b82a01d6b`, including the absolute
  inference-window anchor, to freshly built remote binaries and the raw-evidence builder.
- I0 uses sealed 15-second loaded, inference-window, and drained barriers. Missing or repeated
  barriers invalidate the run; resource snapshots are captured only while a barrier owns the state.
- Useful generation TPS counts model tokens forwarded as the accepted service response. An OUTER
  processor's newly calculated response gives the discarded model tokens zero useful count while
  retaining their nonzero total generation count. The request evidence binds this provenance.
- Model: Qwen3.5-122B-A10B UD-Q5_K_S, cuts `[0,24)`, `[24,36)`, `[36,48)` on Spark GB10,
  Mac20 M4 Pro, and Mac21 M4 Pro.
- Shape: resident 8, context 102,400 per sequence, total context 819,200, batch 128, ubatch 64,
  F16 unified KV, maximum output 2,048 tokens.
- The local Windows desktop performs control, static validation, and evidence collection only. It
  does not build a product binary or run model inference.
- Task agents and native listeners use ports outside every participating OS dynamic range. Existing
  protected agents on port 52005 remain alive and unchanged.

## Preconditions

The first sealed I0 execution is RED: three actual distributed requests completed and released, but
only the short answer matched the strict oracle. A corrected GGUF no-thinking template still passed
only 1/3 on an independent single-host server. The source and input have changed since H0 v5;
therefore that historical seal cannot authorize a new LOAD. Before H0 v6 or another distributed
I0 run, establish a product-owned deterministic calculation/validation authority and run
`judge-reference-capability.py` on the exact sealed short/medium/long prompts. It must return 3/3
with exact source-derived JSON, prompt identity, evaluated-token count and EOS. This is an
admission gate, not an iterative prompt search. Preserve the failing response hashes and the
strict oracle; do not adapt the expected outputs to the model's guesses.

1. `validate-integrity-test-spec.py --self-test` passes the canonical contract and every weakening
   mutation. The actual contract then passes the same validator.
2. The integrity artifact judge passes its baseline fixture and rejects, independently, missing
   response, wrong oracle, non-EOS, missing RELEASE, deadline/SLO excess, missing batch evidence,
   missing GPU coverage, nonzero backlog, unclassified overload, post-cancel output, stale return
   acceptance, and incomplete cleanup.
3. Working bytes, HEAD blobs, staged blobs, and a fresh checkout match every sealed source, corpus,
   plan, materializer, judge, and validator hash.
4. Each advertised agent address completes the real bidirectional INSPECT round trip. Before the
   task agents start, task children/listeners are 0. After they start and before LOAD, every host has
   nodes 0, native children 0, exactly one task-agent listener, transport failures 0, and CLOSE_WAIT
   0. `inspect-i0-active-host.py` also binds the task PID, command, binary hash, advertised address,
   and exact topology-owned ESTABLISHED peers. SSH or a one-way socket probe is not a substitute.
   The controller verifies all four task SSH tunnel PIDs and command lines immediately before the
   advertised-route probe. A route failure preserves the transport snapshot; the same agent instance
   cannot enter LOAD because peer failure is deliberately sticky until explicit reconciliation.
5. Source, binary, native library, model shard, tokenizer/template, corpus, stage plan, device,
   generation, resource profile, telemetry sampler, and judge identities equal the seal.
6. The event runtime owns bounded deadline to artifact assembly, FINISH, and cleanup. External TERM
   is only an INVALID recovery path and can never make an integrity arm pass.

## Test Cases

### I0 — Current-source single-request service

Run one LOAD and submit three requests sequentially on that unchanged load. The machine-readable
`execution_groups.I0` authority fixes `load_count=1`, `unload_count=1`, the exact case order,
`max_in_flight=1`, deadline vector, and 3,900,000 ms group timeout.

| ID | Corpus input | Submission | Deadline | TTFT p95 | ITL p95 | Required result |
| --- | --- | --- | ---: | ---: | ---: | --- |
| I0-S | exact `case-00` short | next after RELEASE | 600,000 ms | 60,000 ms | 250 ms | exact JSON oracle, EOS, delivered/completed/released 1/1/1 |
| I0-M | exact first medium case | next after RELEASE | 1,200,000 ms | 300,000 ms | 250 ms | exact JSON oracle, EOS, delivered/completed/released 1/1/1 |
| I0-L | exact first long case | next after RELEASE | 1,800,000 ms | 900,000 ms | 250 ms | exact JSON oracle, EOS, delivered/completed/released 1/1/1 |

The arm fails on the first nonterminal request. It still writes the partial artifact and performs
bounded FINISH and NODE_UNLOAD. Passing only I0-S is not product integrity.

### I1 — Complete normal corpus on one load

`I1-Q64` submits all 64 sealed corpus cases in corpus order with `max_in_flight=1`. A new request is
submitted only after the preceding terminal RELEASE. Per-class deadlines are 600,000/1,200,000/
1,800,000 ms and the total bound is their exact sum plus 300,000 ms cleanup grace.

Before LOAD, H0 v8 must pass `verify-h0-v8.py` and the sealed I1 config must prove 64 distinct cases
(short32/medium16/long16), exact prompt/JSON oracle/processor/deadline bindings, 15,000 ms holds at
LOADED, INFERENCE_WINDOW, and DRAINED, a driver timeout beyond the run bound, and a 900 s artifact
transfer bound. The remote collector checks all advertised round trips, exact task tunnel PIDs and
17 source/config/binary hashes. This is deterministic lesson L086; changed source also requires a
fresh I0 pass before I1 starts.
The Windows controller tunnel manifest is UTF-8 without BOM and its four exact PIDs/commands pass
`cleanup-i0-owned.py --inspect-only` before the route check; lesson L087 blocks a malformed manifest
before LOAD.

All 64 requests must be delivered, oracle-correct, EOS, completed, and released. Incomplete,
unreleased, uncertain, unsubmitted, runtime error, evidence missing, and cleanup error are all zero.
The run stays on one model load and may not restart an agent to make progress.

### I2 — Bounded overlapping service

| ID | Exact arrival schedule | Required convergence |
| --- | --- | --- |
| I2-COLD8 | cases 0–7 together at 0 ms | all 8 terminal and released within 1,800,000 ms |
| I2-WAVE64 | eight groups of 8 at 0, 180,000, 480,000, 780,000, 1,080,000, 1,380,000, 1,680,000, and 1,980,000 ms | all 64 terminal and released by 3,780,000 ms; actual send slip <=1,000 ms |
| I2-RECOVERY24 | three blocks of 8, 30,000 ms quiescence after each drained block, same load | every block returns active/pending/flight/frontier to zero and the following block succeeds |

I2-WAVE64 must show at least one later wave's first token before the preceding wave's last terminal
and must show overlapping prefill/decode active intervals beyond the 10 ms clock-error allowance.
Pending work without token progress is not overlap. After the last arrival, queue depth must decrease
to zero without a restart.

### I3 — Overload, cancellation, and failure ownership

- `I3-OVERLOAD80`: submit 80 unique request IDs within 1,000 ms. At most 72 may be admitted and at
  least 8 must receive an explicit rejection within 5,000 ms. Every admitted request reaches a
  declared terminal and RELEASE. Rejected requests change no ledger, reservation, credit, KV,
  output, or native effect. There are no missing or unclassified requests.
- `I3-CANCEL`: submit 8, cancel two exact request IDs after their first token. Output accepted before
  the cancel linearization remains; no output or new native issue occurs afterward. The other six
  requests complete normally. All eight release their ownership.
- `I3-SLOW`: add exactly 1,000 ms edge delay to 8 requests. All eight either meet the unchanged
  normal deadline and oracle or the arm fails; the delay is not removed after launch.
- `I3-DISCONNECT`: disconnect one exact edge after its first output. The target reaches one explicit
  failed/canceled/uncertain terminal, the other seven remain correct, and the failure ledger retains
  the undelivered effect until exact reconciliation.
- `I3-RESTART`: restart the declared middle stage after the first output of one exact target. Every
  request reaches a declared terminal, non-target requests are not silently lost, old-generation
  returns cannot mutate the new generation, and the same load or its explicit failed generation is
  fully reclaimed before reload.
- `I3-LATE`: delay one exact old-generation result by 30,000 ms. The stale result is rejected without
  ledger/KV/output mutation and all current-generation requests remain correct.

### I4 — Sustained integrity and final cleanup

`I4-SOAK` runs 32 waves of 8 on the same load, one wave every 180,000 ms: 256 requests and a minimum
duration of 5,580,000 ms. The arm continues until both minimum duration and all terminals are met,
with a total bound of 7,380,000 ms. Normal responses and terminals pass 100%; unanswered, lost, or
cross-session results are zero. RSS/VRAM, retained bytes, ledger, queue, and credit return to their
declared baseline range after drain.

At the end, send NODE_UNLOAD exactly once per node. Each must return `succeeded/absent`. INSPECT must
show nodes 0; task-owned native children and listeners must be 0; GPU compute processes must be 0;
and transport failures must be either 0 or preserved with exact reconciliation status. Killing the
agent is not cleanup success.

## Performance Evidence Required From Every Arm

The following scorecard is mandatory even though this phase does not claim improvement:

- request count by delivered/completed/released/rejected/failed/uncertain/unsubmitted;
- per request: eligibility, send start/completion, arrival, first output, every output receipt,
  terminal, RELEASE, input tokens, generated tokens, stop reason, oracle result, and measured
  queue/prefill/decode values;
- run: useful generation tokens divided by request arrival through terminal, release-inclusive total
  generation TPS, prefill input rows/s, decode evaluation rows/s, TTFT and ITL distributions;
- head scheduler per phase: physical batch count, rows mean/p50/p95/max, full-ubatch fraction, mixed
  count, runnable/eligible/blocked/pending, block reason, flight/open peak, and idle interval;
- per stage: span and execution counts, rows, queue/compute/publish time, compute p50/p95, open peak,
  and overlap time. Head-only scheduling evidence is never copied and relabeled as another stage;
- per host in the same analysis window: sample count and coverage, GPU/device utilization
  mean/p50/p90/zero fraction, memory peak, power and temperature when available; unavailable fields
  carry an explicit reason and are never written as zero;
- every raw GPU sample carries its remote UNIX capture time. The evidence builder independently
  binds it to the event artifact's absolute start and 1,000 ms grid within 750 ms, and independently
  rejects coverage below 95%; relative counters without the absolute anchor are invalid;
- stage queue/compute/sample/copy/network/settle time, link bytes, clock-error bound, KV and every
  count/byte/token reservation before load, at peak, after drain, and after unload.

Missing scorecard fields make the arm INVALID. Batch fill or GPU utilization alone cannot make an
arm pass. The I0–I4 scorecard becomes the immutable P0 baseline only after every integrity arm passes.

## Expected Results

- I0 through I4 all pass on one sealed current source and evidence schema.
- Normal requests are oracle-correct, EOS, terminal, and released at 100%.
- Overload and fault requests have complete, predeclared outcomes with no unclassified remainder.
- No arm needs an agent restart for normal progress; final cleanup succeeds through NODE_UNLOAD.
- The report states `integrity_baseline=GREEN` and `performance_improvement_claimed=false`.

## Logs To Capture

- Immutable spec/config/judge/source/binary/library/model/corpus/plan hashes.
- Pre/post INSPECT envelopes, process/listener/GPU snapshots, PID/argv, protected-agent status.
- Full event artifact and scorecard, every response and oracle decision, all agent/native logs.
- LOAD/UNLOAD terminals, transport-failure ledger, cleanup evidence, first error, evidence missing,
  and cleanup error as separate fields.

## Uncovered Risk

Passing integrity proves a functioning bounded service on the sealed three-host hardware. It does
not prove that throughput or GPU use improved over an earlier version. Performance work starts only
after this baseline and must compare the same single-request and overlapping workloads before and
after one isolated change.
