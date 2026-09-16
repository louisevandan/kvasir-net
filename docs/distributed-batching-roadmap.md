# Distributed Batching for Very Large Models — Current Status and Execution Roadmap

Latest status summary: 2026-09-16 — The 3 current H0 v8/I0 single requests for Qwen122B passed exact JSON, EOS, RELEASE, time limits and cleanup on 3 real physical hosts. This acceptance is limited to the source-data-based `engineering_power_v1` OUTER service contract. The raw model still gets medium- and long-form arithmetic and format wrong, and the useful generation TPS of discarded model tokens is 0. I1 full corpus, I2 sustained ingress, I3 faults and I4 soak are still not run. In the past, H1 round 1 completed only 8 of 64 requests, and H1 round 2 was INVALID because it had no terminal artifact. The execution order is **establish service integrity → seal the baseline → improve performance**. Performance candidate development and H5 do not start until I1–I4 are all GREEN. Builds and model runs on this PC are blocked; only remote hosts are used.
This file is the **sole owner of the current goal, status, work order and phase promotion**.
Test details and real-hardware verdicts are owned by the [verification protocol](distributed-batching-verification.md), per-layer responsibility and update isolation by the
[isolation contract](layer-isolation-contract.md), and the roles of existing documents by the
[document map](document-map.md). The initial document migration and the follow-up implementation are kept separate.
**For current status and resume conditions, only §0 below takes precedence.** §3 is the initial audit; the long follow-up records in §8 are chronological history.
A "next" inside an earlier record was the plan at that time, not an instruction to continue that implementation now.

<a id="current-status"></a>

**2026-09-16 integrity-first realignment — current execution order:** B0–B5 and node lifecycle M0–M4 are necessary
conditions for execution safety, but they do not count as completed stages of the current product service. The current H0 v8/I0 single requests are accepted, but
the last real-hardware run of continuous requests did not converge, with a completion rate of 12.5% (8/64). Therefore I0–I4 below are the sole current execution order.
The planned H1 round 3 repeat is cancelled. The new I phases, which changed the contract, preflight and judge, are reviewed independently, and whenever the final
source of a phase changes, execution restarts from the earlier I phase. The model is loaded only after the [integrity-first test plan](../tests/plans/release-a-integrity-first-20260916.md),
the [execution contract](../test/benchmarks/cluster-inference/release-a/integrity-test-spec-qwen122b-i0-v2.json),
the contract checker and the result judge are accepted.

**2026-09-16 H0 v8/I0 final-source re-acceptance:** In the [real-hardware report](../tests/reports/release-a/20260916_213700.md),
within one LOAD across Spark→Mac20→Mac21, the 3 short/medium/long requests again passed exact service JSON, EOS, deadline, RELEASE,
unload and task resource cleanup. The inference window was `1,165,743 ms`, inter-host transfer was `3,530,395,656 bytes`,
and errors, missing evidence and cleanup errors were 0. Useful model generation TPS is 4.223 for short, and 0 for medium/long, where OUTER
discarded the raw model output. H0 v8's 21 non-model checks and the byte-identical regeneration of the 64 I1 config/seal files also
passed. **I1–I4 have not been measured yet, so overall `integrity_baseline=false`.** The first next action is to prove the pre-LOAD round trip and owned resources at 0
for a new task agent and return tunnel, and then run I1-Q64 within one LOAD.

**2026-09-16 I0 v7 historical acceptance, I1 pre-run contract hardening:** In the [re-run report](../tests/reports/release-a/20260916_204300.md),
within one LOAD across Spark→Mac20→Mac21, all 3 short/medium/long requests passed source-data-based valid JSON, EOS, RELEASE and
deadline. Distributed transfer was 3,530,395,645 bytes, and final task nodes/native/listener and
transport failures were 0. TTFT, prefill rows/s, useful/total model generation TPS, batch and GPU samples from the same inference window were also
preserved. The useful TPS of the discarded medium/long model tokens is 0; the useful TPS values of 0.884/0.249 in the earlier v6 report were wrong and have been corrected. The raw model's wrong medium- and long-form answers stay in `model_response`, and OUTER's bounded
`engineering_power_v1` computation produces the service response. I1–I4 are still not run, so overall
`integrity_baseline=false`. The 64 I1 corpus items were statically checked against the source-data oracle 64/64, a total of 2,201,802 input tokens, 7,159,035 prompt bytes and the request budgets. The missing LOADED and DRAINED observation barriers in the earlier I1 generator were blocked in H0 v8 below. Because the final source changed, verification restarts from I0.

**2026-09-16 H0 v8 non-model gate GREEN, awaiting I0 re-verification:** The source bundle and file SHAs in the [pre-run contract report](../tests/reports/release-a/20260916_210000.md) bind the I1 64-request runner, raw evidence and judge, three 15-second observation barriers, and a 900-second artifact transfer limit. H0 v8's 21 fixed checks and the 64-request full-path counterexamples passed. This is only contract approval before model LOAD, not I1 real-hardware acceptance. The next action is to confirm a new task agent, the advertised return round trip and owned resources at 0 on the same 3 physical hosts, re-run I0-S/M/L with the changed final source, and only then run I1-Q64 within one LOAD. I1–I4 not run and `integrity_baseline=false` remain in effect.

**2026-09-16 I0 first real-hardware run RED — new distributed runs blocked:** The [I0 first run and single-host cross-check](../tests/reports/release-a/20260916_161600.md)
observed actual completion, EOS, RELEASE, normal cleanup and SLO for all three requests, but only short, 1/3, was correct.
medium/long were also wrong on an independent standalone llama-server, and after the corpus was corrected with the GGUF canonical no-thinking suffix
and re-verified, the result was still 1/3. The suffix mismatch is a real defect, but the hypothesis that it is a sufficient cause of the wrong answers
was refuted. The new capability gate checks the original prompt, token count, EOS and exact JSON/oracle, and
automatically blocks distributed I0 round 2 with medium `oracle_mismatch` and long `invalid_json`.
**The earlier seal of the current H0 v5 does not approve the corrected corpus/judge.** I0 status is RED,
and I1–I4/P0–P3 are BLOCKED. The first next action is to first design and implement a product algorithm/output verification contract that does not leave
authority over the required arithmetic and factual judgement to model guesses, and to prove single-host 3/3 and removal mutations against the same strict
oracle. After that, a new H0 seals source/input/judge.

**2026-09-16 I0 wrong-answer cause isolation:** In the remote single load with 6 fixed probes from the [pre-sealed plan](../tests/plans/release-a-quality-cause-20260916.md) and
[run report](../tests/reports/release-a/20260916_172000.md), both
medium/long output the selected facts exactly 3/3, but the power calculation combined with the full records scored
1/3 and 0/3 respectively. Even with 382/381-token calculation inputs that kept only the three selected records, they scored
0/3 each. All 6 wrong power values from the original were used unchanged in the energy multiple.
Therefore a **model-generated arithmetic error** that cannot be explained by long-context retrieval or distributed transfer alone was independently reproduced, and
the service exposure boundary is that no semantic verification authority existed when that output was delivered as a normal EOS. The code fence in long is
a separate format error. The cause of the number selection inside the neural network is not speculated on. The first next action is to design, outside generic
P4 transport, the extraction identity, deterministic calculation/verification and mismatch terminal
contract for tasks that require exact arithmetic, and to add removal mutations. I0 RED and the block on new distributed LOADs remain.

**2026-09-16 OUTER wrong-answer verdict boundary hardening:** The per-request `expected_json` from the [fix and verification report](../tests/reports/release-a/20260916_180500.md) is bound to the H1/I0 configuration. When the model produces wrong arithmetic, the run artifact is also judged a failure, and the raw output is preserved. This fixes a missing verdict; it does not fix Qwen122B's arithmetic, and it does not make the product block responses before verification. Therefore I0 RED and the block on new distributed LOADs remain. Exact-response acceptance is not promoted until the OUTER caller's authority over source data, targets and calculation/verification, and the actual consumption path, are implemented and proven.

**2026-09-16 H0 v4 seal:** The [H0 v4 spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v4.json)
binds runtime `19f2b1afa`, the new agent/event-drive binaries of the three remote hosts, native/library/model/layout,
the I0 materializer, the raw evidence builder and the two-stage judge. The gap that allowed relative-time GPU samples to be attached to a different run
was closed with `started_unix_ms` and the remote capture time, and fixed verification including contract22, builder10, I0 judge11, full
judge27, active-host preflight4 and H0 spec4 passed. `load_authorized=true`, but because this is the state before the model runs,
`runtime_acceptance=false` and `integrity_baseline=false`. The next action is to confirm the new task agent's actual bidirectional
INSPECT and the exact pre-LOAD state, and then run I0-S/M/L once within one LOAD. v5 places a 15-second observation barrier after the start of the inference window and another after drain, and does not accept process/GPU state guessed outside the barriers as evidence. The remote runner collects state before LOAD, at peak, after drain and after UNLOAD, plus transport bytes, and reclaims only the exact task-owned processes.

| Stage | Status | Exit condition |
| --- | --- | --- |
| I0 current single-request baseline | **GREEN (H0 v8, correct answers 3/3)** | With the sealed source/binary/model/topology, complete short, medium and long within one LOAD with valid JSON/EOS, deadline and RELEASE. Preserve per-request TTFT, prefill rows/s, useful/total generation token/s, E2E, per-phase batch width and per-host GPU samples in the same absolute time window. After task agent start and before LOAD: nodes/native0, agent listener1/host; after final shutdown: nodes/child/listener0. medium/long useful model token TPS is 0 |
| I1 full normal corpus | TODO | In the same load, all 64 closed-loop corpus requests are correct, with EOS, deadline and RELEASE. Errors, unclassified outcomes and restarts are 0 |
| I2 bounded sustained service | TODO | resident8 cold8, 8×8 sustained, same-load recovery3×8. Normal requests 100%; no-response, loss and session contamination 0; backlog converges to 0 within finite time |
| I3 overload, cancellation, faults | TODO | overload80 explicitly rejects requests beyond the limit; terminal states for cancellation, slow/broken edges, mid-stage restart and late returns, with reclaim of ledger/KV/credit/output authority |
| I4 integrity soak | TODO | 32×8, normal and fault arms of at least 93 minutes. Normal responses 100%, predefined terminals 100%, and after shutdown nodes/child/listener/ledger/queue/RSS/VRAM at the baseline state. At this point `integrity_baseline=GREEN` |
| P0 baseline seal | BLOCKED(I4) | Seal the single, sustained and overload scorecards of the final I0–I4 source as the performance baseline. Numbers from past models, other topologies or failed runs are not used as comparison baselines |
| P1 per-bottleneck candidates | BLOCKED(P0) | Change only one cause among queue/tokenize/prefill/decode/sample/copy/network/settle that instrumentation has proven. Run the same single request and sustained wave before and after every change |
| P2 performance promotion | BLOCKED(P1) | H5 paired 8 pairs and holdout 4 pairs, median useful TPS improvement ≥5% with 95% CI lower bound >0, absolute SLO and TTFT/ITL non-regression, quality and resource integrity maintained |
| P3 final re-acceptance | BLOCKED(P2) | The selected candidate passes I0–I4 and the applicable scope of H1–H7 again, and a reproducible bundle, operational defaults and the unaccepted scope are published |

I0 is not a simple smoke test. The first run itself is the current product verdict, and if it fails, current service integrity is confirmed
RED. The same run is not repeated; the bottleneck and exit boundary identified from the artifact are fixed in code and preflight.
I0–I4 also collect the full performance scorecard, but no gain is claimed. Its purpose is to build a comparable
baseline for the P phases. A phase that passed only safety, documentation or unit tests is recorded as `enabling` and is not counted toward product or
performance progress.

I-phase tests are not an exploratory procedure for discovering algorithms. Product code first completes every request, resource and failure transition
as a deterministic state machine, and the tests rigorously prove those invariants with fixed IDs, fixed schedules and fixed injection
boundaries. Random recovery, timing-dependent retries and terminal classifications that differ between re-runs are not accepted.

**2026-09-16 H1 round 2 INVALID and H0 v3:** In the [run and cleanup report](../tests/reports/release-a/20260916_104300.md),
the 3-host closed-loop run performed actual stage computation for 41 min 13 s but had no terminal artifact, and an in-run
audit found that the H0 v2 judge did not evaluate per-request E2E deadlines or per-class TTFT/ITL p95.
Without changing the measurement source or judge mid-run, the exact PIDs were terminated. NODE_UNLOAD 3/3 returned
succeeded/absent on the first request, all task agents/native/proxy/tunnel were reclaimed, and the 3 protected agents were preserved.
H0 v3 keeps the same runtime/model/topology/SLO while enforcing the figures above with real timestamps, and passed check6,
host inspector4, preflight5, materializer4, judge10 and spec4. The H1 rounds are round 1 RED and round 2 INVALID.
The "H1 round 3" in this record will not be run. The A-COST draft is absorbed into the short arm of I0 above, and medium, long,
performance instrumentation and normal UNLOAD are closed out on the same current-source baseline.

**2026-09-16 Qwen122B H0 v2 re-seal:** The [H0 v2 spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v2.json) and
[report](../tests/reports/release-a/20260916_093739.md) bind runtime source `25edd33cf`, the 3-host binary/model/layout, and
the H1 materializer/judge. H1 quality uses `max_in_flight=1`, which submits the next request after the terminal RELEASE,
and fixes the deadlines for short32/medium16/long16 and an overall cap of 67,500,000ms.
H0 check4, host inspector4, preflight5, materializer2, judge4 and 40 weakening mutations passed. The existing protected agents
hold no model child/GPU, and Mac20/21 have nodes0, but the old Spark agent could not be INSPECTed because of CLOSE-WAIT310 and a full backlog,
so it is not used for H1. `load_authorized=true`, `runtime_acceptance=false`. The first next
action is to confirm the new task agent's bidirectional INSPECT, nodes/failure/child/listener/CLOSE_WAIT0 and the per-topology
ESTABLISHED peers, and then
run only H1 round 2.

**2026-09-16 H1 round 1 RED and corrective implementation:** In the [run report](../tests/reports/release-a/20260916_084650.md),
the real 3-host run had only short8 of delivered64 reach EOS and RELEASE; at the 30-minute stop, 8 medium/long requests were still in prefill
and 48 were pending. H0 v1's concurrent submission of quality64 mixed the boundary between H1 quality and the H2 concurrent wave, and did not put per-request
product deadlines into the event. Busy UNLOAD was not turned into success; the task-owned agent/native were terminated only through
failure recovery, and the 3 existing `:52005` agents were preserved. The event-drive RELEASE-based
`max_in_flight` and per-request deadline implementation passed 2 independent mutations, workspace feature off/on 1,529/0 each,
HF Python57/0, and real Qwen3.5-0.8B generation, cancellation, cleanup and re-acceptance on llama.cpp and HF. H0 v1 remains historical evidence of
the source at that time but is not used to approve new LOADs. The corrective implementation is bound to H0 v2 above, and
before H1 round 2 the new task agent preflight must pass.

**2026-09-16 Qwen122B H0 v1 historical seal:** The [H0 report](../tests/reports/release-a/20260916_072100.md)
binds the then-current runtime source `c6a28b582` and the [benchmark-spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v1.json).
It specifies the GGUF header's total 124,635,206,144 / active 9,954,546,176 parameters, device identification of the 3 physical hosts,
1Gbps/RTT and the reason power is unavailable, agent/native/library hashes, the `[0,24)/[24,36)/[36,48)` placement,
count/byte/token/KV/result caps for resident8 and pending64, and all H1–H7 workload/SLO/A-B/telemetry. Node10/10, Python4/4,
25 structural mutations and 1 file-replacement mutation passed. Only H0 is GREEN;
`load_authorized=true`, `runtime_acceptance=false`. The first next action is, before starting the `:22150` task agent,
to INSPECT and UNLOAD the existing task-owned node/listener/worker, re-check shard/source/binary/library/pool, and then
run NODE_LOAD 3/3 and H1 quality64. The existing `:52005` agent is preserved.
**2026-09-16 node lifecycle overhaul M4 complete:** M0–M4 of the [LOAD/UNLOAD lifecycle integration plan](node-load-lifecycle-plan.md)
are done. CREATE/DELETE on the normal path and the direct node lifecycle bypass were removed, and on real llama.cpp
2-stage and HF single GPU, generation, two requests, interleaved execution, cancellation, cleanup after partial LOAD failure, rejection of the previous generation,
reload on a new worker and final nodes/child 0 were confirmed. Python 57 tests and workspace feature off/on
1,526 PASS / 0 FAIL / 7 ignored each, and 2 independent recompile mutations, passed.
See the [M4 report](../tests/reports/node-load-lifecycle/20260916_064306.md). Small-model lifecycle acceptance is
not a Qwen122B H0–H7 promotion. The first next action is to review and seal an H0 benchmark-spec that binds the existing 122B 3-host artifact/topology and B0–B5 to the new
NODE_LOAD/NODE_UNLOAD contract. No LOAD before H0 approval.

2026-09-14 HF-0~3 acceptance complete: see the [integration guide](hf-integration.md) and the [acceptance report](../layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md). The HF-owned Rust bridge and per-model Python were connected to the P4 factory/INSPECT, and two physical hosts, cancellation/re-acceptance, cleanup, Python replacement, return-disconnect recovery and reproducible builds were verified. The existing llama.cpp also passed real generation and cleanup with the feature on/off. This is small-scale conformance, not an H0–H7 promotion.

## 0. Current status — HF internal integration verified, existing Release A state preserved

### 0.HF migration (2026-09-14)

HF was integrated into P4, and the full workspace passed with 1460 passed / 0 failed / 7 ignored for on/off each, along with model verification locally and on two physical hosts.
See the [migration report](../layers/adapters/hf/tests/reports/migration/20260914_120000.md). The original contents were moved to a backup; only deletion of the empty original directory was blocked.
See the [migration record](../layers/adapters/hf/docs/migration/README.md). The existing Release A real-hardware/occupancy state and failure evidence are kept.

### 0.A Release A kickoff (2026-09-14)

**Execution principle — use the first-verification pass rate as a quality metric:** The maximum of 3 attempts per phase is not a trial-and-error budget.
Before a candidate is first run, its callers, actual consumption path, state/byte ownership, no-effect-on-rejection, failure artifacts and existing counterexamples are
reviewed deterministically, and the first run gets a complete candidate whose implementation, compilation, consumption path and regressions are closed. Every failure
from compilation or test runs counts toward the attempt count. `first_pass=pass|fail` is judged not on a subset of the targeted tests but on the first complete gate
covering documentation, compilation, the actual consumption path and required regressions, and is recorded in the phase record. Attempt 2
adjusts only the bounded differences exposed by the first run, and attempt 3 confirms with a clean rebuild, full regression and independent mutations
without functional changes. If attempt 3 needs a new design or functional fix, the phase is judged a pre-review failure and stopped.
The operational goal of the roadmap is to keep raising the first-run pass ratio.

**Remote real-hardware preflight — reuse rules learned from failures as run conditions:** B5 and later remote model runs
start only after `tools/validate_event_runtime_preflight.py` approves the sealed run configuration and the immediately preceding evidence.
The evidence includes a bidirectional INSPECT round trip for each advertised agent exactly as configured, the response source and OUTER return route,
the dynamic/ephemeral port range queried on each participating OS, nodes/transport failures before start, and the count of task-owned native
child/listener processes. SSH or one-way connection success, or an agent restart, does not substitute for this.
After a failure, the cause is not only written into a report; a new counterexample is added to this check or to the actual consumption-path test before
moving to the next run. After shutdown, the same state items are checked again to judge cleanup completion.

**2026-09-15 FINISH deterministic resume:** Through the [resume verification](../tests/reports/release-a/20260915_142237.md),
Rust event-drive and the HF Python client were aligned to preserve all unexpected valid frames and partial EOF/timeout receipts, and to reject oversized frames
before allocating the body. The targeted tests passed in one go, but the first complete workspace gate
failed because of mixed EOL in 2 documents, so `first_pass=fail`. After fixing only the line endings, feature off/on each gave
1476 PASS / 0 FAIL / 7 ignored, Python49 PASS, removal mutations were detected, and real llama.cpp/HF generation and cleanup were confirmed.
Local functional acceptance of FINISH is complete. The first next action is to close the explicit settlement/reconnect contract for unknown transfer outcomes,
and then run Qwen122B full-topology PLAN, LOAD, normal responses and deadline/8wave.

**2026-09-15 transport settlement pre-review:** The [settlement/reconnect plan](../tests/plans/release-a-transport-reconciliation-20260915.md)
traced the current write completion, failure owner and duplicate window. With no remote acceptance receipt and no outstanding pin,
an implementation that only clears the peer cache or resends an uncertain Event is unsafe. The first candidate implements a versioned hop receipt,
count/byte/horizon caps, exact/unknown/quarantine, INSPECT, fail-closed for old peers and Rust/HF OUTER consumption
as one R1–R9 gate.

**2026-09-15 transport settlement R1–R9 acceptance complete:** In the [final acceptance report](../tests/reports/release-a/20260915_183158.md),
source `e41cf2c5f` verified hop ACK/receipt, bounded outstanding/store, preservation of unknown outcomes, exact reconcile, fail-closed for old peers,
INSPECT and Rust/HF OUTER consumption up to real physical 2 hosts. The full workspace with feature off/on gave
1490 PASS / 0 FAIL / 7 ignored each, HF Python50 PASS, and R9 Python2 PASS. 4 independent recompile mutations were detected, and
after a physical receipt loss, ingress uncertain→target accepted_exact→failure0/nodes=[] on both sides was confirmed. With the same final
source, Spark's real llama.cpp PLAN/HOP/KV restart reconcile/UNLOAD and HF single GPU's correct answers `4` and `서울` (English: "Seoul"),
logits4/cache4 parity, UNLOAD/DELETE and final nodes=[] passed. The old `:52005` agent's INSPECT
timed out, but it was confirmed to have model children0 and GPU occupancy0, so it was not changed. The desktop was not used for builds or model runs.
The first next action is A-PLAN: compute and seal the actual machine snapshot for the Qwen3.5-122B-A10B 3-shard, shared pool de-duplication, per-stage integer
allocation and a legal cut. No LOAD or arm repeats before PLAN approval.

**2026-09-15 Qwen122B A-PLAN acceptance:** The actual remote PLAN in the [A-PLAN report](../tests/reports/release-a/20260915_190631.md)
sealed Spark `[0,24)`, Mac20 `[24,36)` and Mac21 `[36,48)`. All three hosts have
the same upstream `451b89bae`/patch set `a07b7826…`, and all owned layers are on CUDA/Metal
with no CPU expert offload. After summing unified device/host entries into one pool and reserving 8GiB per host, the headroom is
62,921,352,800/10,569,843,648/17,677,750,720bytes respectively. The public `validateNativeDeployment` rejects illegal cuts,
source/patch, device and shape mismatches, shared-pool over-reservation and PLAN≠MEMORY_ACTUAL, and detected 4 independent source
mutations. Currently `loadAuthorized=true`; actual allocation and runtime acceptance are false. The first next
action is to run a 3-stage LOAD once in a separate agent namespace that seals the current source, and confirm MEMORY_ACTUAL agreement and
children/occupancy0 after UNLOAD. No request arm starts before the output/receipt/edge byte bounds are bound.
The full workspace with feature off/on gave 1490 PASS / 0 FAIL / 7 ignored each. Builds on this PC are blocked by default because an additional hard power loss
was confirmed during the later B4 cold build. Only when inference on this PC is unavoidable under separate permission is
the designated single RTX 3090 exposed; the RTX 4080 is not used.

**2026-09-15 Qwen122B A-LOAD acceptance:** In the [A-LOAD report](../tests/reports/release-a/20260915_195106.md),
current P4 agent source `8be242082` and the sealed native created
stages in a new `:52150` namespace on Spark/Mac20/Mac21 and loaded `[0,24)`/`[24,36)`/`[36,48)` concurrently. The first run was `first_pass=fail` because native
rejected port 0 in the LOAD endpoint as is; this was not a model or power failure. Attempt 2, using the free fixed
loopback port53150 as the launcher contract specifies, passed all three LOAD/READY and INSPECT. The actual allocation matched the per-stage
plan in every case, and `actualAllocationConformant=true`. Then UNLOAD/DELETE 3/3, final nodes=[] and
53150 listener0 were confirmed, and only the current task-owned agent52150 was terminated. The existing agent52005 was kept. The first next
action is to derive the four A-BYTES byte bounds from actual codec, mailbox and hop consumption and verify them
fail-closed in the manifest and runner. No request arm starts before that.

**2026-09-15 A-BYTES B0 acceptance:** In the [B0 report](../tests/reports/release-a/20260915_211000.md),
the compat patch computes the outgoing cut tensor payload/descriptor of the reserved worst graph with exact alias rules and checked
integers. native records the maximum result including physical-v4 metadata identically in PLAN, ACTUAL and READY,
and the actual C++ encoder and Rust decoder check this cap before allocation. Final CUDA CTest16/16,
Rust563/563 and the bound-removal mutation passed. In the real Qwen122B stage0 LOAD, payload786,432bytes/tensor1 and
max103,843,468bytes agreed across the three lifecycle records, and PID/listener/GPU were 0 after UNLOAD. The first full native
gate was 15/16 because the descriptor fixed part was overcounted by 2 bytes, so `first_pass=fail`. A-BYTES as a whole and actual
inference are not yet accepted. The first next action is to verify the B1 versioned resource profile before LOAD.

**2026-09-15 A-BYTES B1 acceptance:** In the [B1 report](../tests/reports/release-a/20260915_222346.md),
the version 1 resource profile owns all caps for request count/bytes, input/output tokens, native result, completion, edge and receipt.
LOAD checks the profile format, checked integers, internal caps and the actual remaining mailbox/edge/hop receipt capacity
before native. Workspace feature off/on 1,497/0/7 each, the real TCP shortage boundary, Qwen122B stage0
LOAD→READY→UNLOAD→DELETE and independent removal mutations passed, and all task-owned resources were reclaimed. This is a check of current remaining
capacity, not a concurrent-execution reservation. The first next action is, in B2, to reserve the completion forward and
the entire observation fan-out as one group before the native call.

**2026-09-15 A-BYTES B2 acceptance:** In the [B2 report](../tests/reports/release-a/20260915_232600.md),
a backend-neutral completion group reserves the full count/bytes of the forward and observation fan-out once
before the first/middle native call. Publication moves the same claim and checks the actual Event cost. A reservation rejection
preserves scheduler/flight/KV/native/ID/effect/output entirely. Package 682/0/7, workspace feature off/on
1,503/0/7 each and the reserve-after-native independent recompile mutation passed. The first package gate failed because of a store/profile mismatch in an existing
ring fixture that bypassed LOAD, which was corrected to the same 64 MiB contract, so `first_pass=fail`.
A-BYTES as a whole and actual inference are not accepted. The first next action is, in B3, to verify the lifetimes of pending/completion/broker receipt/
hop outstanding/native response as separate INSPECT fields.

**2026-09-15 A-BYTES B3/B4 acceptance:** In the [B3/B4 report](../tests/reports/release-a/20260915_235900.md),
a backend-neutral retention snapshot separates pending request, retained completion, broker receipt, hop receipt and
outstanding, and native response. The destination commits after reserving the actual Event cost, and the source keeps the original claim
until `AcceptedExact`. Count/byte bounds on the actual worker and TCP consumption paths, duplicate/late,
uncertain transport and the neutral adapter passed, and all 4 independent recompile mutations were detected. B3 workspace
feature off/on gave 1,506/0/7 each, and the relevant packages of the final B4 source passed on all 4 remote hosts.
`first_pass=fail`; a hard power loss recurred during the fourth consecutive local cold build in B4, confirming that the assumption that 33/48 affinity
meant 70% power was wrong. Since then, builds on this PC are blocked by default and only remote verification is used.
A-BYTES as a whole and actual inference are not accepted. The first next action is, before B5 starts, to INSPECT the new namespace and task-owned nodes
and UNLOAD/DELETE all of them, then run, with the final source, llama.cpp and HF/Python regressions and Qwen122B 3-host
boundary rejection, 1 normal request and final cleanup as a single sealed gate.

**2026-09-16 A-BYTES B5 acceptance:** In the [B5 report](../tests/reports/release-a/20260916_013500.md), the final
workspace feature off/on gave 1,506/0/7 each, HF Python 50/0, and native CTest on the three hosts 16/0 each.
2 small llama.cpp generations with cleanup and HF Qwen0.8B interleaved execution, cancellation and re-acceptance passed. Qwen122B 3-host
passed byte ±1 no-effect rejection, LOAD 3/3, a correct normal response of 119 tokens/EOS, delivered1/uncertain0,
UNLOAD/DELETE 3/3 and final nodes/failures/native/listener0. `first_pass=fail`; the repeated
return route and dynamic port failures were locked down with an exact round trip and an automatic preflight of the OS port range. A-BYTES B0–B5 are
GREEN and H0–H7 are not accepted. The first next action is the common codec and supervisor implementation of node lifecycle plan M1.

**2026-09-15 target change:** By user instruction, Release A proceeds on Qwen3.5-122B-A10B UD-Q5_K_S 3-shard.
The 550B originals and failures are preserved, but reloading and re-running them are removed from the preconditions of the new target. Qwen is verified with a new artifact/corpus/
PLAN/profile, and the criteria for resident8, long-form, 8wave, correct answers, SLO, cancellation/cleanup and regressions on both adapters are kept.
The exact topology is selected again on at least 2 physical hosts. The 550B 7host/8stage layout is not copied.
The full hashes of the 3 artifacts, independent verification of the new 64-item corpus, 6 contract tests and a one-cut PLAN on the local 3090 were confirmed.
Full topology, loading and distributed service are incomplete; see the [Qwen transition record](../tests/reports/release-a/20260915_131432.md).
The earlier record of FINISH stopping after 3 cumulative attempts and the unresolved return failure are not considered resolved by the model change.

**2026-09-15 model-free cluster query:** The user-specified [9-machine and MI250 SSH verification](../tests/reports/release-a/20260915_124433.md) was performed.
LAN 7×7 and the local gateway plus two MI250 machines 3×3 gave 542 normal responses and 20 rejections for wrong context.
Direct SSH timed out, but the Ubuntu jump and forward/reverse tunnels passed. After the conflicting-ID test, the state in which the failed peer
remains preserved was also confirmed; this is not acceptance of automatic reconnect/settlement. Owned test processes and tunnels were cleaned up.
No model generation re-verification or full Release A promotion is done, and the existing FINISH failure and the stop state below remain.

**2026-09-15 return context contract applied:** By user instruction, the [common return context](../tests/reports/release-a/20260915_121000.md) was implemented.
Explicit route and OUTER match verification for all valid events, removal of the source fallback, a common ReturnContext and
response selection per mixed owner were connected. workspace1472/1/7, 43 Python tests, 3 independent mutations, and
real llama.cpp/HF generation and cleanup passing only through ingress A were verified. The 1 failure is the existing FINISH. Completion of this scope is distinct from the stop/remaining roadmap below.

**2026-09-15 ingress route applied:** By a follow-up user instruction, the [ingress agent envelope](../tests/reports/release-a/20260915_113754.md) was reviewed and fixed.
Network returns go to ingress_agent, and only that agent registers the OUTER socket. Real TCP, 2 independent mutations and
llama.cpp/HF generation and cleanup passing only through ingress A were confirmed. The full workspace is 1465/1/7, and the 1 existing FINISH failure remains.
This change is limited to the ingress route. The FINISH stop below and the incomplete verdicts for the rest of the roadmap remain.

**2026-09-15 04:47 STOP:** Implementation verification in the [connection cleanup WIP](../tests/reports/release-a/20260915_044735.md)
failed 3 cumulative times in the ownership cost test, compilation and the unexpected-output preservation test. Per user instruction, at this phase,
development, re-testing, deployment and progress to the next phase are stopped. An intermediate PASS does not reset the failure count.
The earlier "next" items below are candidates after the stop is lifted and are not executed automatically. See the [test plan](../tests/plans/release-a-transport-20260915.md).

A0 proceeds per the user's follow-up development instruction. See the [phase plan](../tests/plans/release-a-20260914.md) and the
[A-RED/trace audit](../tests/reports/release-a/20260914_040306.md).
Timer no-input expiry and the actual RELEASE interleave were separated, and 2 independent recompile mutations were detected.
The full workspace is 1451 passed/0 failed/7 ignored. The runtime timer and default policy were not changed.
The past Nemotron per-request 7,864 prefill positions and per-stage RPC/forward were confirmed, but the pure kernel and
the lower bound of current pin cost are undetermined. The next task is connecting the A0 corpus/tokenization/manifest/runner and cost instrumentation.
A0 as a whole, A1–A5 and product SLO/H0–H7 are not yet complete.
The follow-up [corpus/spec preparation](../tests/reports/release-a/20260914_041920.md) verified real vocabulary tokenization of 64 items,
independent recomputation of correct answers from the source text, token identity of the existing 8 items, and spec rejections/4 mutations.
Current pin cost instrumentation, a complete cap derivation for native PLAN and the service runner remain.
The conflict between metadata rejection and the isolation check that blocked the current native rebuild was separated inside compat,
and the [full/relink and both-adapter regressions](../tests/reports/release-a/20260914_043652.md) passed.
[Native cost observation](../tests/reports/release-a/20260914_050400.md) was added, and the CPU real path on/off and the omission mutation were verified.
[Current pin/deployment candidate verification](../tests/reports/release-a/20260914_054100.md) confirmed token identity of 64 items, CUDA/Metal native,
5 OS agents, llama/HF regressions on the new agent and native→Worker cost binding.
The [fleet preflight](../tests/reports/release-a/20260914_062025.md) confirmed 7-host INSPECT and 6/8 stage native PLAN.
CUDA environment passing and device checks in the check tool, and 2 tool-removal mutations on the real native path, were verified. Check-owned processes, tasks and rules were cleaned up.
The [app runtime environment recheck](../tests/reports/release-a/20260914_102600.md) withdrew the verdict that `.29` model access was blocked.
The installed app reads the same 10 shards on S:, and the remaining stage0 and 1 also passed in the user session, giving native PLAN8/8.
Access failures of SSH and the test S4U agent are not generalized to the state of the installed app. The existing installed app is kept.
Next is verifying real LOAD, normal responses, occupancy cost, instrumentation overhead and transfer/client latency for 550B on 7host/8stage.
Just before the run, a loading node for a separate Qwen122B demo appeared on Spark, leaving 44.20GB available against 84.05GB needed, so concurrent loading was impossible.
Resume after that demo's resources are reclaimed and current occupancy is rechecked. This is not an installed-app/NAS access block.
In the [2026-09-15 fleet cleanup](../tests/reports/release-a/20260915_005456.md), existing nodes were cleaned up per user instruction.
Registered 7/7 INSPECT nodes=[], and 0 P4 native processes on the 9 physical machines. The failed UNLOAD and cold recovery are preserved separately.
After rebuilding the agent at the current HEAD, proceed with the same 550B real-hardware run. Past request settlement and service acceptance are not approved retroactively.
The [current fleet resume](../tests/reports/release-a/20260915_011200.md) passed a current-source rebuild, PLAN8/8,
7-edge round trips and both-adapter regressions. On Mac, the same binary was run in an SSH foreground session to
confirm direct LAN return. 550B case-00 passed correct answer/EOS, completion/release 1/1, UNLOAD/DELETE and 1104 cost bindings.
With instrumentation off it also passed the same 94 tokens, correct answer/EOS, completion/release 1/1 and cleanup. On/off TTFT 662.342/656.619 s,
E2E 770.047/763.552 s and ITL p95 1.741/1.754 s failed the SLO on both sides. This is a single fixed-order pair, and
the observed E2E difference of 0.851% is not confirmed as causal instrumentation overhead. Per-request deadlines are not implemented.
Test agents/tasks/rules were cleaned up, and native0 on the 9 physical machines and existing registered 7/7 INSPECT nodes=[] were reconfirmed.
The old Spark/Ubuntu agents hit control-connection saturation again and were cold-recovered with the same binary and configuration.
A permanent fix and past request settlement are not approved. The first next action is counterexample, consumption-path and mutation verification of connection slot lifetime
that preserves TCP half-close/return route ownership; the CPU expert cost candidate, a finite profile and the deadline runner remain.
A0 as a whole and A1–A5/H0–H7 acceptance are incomplete.

### 0.HF acceptance results (2026-09-14)

By user instruction, **[p4hfadapter acceptance](external-analysis-improvement-plan.md#hf-integration) was completed before all existing
release work.** The execution runtime is P4 `0bd3734a9` + HF `6d8144d`; the later test tools/documents and
shipping commit follow the source manifest of the acceptance report. The common core/protocol/existing llama runtime were not modified.

1. 9 HF bridge tests, 12 real broker fixtures, 5 independent Rust mutations and 7 Python mutations were verified.
2. Local Qwen event99 logits/198 stage-cache, two physical hosts 75/150, and recovery after return disconnect 4/8 passed.
3. Within the same LOAD, 8 requests×3 epochs and rejection of the previous epoch, and Python A→B plus llama.cpp generation/cleanup on the same agent, were confirmed.
4. The full P4 workspace on/off gave 1450 passed/0 failed/7 ignored each. Ignored tests and external HF tests are counted separately.
5. The first next action is to audit HEAD/dirty and the existing large-model failure evidence when Release A starts. This §0 request does not run A automatically.

P4 knows only `hf-transformers`; per-model Python and the concrete Rust adapter are owned by `layers/adapters/hf`.
INSPECT and generation consume the same factory list. The small Qwen integration does not change the existing H0–H7 goals.
The existing llama timer test was PASS in this full run, but because the timer implementation was not changed, no general fix of the past RED
is claimed. The heterogeneous BF16 failure and Release A's incomplete large-model/SLO work are preserved.

### 0.Existing product development plan (2026-09-13, applied after HF acceptance)

The [single development plan](external-analysis-improvement-plan.md) cross-checked the external deck, sparse analysis, batching G1–G6 and
DFlash/DSpark against the actual code at `245d6b785c96ec770dc6041ded35457c2ef97260` and official sources.
No separate per-version files or earlier conversations are required. Plan writing is complete, but product development and remote real-hardware runs
were not resumed at this time. Below is the product lineup to apply after HF acceptance. On a new development request, the first target is Release A.

| Priority | Product and exit condition | First next action |
| --- | --- | --- |
| After HF acceptance | **Release A: sustained use of the existing large model.** Ship long-form normal service, batch progress, cancellation, cleanup, re-acceptance and a finite deployment profile for Qwen3.5-122B-A10B together | Audit HEAD/dirty with the [fresh-session procedure](external-analysis-improvement-plan.md#fresh-session) and reproduce the timer RED. Write the corpus/manifest/actual runner of the [A acceptance contract](distributed-batching-verification.md#release-a-contract), separating the cost and termination paths |
| Conditional | S: useful long-form service on one selected sparse model | Confirm artifact, auxiliary state, legal cut, actual backend sparse computation and task utility versus the existing model. Releases that only flip a flag are excluded |
| Conditional | B: reuse conversations over repeated document/code context | Adopt based on actual valid prefix, checkpoint bytes and net savings across cold/hit/no-hit. Complete request consumption together with eviction, contamination prevention and cleanup |
| Conditional | C: speculative response service matched to the target | Compare non-spec/MTP with feasible DFlash/DSpark and low-cost drafts and select one approach. Include hidden-state transfer, state replay, queue/fence and draft memory. Ending the investigation at MTP alone is not allowed |

A's A0–A5 are implementation/verification commit units, not separate product versions. G1/G3/G4/G6 are bound to A's required
progress/observation contract, and G2 early UBATCH send and G5 multiple prefill fragments are scheduled into a later product only when their cost and state contracts
are proven. The existing OUTER planner, receipt fix and physical MTP are not reimplemented.

The current batch selection re-run is **44 passed / 1 failed**, and reproducing the timer alone also failed. The original document's
45/45 is not handed off as current GREEN. Follow the [correction and reproduction](batching-code-review.md#review-correction),
and fix it after distinguishing whether it is an observation-timing problem with normal RELEASE or a runtime permission change.

S/B/C are not serial phases that must all be implemented. The order is decided by task quality, actual recomputation cost, cost per confirmed token and
maintenance cost. No candidate ships in a state that becomes operable only after the next product arrives.
Acceleration is not layered on top of unapproved sparse-model state or kernels. Every product must satisfy independent normal service, cleanup and
multi-computer real-hardware acceptance. The existing V1.1/M3 closures and the Nemotron failure are preserved as is.

### 0.MiniMax M3 MSA closure verdict (2026-09-13)

The TPS of the earlier old dense-fallback GGUF is not a MiniMax M3 MSA performance baseline. With the valid
Bartowski Q5_K_S 8-shard artifact, the CUDA and Metal heterogeneous distribution was judged again. Context
4,096, sequence 1, batch/ubatch 128/64 and `--flash-attn on --no-kv-unified` were used, and
on the central host only the user-designated RTX 3090 was used.

The native plan exited with code 7 on every stage — central CUDA0, Spark CUDA unified memory and Mac21 Metal — with
`llama.cpp memory implementation does not declare stage-local residency support`.
Actual CREATE was 4/4, but LOAD was rejected with Mac21 native exit 5, and SESSION and queries were not
run. Normal responses and TPS are not measured. Detailed evidence is owned by the
[MSA distributed load rejection record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md)
.

Compat patch 0029 only added a static flag to the MSA wrapper and had no effect on the actual virtual residency check,
so it was removed. The product returned to the 27-patch fail-closed state. Explicit rejection of a missing indexer,
disabled flash attention, and the combination of multiple sequences with unified KV remains.
In this version, M3 stage residency is not modified or re-tested further.

The model placement policy and spec collection are kept independent of the M3 result. `tools/model-loading`
first reserves KV/runtime headroom, then computes the minimum tier in the order `GDDR -> Mac unified -> GB10 unified -> x86 DDR`
and a contiguous cut based on service time. Agent INSPECT probes, beyond NVIDIA,
Linux AMD DRM and Apple `system_profiler`, and the OUTER collector preserves the timestamped history and
`latest.json`. Fleet deployment and real-hardware acceptance of that feature are work for the next version.
### 0.Nemotron LAN mixed-wave follow-up test (2026-09-12, local state rechecked 09-13)

**2026-09-13 recheck:** The local test-progress ended after about 7,203 seconds of the long arm with completion/release 0/0,
deadline, missing observations and UNLOAD busy. Mixed progress refused to start with old native work remains.
The 22:11 progress and the next action of that time below do not take precedence over this end
state. No new real-hardware or remote check was done, and the cause and valid goodput are undetermined. For file identification and
investment impact, see the [review basis](external-analysis-improvement-plan.md#2-starting-point-according-to-current-code-and-measurements).

This is a separate real-hardware run performed per the user's follow-up instruction on loading, long context and mixed repeated waves. It does not change the V1.1
3-turn closure and no-service-promotion verdict above, nor does it automatically resume the closed algorithm development.
The source reference is `10e8dfc9b`, and the execution adapter is the `a43950bed` build. Product code and batch policy were not modified.

- Nemotron 550B UD-Q5_K_S was loaded on the same 7 LAN machines, 8 stages (CUDA/Metal and CPU expert offload).
  Actual LOAD/SESSION 8/8, and short arithmetic smoke EOS, completion and release 1/1. The smoke kept the load, so it is
  not UNLOAD acceptance evidence. Run data is in `target/nemotron550-all-fleet/`.
- The existing cold arm is 8 new 100,038-token inputs, output cap 2,048 and resident 8.
  At the 2026-09-12 22:11 KST check it was still in prefill, and the limit time is 23:28:01 KST the same day.
  Conditions are not changed midway. Long-form normal responses, full release and UNLOAD are still undetermined.
- The follow-up runner in `target/nemotron550-mixed-waves/` started at 22:11 KST. The current stage is
  `waiting_for_cold_arm`, and mixed inference has not started yet. After confirming the end of the existing test and instrumentation and
  0 existing native children, LOAD/SESSION with a new name and generation are run.
  The existing failed worker is not treated as a normal drain. Device, batch policy, KV and resident are kept.
- Inputs, by the real tokenizer, are requests of 86–6,512 tokens and 1 request of 100,038 tokens, 127,735 tokens in total.
  64 requests are sent at fixed times in 8 waves at 0/180/480/780/1080/1380/1680/1980 seconds.
  4 of the 8 short inputs in the first wave require long outputs, so that later prefill and generation overlap.
  Output requirements are 50 short, 7 medium and 7 long. The actual consumer's output cap is a common 2,048,
  and length differences are checked via prompt requirements, EOS and per-response minimum length. It is not an implementation of different hard caps.
- The new LOAD limit is 75 minutes, and the mixed inference limit is a separate 2 hours. On timeout, that run's native processes are
  reclaimed first, and partial results, the first error and cleanup errors are preserved. Executable hashes on the 7 machines were rechecked,
  the mapping of 64 inputs/waves/expected responses, the context cap and prompt hashes were checked, and the run script syntax was checked.
- `MANIFEST.json` seals 78 input and runner files. `progress.json`, `mixed-artifact.json`,
  `request-metrics.json`, `report.json`, GPU/RAM and run logs are the status and verdict data.
  A scheduled/actual send difference over 1 second is recorded as an INVALID arrival spec. Actual ITL, TTFT, incomplete requests and D+P
  observations are separated, and valid goodput is not approved before semantic review.

This is not a batch policy A/B or a full H2/H5 acceptance. FIFO acceptance is unchanged, and no slot reservation
policy for long requests was added. No TPS improvement rate is claimed from the different input compositions of cold and mixed.
Raw data exists only in the current local target, and the long-term evidence bundle and final result record are incomplete.
The next actions are: collect the existing cold results → run the new LOAD/mixed waves → judge together the results including incomplete requests and
per-request wait, generation latency and long prefill progress.

<a id="v11-plan"></a>

### 0.V1.1 MI250 and Hy3 integrated fix plan (2026-09-11)

Real-hardware evidence is sealed in `F:/dev/p4-releases/v11-profiled-cohort-a43950bed-runtime-20260912.zip`.
SHA256 `89fedfd692dee3f0571fa91737982ee42dd3eafbffce405b9b5d4b1dc6fc0d6a`, all 682 members verified.
Model and binary hashes unchanged, owned processes/ports cleaned up, protected agents kept.

**The final status takes precedence over the progress history below.** The three turns are closed. The generation-first and independent-cohort code is kept,
but latency16 is kept only as a candidate comparison baseline for the next version. Throughput512 had a short ITL of 7.346 s and completion 0/16 at 100k, so it is
not promoted to a recommended service value. 100k baseline/latency completed and released 8/16, but all ended by length, and the 8 long requests did not complete.
Hy3 real-hardware is not run/BLOCKED, and B2/B3 as a whole, H5 and valid goodput are not approved.
The first task of the next version is a termination watchdog independent of timeout cancellation/drain/reservation return and artifact transfer.
After that, separate the actual n_kv, CPU mask and HIP attention costs to improve the native work scope, and build per-context advance profiles.
Verify Hy3 after the OS return route is restored. No follow-up development starts in this closure.
[Final raw-data verdict](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#profiled-long-closure).


**Top-priority closure rule (user instruction, 2026-09-12): at most 3 turns.** The response that received this instruction is 1/3,
and intermediate commit requests that arrive within it are reinforcements of the same turn. Later user or automatic goal resumes also consume remaining turns.
In turn 3, test results, adoption/rejection, commit/remote state and process cleanup are all closed out, and development/re-testing in a 4th turn is not started automatically.
This rule takes precedence over the earlier open research/development order below. Commit at every restorable intermediate point.

| Turn | Fixed work | Exit condition |
|---|---|---|
| 1 — design freeze | Cross-check the actual Sarathi scheduler/PP runner against existing raw data. Fix the algorithm, cost-profile selection rules, pass criteria and real-hardware scope | Contract below and evidence §27 commit `da6507164`. Done |
| 2 — implementation, local verification, real-hardware start | Independent generation cohorts, generation-first remaining-token allocation, initial request protection. Actual consumption counterexamples/mutations/full tests. Fix operating values from one cost-profile pass on the same native, then seal/start the remote arms | Intermediate commit of implementation/local verification. No source/profile changes during measurement |
| 3 — real-hardware closure | Declared comparison on MI250/Hy3, completion of long inputs/outputs or an explicit timeout failure verdict. Check full response text, latency, throughput, memory and cleanup | Verdict of recommended value or baseline kept, evidence/code commit and push confirmed. No follow-up turn/exploration |

**Currently 2/3 — implementation midpoint:** Independent cohorts based on generation population, preservation of initial prefill width, a total token cap,
and a coalescing target no larger than the legal generation width were implemented. Adapter 557/0/0 and composer 4/0 passed.
Porting the final fixture onto the previous source makes 2 actual consumption tests fail. The implementation midpoint commit is `a9d2d1a63`.
Follow-up verification ended with full workspace 1445 passed/0 failed/7 ignored (58 summaries), 6 independent mutations each with 1 failure, and
docs-lint94 clean. The source and each recompiled binary hash are bound in evidence §28.
Source `a43950bed` was pushed to main/GitHub main, and MI250/Spark/Ubuntu/Mac and Windows Release builds were completed.
One advance profile pass of 6 runs passed 96/96 completion, release and UNLOAD. The latency candidate of 16 total rows and the throughput candidate of 512 rows were
sealed per the selection rules. This is a selection within the calibration sample, not a performance/service approval. A generation-only comparison
on the same source and native has started; after it, the 4k mixed comparison and actual 100034 tokens×8 long requests + 8 short requests will run.
Output 2048/minimum 1024/EOS and context 102400 are fixed. The common real-hardware cutoff is 2026-09-12 05:25:14 KST.
On Hy3, P4 return failed because local Windows TCP blocked the new test exe, and the rule change was rejected
for lack of administrator rights. A scope-limited/revert script for the administrator and a request for user input were prepared. No response is not interpreted as permission.
Remote test agents and temporary remote rules were cleaned up. In the next 3/3, the remaining real-hardware runs, verdicts, evidence bundle and cleanup are closed out.

**3/3 progress — short comparison finished:** Generation-only went from 155.25→206.06 raw TPS, and pooled actual ITL p95 from 92→89ms.
In 4k mixed, against the baseline of 75.36/76.08 TPS, the throughput candidate gave 83.11 TPS, short ITL p95 2550.5ms and long TTFT p50 16.715s,
and the latency candidate gave 56.19 TPS, 198ms and 39.031s. Both candidates passed the initial short-TTFT regression criterion.
These 6 holdouts were 96/96 EOS, completion, release and UNLOAD, but the 120–180-word contract was not fully met, so this is not approval of valid goodput.
The actual 100k comparison is running. No default, H5 or Hy3 promotion is made from current data.

**3/3 execution exception record:** The 100k baseline comparison failed at 1800 seconds with 8/16 completed and released, and UNLOAD busy required forced native cleanup. The original runner stopped as scheduled. Afterwards, absence of owned processes, return of 9 ports, 8 GPUs idle/under 512MiB and survival of the existing protected agents were confirmed separately. Conditional on passing this resource audit, only the two long arms of the already sealed Q512/Q16 are run under the original 1800-second/common 05:25:14 KST limit. The original automatic-stop text is preserved, and code, profile, input, output and pass thresholds are not changed. This is an explicit exception to the execution order and does not approve the failed comparison.

**Policy to implement this time:** Apply Sarathi-Serve's advance-profile-based token budget + generation-first chunked prefill
to P4's finite independent flights. The P4-specific part is preserving request cohorts by admitted ready/inflight population.
Merging all decode, and the existing cold time gate that learns from 1 row while serving requests, are not used in this operating candidate.
There is one method, and OUTER selects a pre-fixed profile that fits the **throughput-first/response-latency-first** objective.

- The generation participation cap is derived from the total active generation population and the independent cohort target, not from instantaneous ready/free.
  The default safe cohort target is within the existing allowed flight window. Node execution 1 and decode outstanding 1 per request are kept.
- Reserve generation rows of the selected cohort first, and assign P to the remaining **total token budget**. A single large request does not exhaust the whole
  ready set, and the existing rotation order and prefill progress opportunities are preserved. Legal progress in different stages
  is not blocked by a global drain. The coalescing target is kept from exceeding the acceptance width of the actual cohort.
- An initial P cohort whose actual generation has not started yet is not turned into a small quantum because of a single 'future generation'.
  The TTFT regression counterexample for the initial 8 short requests is included. Cost learning/preparation cost is measured separately and not hidden in the first customer request.
- The cost profile is bound to model/native/device placement/phase/context/width. Existing samples under the same conditions are used first, and
  missing cost samples are filled by one pre-fixed profiling procedure. Settings/verdict thresholds are not changed after seeing the main test results.
  Throughput-first maximizes useful throughput within the measured range, and latency-first selects the largest legal token
  budget within the latency target. 250ms for MI250 and 5000ms for Hy3 are **experimental actual ITL p95 targets** bound to earlier figures, not a confirmed user product SLO.
- Unmeasured 100k cost is not approved by linear extrapolation from 4k. Outside the profiled range, the verdict relies on fixed safety limits and actual
  measurement, and meeting the latency target is not guaranteed in advance. The existing resident/open/fragment limits are not raised.

**Verdict freeze:** Compare against the same native/model/placement/input on the current 26c13b9ae baseline. Generation-only throughput ≥95% of baseline,
actual ITL p95 ≤110% of baseline, and initial short request TTFT p95 ≤125% of baseline are the common regression criteria for both operating modes.
These are the screening criteria of this engineering pass and do not replace H5's approval criteria. The latency mode evaluates the actual ITL target above,
and the throughput mode evaluates useful throughput under the same load/quality contract; P throughput and long-TTFT losses are not hidden.
Fixed output length for load diagnosis is distinguished from natural EOS/content verification. The existing 120–180-word failures are recorded as is, and the judge is not relaxed.
Generation-only / 4k mixed / actual 100k long input and long output / completion→release→UNLOAD are the declared scope. In-cluster comparisons take priority, and
a single-host MI250 result does not substitute for Hy3 acceptance. A 2-hour common cutoff applies to all remote real-hardware runs.
If host access, memory or budget is blocked, the raw failure and partial results are kept and the verdict is closed within these 3 turns.
Fixing and re-verifying safety defects is allowed, but no candidate redesign or knob exploration is done because the performance results are disappointing.
Details are in [source/runner cross-check and selection rationale](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#three-turn-closure).

**User resume goal (2026-09-11, baseline `5ee827af5`):** Confirm main/remote match → latest official
llama.cpp compatibility, Release build, small model → implement the verified batching design on the actual consumption path → perform MI250 and
Hy3 real-hardware re-runs as one task. The two existing evidence ZIPs are preserved as immutable baselines.
At start, local main and GitHub main were the same full SHA, with no dirty/untracked files.
The latest upstream found is `451b89bae0c4b1dd612eb503ceace906c01ddcc9`, which is distinct from approval to adopt it.

This implementation does not end with further exploration of decode cohort knobs. It proceeds under the contract below.

1. **Independent prefill cohorts:** All 91 initial pure-prefill flights on MI250 were 16 requests×32 rows=512 rows,
   with open=0 before publication. This is the same for both candidate and baseline. Keep the total width while limiting the number of participating
   requests, so that other request cohorts can be published immediately. Verify first with fragment=1.
   For 16 requests, 2 requests×256 rows is the first counterexample input that makes 8 independent cohorts, not an optimal value.
2. **Per-phase cost and fairness:** pure-prefill fills an efficient chunk, and when coexisting with decode,
   a separate prefill service budget applies. Short prompts/new requests are not left waiting forever, and
   a selection rejection does not consume fairness. RPC averages with different prefix lengths, phases and cohort sizes are not
   regressed onto a single fixed cost. A time budget is not a real-time guarantee for a non-preemptive native call.
3. **Safety budget:** Pending prompt/return/flight and broker receipt are each limited.
   Duplicate/replay contracts that need the original are not silently replaced by payload deletion or hash equivalence.
   Node execution 1, decode outstanding ≤1 and KV reuse after completion are kept. Increasing the fragment window comes only after a separate proof.
4. **Research binding:** Compare the latest llama.cpp `tools/server/server-context.cpp::update_slots`,
   vLLM V1 `scheduler.py`, SGLang `schedule_policy.py::PrefillAdder` and Sarathi-Serve.
   Token budget, KV reservation and chunked prefill are design principles to reuse, but a single instance's selection of all decode
   at once is not transplanted as is into a distributed pipeline. Unverified GPU cost is not declared optimal.
5. **Verification:** Each fix is bound as actual consumption counterexample → fix → independent mutation → full gate.
   Within each cluster, the old/new policies are compared on the same native to separate upstream updates from policy effects.
   MI250 fixes the new 451 native and Hy3 the existing fleet 434 native on both arms. All-platform adoption of the latest native
   is separate from this policy comparison, and Hy3 as a whole is not reported as re-verified on the latest upstream.
   After small discrimination arms, many actual long inputs and long outputs are run, and input token counts are confirmed with the tokenizer.
   The context of 100k input plus output budget, KV-first device placement, RAM offload and host CPU budget are stated explicitly.
   Occupancy by other jobs on MI250 is monitored, and unrelated processes are not terminated. Hy3's CPU expert bottleneck is judged separately.

This resume item defines the current first action. The unfinished budget/quality gates in the earlier progress table below remain open.

**Resume progress:** The 26 patches on the latest pin passed clean replay/classification checks (`0cf373d83`).
CUDA sm_86 Release CTest16/16, MI250 gfx90a ROCm Release CTest15/15, and Qwen2.5-1.5B
real CUDA inference, KV save/restore and UNLOAD 1/1 were completed. This is small single-device approval scope.
The admitted input account was connected to the actual PREFILL consumption path to limit count, capacity bytes and input/output tokens of pending/active/shared provenance.
This is not B2/B3 as a whole; native return/outbox/broker/remote grant remain.
For the admitted input unit, workspace **1391 passed / 0 failed / 7 ignored** (58 summaries), 2 actual consumption tests and
independent mutations (admission bypass 1 failure, retirement omission 2 failures) were confirmed (`54b4fd9b5`, pushed to main).
The optional pipeline policy divides the number of eligible requests by the number of remaining flight slots to build independent cohorts while keeping
the full pure-prefill row width. The mixed-prefill quantum also applies while decode is in flight.
Only a finite open window and fragment1 are allowed, and it is disabled by default. The cost model, time SLO and B2/B3 as a whole are not yet done.
Actual loop counterexamples, 2 independent mutations (2 failures each), workspace **1396/0/7** (58 summaries, exit0),
and Node4/4 including actual CLI rejection/preservation in the common OUTER composer were confirmed. e4503e10f closed out the same-condition comparison real-hardware runs on both clusters.

**This resume's closure — current verdict:** Independent prefill cohorts actually reduced return wait. While keeping the initial central 512 rows on MI,
participation went from 16→2 requests, head inter-RPC wait from 1948→196ms, and total concurrent RPC from 0.96→4.58/8.
MI raw generated went from 22.83→39.00 TPS (+70.83%), and the first 1024 generations of all requests from 1060.5→394.9 s, but the candidate had EOS 15/16 and leading calculation 2/16, so
valid response performance is not approved. The P4-free native baseline on the same 16 prompts also scored calculation 2/16, reproducing the error in the model path itself.
The Hy3 candidate was 8/8 EOS, completion, release and UNLOAD, 7.5298 TPS, leading calculation 7/8. The baseline completed 0/8 at the 2-hour common cutoff,
and 3164 partial outputs were preserved. Only the first output of all requests, 1871.1→960.4 s, is a common latency comparison; no overall TPS improvement rate is computed.
The actual longest input was about 10.6k; 100k inputs, continuous waves, H5 and an optimal policy were not verified. Default disabled is kept.
Raw data, before/after hashes of 132 executable paths, cleanup and causal analysis are in the
[independent prefill comparison real-hardware run](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#prefill-cohorts-20260911).

**Current first next action:** The all-decode merge of the c6bd6c597 candidate below is not adopted as a performance policy.
Separate ready/inflight population from measured stage cost so that independent generation cohorts are kept even without prefill.
In the actual consumption counterexample with 16 generation requests and window 8, first lock down that several independent flights are published before the first tail return,
and that repeated returns do not merge them into one or two cohorts. Within a selected cohort, generation-first is kept.
Then confirm no regression in generation latency and throughput in generation-only and mixed comparisons with the same native/input/placement.
The time-250ms candidate is not recommended either. It is not made to pass with an arbitrary larger number; initial learning delay, actual execution remainder,
cost selection including transfer/return and request deadlines, and prefill time deficit/aging are bound in turn.
Scaling to actual 100k comes after the independent receipt/future-return byte budget and this consumption/real-hardware gate.
Hy3 stopped before inference at the Mac agent's reverse P4 response `No route to host`. Python TCP success is not
extended to approval of agent communication. Test process cleanup and restoration of the M42-only firewall exception were completed.
See [real-hardware scope, refutation and RPC attribution](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#cohort-runtime-audit).
See [consumption counterexamples and verification](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#prefill-population).

**Generation/time policy implementation (2026-09-12, local GREEN):** ready decode is not split by instantaneously free flights; it is
assigned first while respecting explicit caps/physical capacity. The last prefill return is also considered as future generation service.
The last stage RPC completion is predicted including decode-only cost feedback and the order of open work, and
when a large candidate is rejected, prefill rows are reduced and the actual plan is rebuilt. Unknown/impossible costs are measured with a 1-row probe when there is
no earlier prefill. Default disabled, ordinary/fragment1 and the existing window are kept, and the same number as the earlier per-stage
service budget does not have the same meaning. Full 1439/0/7 (58 summaries, exit0), 5 independent mutations
each 0 pass/1 fail, and the timer's actual wait, no-input republish and ledger invariance were confirmed.
The barrier where learning an initial small shape required first waiting for a large prefill's return was also fixed with an actual consumption counterexample.
After large work, one cold 1-row probe within the existing window is allowed, but none is added if a 1-row probe or an unidentified open already exists.
After the reinforcement, full 1441/0/7 and 6 independent recompile mutations each 0 pass/1 fail were confirmed.
A client deadline including transfer, dispatch and return remainder, time deficit/aging, and full return reservation still remain.
The reference is [implementation, counterexamples and verification scope](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#generation-service-policy).

**Generation/time candidate real-hardware verdict (c6bd6c597, performance RED):** On the same MI250 single host, 8 stages, new HIP native,
baseline 26c13b9ae / generation-first / generation-first + time 250ms were compared in A/B/C/C/B/A order. Each passed 16/16 EOS, completion, release and UNLOAD,
actual 45-layer ROCm placement and before/after executable hashes. Actual inputs were 8×213 tokens + 8×4255 tokens, not 100k.
Raw TPS was 72.84–75.04 for baseline, 50.27–52.31 for generation-first and 30.43–32.53 with time. Full denominators and output counts are in the evidence.
The actual ITL p50 of short requests during long prefill was 616–628→646.5–680→172–180ms, but
long request TTFT p50 was 21.09–21.21→22.23–23.59→65.16–69.92 seconds. The initial 7 short requests of the time candidate also
had their first output delayed to 10–13 seconds. Under pure generation with 16 active, generation-first regressed width from 2→about 8, open-before-publication from 6→about 1,
and head inter-RPC wait from 0.51–0.55→71.82–77.69ms. The design that equated generation-first with merging everything is corrected.
The 120–180-word instruction was met in only 50 of 96 requests in total. EOS/basic judge pass is not extended to approval of valid goodput.
Test processes were cleaned up, and the existing MI agent43015 was preserved. Default disabled is kept; promotion to a recommended setting/optimal value is rejected.
The current verdict is [6 arms, regression cause and fix contract](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#generation-service-screen).

**Findings of the investigation into the user's proposal:** The direction of putting prefill into the budget left after generation-first is adopted. P4's general attention
already prioritizes generation rows, but mixed results are returned only after the whole computation, and the fixed 128-row quantum did not protect generation latency.
In a real-hardware run with the same GPU placement, the token interval p50 of short requests was 45ms before long requests arrived / 637.5ms during long prefill / 76ms after.
Another comparison was also 46/636.5/76ms. Context and demand differ per interval, so no causal ratio is claimed.
Sarathi-Serve, vLLM, DeepSpeed-MII, TensorRT-LLM, SGLang and the current llama pin were cross-checked in code/source text.
The detailed basis is [investigation, code defects and discrimination experiments](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#decode-first-research).
The subsequent order is (1) no regression under repeated returns for independent generation cohorts, plus ready→issue/tail and per-stage remainder instrumentation,
(2) joint selection of cohort width/concurrent flights/chunks from observed costs, limiting long waits during initial learning,
(3) chunk re-selection after generation-first within each request's time slack, including context, already-published backlog and transfer,
(4) acceptance of many actual 100k requests/long outputs, binding prefill time deficit/aging with full KV/return byte reservation.
pure-prefill keeps sufficient row width and independent groups, and if the bottleneck is already busy, no more flights are stacked.
equal-width hybrid implements the same service goal with a legal phase split. Default promotion/optimal values come after real-hardware runs.

This native checks `--expect-layer-device begin:end:name` against both the no-alloc PLAN and the actual LOAD,
and rejects when the default layer device of PLAN/LOAD changes even if the byte total is the same. Explicit CPU ranges and expert offloading are distinguished.
workspace1430/0/7, CUDA CTest16/16, actual stdin7/7, 5 independent recompile mutations with 1 failure each,
and 2 product agents, 2 GPU UUIDs and Qwen1.5B 4/4 EOS, completion, release and UNLOAD were confirmed. This is not approval of the full quality instruction set or 100k.
The 12MiB difference between compute PLAN and actual when one native auto-splits across 2 GPUs was reproduced on both old and new binaries and remains a separate RED.
See [counterexamples, consumption verification and constraints](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#layer-placement-gate).

The existing OUTER plan `45-cut_begin` for MI native451 assigned the first repeating layer of each stage to CPU.
The raw 27.42→75.02 TPS from correcting only the placement is not an achievement of the batching algorithm alone. The corrected plan is fixed as the next MI baseline,
but number+1 is not applied wholesale to other native pins/Hy3. ROCm PLAN and rejection of a wrong LOAD with the new query, and
a correct LOAD/inference, were confirmed. The Metal real-hardware run of the same new native still remains.

With the earlier CPU placement, at most 2 independent prefills gave head idle 670→132ms and raw 14.86→27.42 TPS, but
mixed ITL went from 863→1078ms. With the corrected GPU placement, the automatic policy already published 1 prefill request at a time, so
cap 2 had no additional effect (74.70/75.02 TPS). Therefore a per-model-name constant of 2 is not put in as a default.
The next policy unit is to separate independent cohorts, row width and flight targets based on per-phase active/ready/inflight population and stage cost.
The 8-request concurrent outstanding counterexample on the slow/offload path and no regression in existing supply on the GPU path are verified together.

The fixed 250ms service budget degraded raw 27.42→13.87 TPS and long TTFT 72.813→205.312 seconds under the at-most-2 condition, so
it is not adopted as a recommended/default policy. Time feedback is kept and evolved into a policy that selects discrete chunks and coalescing
under the decode tail completion time, reflecting execution remainder, already-published work and transfer. Prefill deficit/aging is bound as well.
The existing safety window is kept first, and exact broker receipt/advance reservation of future return bytes is closed before scaling to actual 100k inputs.
Actual 100k multiple arrivals / arrivals during generation / 1–2 long requests, long natural EOS and content acceptance are repeated separately.
V1.1-3 fragment window expansion comes after KV ordering and cancellation/return proofs, and H5, an optimal policy and valid goodput are still unapproved.

The Hy3 new 5-host agent build finished, but this connection check stopped at TCP blocking of the new M42 executable.
The temporary M42 rule was restored, and the local test executable TCP51118 could not be changed for lack of administrator rights.
The required local permission was passed to the user, and after it is granted, inference on the same fleet resumes. For the CPU expert/DRAM bottleneck,
the cut/weight placement is also replanned under the KV-first condition. The MI 6 arms are not extended to Hy3/multi-physical-host approval.
See [remote 6 arms, causal separation and next implementation](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#service-screen-placement).
The real-hardware figures below and the "next" of that time are history and do not replace this current order.

**Fix publication defects within the fixed window first (2026-09-11, local verification complete):** The full connection of the return budget is kept,
but the misjudged prefill request count and endless decode coalescing of the already allowed pipeline policy are fixed first.
This does not allow new resident/open/fragment caps and is not B2/B3 completion or promotion of a time-based cost policy.
In the experimental ordinary-attention policy, the prefill plan is not blocked by the decode minimum request count, and the wait of a decode-only cohort
is limited by a monotonic clock. Expiry wakes the actual Worker receive wait but does not substitute for native/KV/flight authority.
The existing policy and atomic/equal paths are preserved. In the actual Worker loop and capacity1 completion, the last independent
prefill publication, decode wait expiry without new input, and the existing caps, settlement and release are verified. After that, the return budget connection continues, and
flight expansion, the cumulative prefill time policy and actual 100k approval remain open. workspace1404/0/7 (58 summaries, exit0),
actual consumption failures 2/1 for 2 independent mutations, and docs-lint94 clean were confirmed. The decode-only wait rechecks publication eligibility after 2ms
and is not a bound on native/OS latency. See [counterexamples, verification and seal](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#phase-pacing).

**Owned return delivery boundary (2026-09-11, local verification complete):** `RetainedEventBroker` and `RetainedEventNode` were
connected to an explicit owned adapter boundary. In actual queue1 consumption, concurrent held input/output, independent front progress,
original/claim preservation under Full, closed, duplicate and registration change, and retirement over 32 repetitions are checked. The destination queue slot and
retained bytes are reserved together, and receiver dequeue is not a byte return. The receipt is an independent exact copy.
The product composition root, llamacpp Worker, connection writer and receipt byte cap are still raw/unconnected, and
this boundary implementation is not marked as B2/B3 completion or GPU performance approval. workspace1414/0/7 (58 summaries,
exit0), 3 independent recompile mutations with 7/1/1 failures, docs-lint94 clean. Next is to actually connect the same owned boundary to the Worker and
the control/connection writer, and close the required return/independent receipt advance reservation.
See [plan, consumption tests and verdict](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#retained-broker-node).

**Actual llama.cpp Worker owned connection (2026-09-11, local verification complete):** The common loop of the same `Worker::run` was connected
to the owned adapter. The current input, the input held during Full and the delayed ACK original are kept together with their claim, and
after an abort the original/unprocessed receiver/state/effect remain with the adapter owner. On the actual two-stage path broker→EventNode→
llama adapter→Worker, the existing `ordinary-2` golden and a four-request wave, release and UNLOAD are checked.
This is a local consumption test that replaces only native computation, not a GPU/remote run. The product root/control/writer, future return and
independent receipt byte reservation still remain. workspace1416/0/7 (58 summaries, exit0), 2 actual consumption tests,
3 independent recompile mutations with 1 failure each, docs-lint94 clean. See [consumption counterexamples and verification](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#retained-worker).

**Design conclusion for long multi-prefill:** Batch row width, independent request cohorts and the concurrent flight target are each selected separately.
pure-prefill keeps an efficient large row width while leaving other requests for the next publication. Once decode starts,
the amount of added prefill is limited by the expected completion time including work already queued in front of each stage, and prefill gets
cumulative service deficit/aging and overload admission limits. If a slow stage stays busy, no more flights are
stacked, and cut/weight offload is replanned under the KV-first condition. This full cost policy is not implemented yet, and
its first unit, the per-stage prefill service backlog budget, is verified separately below.
After the return lifetime connection, the order is cost instrumentation → policy consumption counterexamples within the fixed window → comparison with the same actual 100k inputs/long outputs.
See [cause, decision formula and experimental verdict](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#long-prefill-design); existing runs are not recast as 100k or GPU saturation approval.

**Product event runtime owned connection (2026-09-11, local verification complete):** The entrypoint's root/control/TCP
actually select the owned broker/node/adapter. A control Full retries the same original, and a permanent failure
preserves the input, response, unprocessed input and node owner. A socket write failure preserves the current original and the queue not yet written
and does not replay automatically. Deletion briefly blocks ingress and confirms retained count0 for inputs/completions.
Remote ACK, raw frame/serialization scratch and future native/effect/receipt byte reservation still remain.
workspace1424/0/7 (58 summaries, exit0), 6 independent recompile mutations with 1/2/1/1/1/1 failures, docs-lint94 clean.
On an actual product agent→native Qwen1.5B CPU 2-stage, 4 requests of resident2/2wave passed EOS, completion, release and UNLOAD/DELETE.
The hashes of native451, the model and the run agent/driver, and the raw response text, were sealed. This is not full approval for GPU/100k/sentence-count instructions.
After this connection, the first next action is to close that advance reservation and the full return lifetime. Then proceed to
the cumulative prefill time policy within the fixed window, actual 100k and the two-cluster comparison.
See [actual TCP, failure counterexamples and verification](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#retained-runtime).

**Per-stage prefill service budget (2026-09-11, local verification complete):** Actual stage Frame time feedback was connected to head selection
within the existing finite window. When the per-stage expected cost of incomplete prefill + candidate cost exceeds the budget, only ready decode
is replanned. pure-prefill width is kept, absence of samples shows up as cold, and minimum service after earlier prefill settlement shows up as
a progress probe. It is disabled by default and does not complete the full decode deadline, dynamic row width, per-request time aging or
B2/B3 advance reservation. workspace1430/0/7 (58 summaries, exit0), 5 independent mutations with 1 failure each, and
4/4 EOS, release and UNLOAD/DELETE on 2 actual product TCP agents/CPU native 2-stage were sealed. The 16 cost overruns were
actually published as prefill0/decode≥1. This is a deliberate service1ms functional test, not a performance improvement rate or recommended value.
The first full test's 1429/1/7 was a call entry/completion race in an existing fixture, fixed with a completion wait that keeps the expected value.
Next, the new cost policy is screened with fixed-window comparisons on the two actual clusters, cold/prediction error is checked, and it is extended to chunk selection and
full completion-time prediction. The condition of closing the return advance reservation before allowing additional resources is kept.
See [decision, counterexamples and verification](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#stage-service-budget).

**Return lifetime instrumentation progress:** The broker's exact Event copies are classified as indexed/retired/allocated, and
bytes, retirement and final release amounts are connected to the actual INSPECT control response. Queue dequeue, ledger retirement and the last
reference release are observed separately. This follows the ordinary/reserved completion dispatch and actual control loop counterexamples.
workspace1401/0/7, 2 independent mutations with 2 failures each, and a send/receive raw-text comparison of the remaining payload 67112267B after 64MiB processing
on actual local TCP were completed. This does not mean a GPU real-hardware run or B2/B3 completion.
This is the receipt observation unit within V1.1-0, not a B2/B3 byte limit or early deletion. Next is to use these values to
attribute the actual retained storage and connect the actor queue/return reservation. Verification results are in
[receipt lifetime instrumentation](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#receipt-memory-observation).

**Single-main operation (2026-09-11 user instruction):** The sole baseline for long-term development and releases is `main`. Per-model development branches are not operated.
Merge `429e057de` integrated the temporary Hy3 upstream/memory/physical-wire compatibility changes with main's hardware query, path and Linux link fixes, and was pushed.
Model, cluster, policy, workload and runtime identity are independent settings of the common composer in `test/benchmarks/cluster-inference/`. Full unification of the deployment/lifecycle/deadline runner is follow-up work.
Past measurement source/binary hashes are kept as immutable evidence, and real-hardware runs not redeployed with the integrated source are not marked as approval of the integrated main.
After the integration gate and push, the merged temporary branch refs were cleaned up. Only main remains locally and on GitHub, and the existing worktree is preserved detached at the same commit. Later experiment results and fixes go into main.

**Implementation progress (2026-09-11):** V1.1-0 selection-time diagnostics and actual OUTPUT receive time, and V1.1-2's
experimental per-phase request/row caps, were implemented. This is not V1.1-0 as a whole or completed V1.1-2 promotion.
The V1.1-1 B2/B3/receipt budget is unfinished, and the resident/open/fragment windows were not expanded.
The new policy is disabled by default and applies only to ordinary attention. GPU real-hardware screening on the two clusters was performed, but H5 performance/service approval is incomplete.
At batching implementation time, the gate was workspace1384/0/7 with mutation failures 1/5/3/1. The integrated main gate is **1389/0/7** (58 summary), Node21/21, CPU Release CTest15/15, docs-lint94 clean. The CPU gate is not extended to GPU approval.
See the [implementation and verification record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#bounded-implementation).
Next is to close V1.1-0's unpublished intervals/per-request reasons, monotonic-clock issue→settle and byte lifetime instrumentation, and then perform V1.1-1.
Before that, this selection candidate is not promoted to the default policy and the flight window is not increased.

**Concurrent real-hardware screening (2026-09-11):** Actual inference on MI250 two hosts/16 stages and Hy3 five hosts/6 stages ran in parallel.
Hy3 went from **5.32→9.55 TPS** and ITL p50 **1.179→0.533s** at decode cap 0→2, with 8/8 completion, release and UNLOAD on both sides.
MI250 regressed with the default CPU setting: cap4 failed and cap8 gave 8.35 TPS (baseline 28.54/29.73). cap4/min4 also failed although its width was stable.
The cap4/min4 candidate using native CPU threads4 gave **34.05 TPS**, ITL p50 **0.319s**, and
16/16 completion, release and UNLOAD. The same CPU4 baseline group completed at 13.59 TPS, but an external Python GPU job
overlapped, so it is **excluded from the batching-only improvement rate comparison**. Promotion of an improvement rate is BLOCKED until re-verification in a GPU-interference-free time window.
Hy3 is also a single-pair screening result. It is a short load with context 100k/output 256, not approval of actual 100k prefill, normal EOS or continuous waves.
MI used source d5256af44; Hy3 used b9deee4ce, which kept fleet compatibility, and the same existing Mac downstream agent.
See [raw data, figures, failures and hashes](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#dual-cluster-screening).

**Long input, long output real-hardware run (same date, separate workload):** Within the user's 2-hour limit, Hy3 5 hosts/6 stages and
the unoccupied MI250-B 1 host/8 stages ran in parallel. Hy3 gave 18809 generated/3834.099s=4.9057 TPS,
8/8 EOS, completion, release and UNLOAD, and leading calculation 8/8. The MI cap4 candidate gave 51990 generated/1315.775s=39.5128 TPS,
16/16 completion, release and UNLOAD, EOS14/length2, and leading calculation 2/16. Overall response quality is unapproved for both models.
Context was 100k, but the longest actual input was 42154 tokens on Hy3 / 42413 on MI, and no actual 100k prefill was completed.
Hy3 TTFT max 37.13 minutes, MI candidate agent peak RSS 30.934GiB and 30.645GiB remaining after UNLOAD were confirmed.
The MI candidate's ITL p50 before/after all first outputs was 2.669/0.203s, so long prefill has a large impact.
The same MI250-B cap0 baseline ended at the common cutoff with 2/16 completed and released, and 47218 partial outputs were preserved.
The time at which all 16 requests had received their first 1024 generated tokens was cap0 940.498s→cap4 591.322s. This is a common-prefix diagnostic, not a completed-run TPS improvement rate.
The baseline also had leading calculation 1/16, so its quality is unapproved. The full comparison and termination errors are preserved in the evidence document.
For the denominators of the figures, same-host comparison, content failures and seal scope, see the
[long real-hardware record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#long-output-20260911).

**First next implementation after the long real-hardware run:** Bind V1.1-0's live/retired receipt bytes, enqueue/dequeue, unpublished reasons and
head monotonic-clock issue→settle to the actual consumption path. Worker-internal ingress is not a measurement of queue wait before dequeue.
Then close V1.1-1's B2/B3 reservation and broker byte retirement together with duplicate/replay/unknown-outcome preservation.
The heap cause is not settled merely from cumulative payload being close to RSS, and it is not bypassed by simply deleting receipts or raising resident.
After that, prefill quantum 64/128/256 and the decode service reservation, aging and tail coalescing deadline are evaluated on a single axis within the fixed window.
MI's arithmetic failure is separated from the distributed-path impact using a same-prompt/native quality baseline. Promotion of new defaults and flight window expansion are on hold.

**Next work determined by these results:**

1. Add **per-host CPU budget/run-queue wait, native CPU compute, spin, graph preparation/reuse** to V1.1-0 instrumentation.
   cap4min4 was slow even with fewer graph resets and completed with CPU4, so the cause is not pinned on graph misses alone.
   The deployment plan states per-host budgets to avoid a configuration that assigns the whole host CPU to each native process by default.
   threads4 is a diagnostic value from this run and is not propagated as a global default to Hy3 and others with heavy CPU expert offloading.
2. Instrument native bind errors/errno and stdin liveness join for stops before READY, and verify bounded failure and child cleanup
   with occupied-port/restart counterexamples. The product defect is not closed by only changing temporary ports.
3. After closing V1.1-1 B2/B3 and receipt lifetime/termination, proceed with V1.1-2's cost-based cohorts and fairness.
   Cohort size is evaluated not only by stage overlap but also by host CPU budget and actual native service time.
   More flights are not allowed based on GPU/RPC utilization alone, and node execution 1/decode outstanding ≤1 is kept.
4. Re-screen the Hy3 cap2 and MI candidates with explicit CPU budgets by repeating the same conditions **in a time window with confirmed GPU non-interference**.
   Per-process GPU lists/memory, CPU usage and external job start/end are sealed together. Only promising candidates get actual 100k prefill/
   decode mixing, continuous waves, long normal responses and H5 paired 8 pairs/holdout 4 pairs. The new policy default of 0 is kept.

**The current development order moves to this section.** The "next" items of that time in the v0.9.0/P/U records below are not made serial preconditions again.
This analysis does not change whether v0.9.0 is sealed, nor mark v1.1 implementation as complete. No version bump/tag yet.
The evidence from audit HEAD `11dc7a0ce`, MI250 run `f3658f1b` and Hy3 adapter/native `9ad366f9063` was merged into the
[integrated diagnosis](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md).
TPS from different upstreams, models, backends and workloads was not combined. The verdict contract follows the
[v1.1 verification applicability table](distributed-batching-verification.md#v11-gates).

**Fix goal:** Keep 1 backend execution per node while flowing independent batches through multiple stages.
Separate the batch composition and service time of prefill and decode to reduce latency propagation from long prefill, and
keep admission/transfer/return/receipt memory from growing without bound with request volume.

| Measurement/code basis | What the plan changes |
| --- | --- |
| MI250 16-stage: max concurrent RPC 1 on each of the two machines; 16 requests consumed in one cohort | Control decode request cohort size separately. max_issue_rows=64 also holds all 16 decodes, so this alone does not solve it |
| Both MI250 and Hy3 had 0 issue gate rejections; MI250's effective open setting unsealed | Do not conclude the open-batch cap is the cause; first instrument the actual eligible/blocked/flight causes |
| Hy3 decode interval median 1.869s → about 30s during prefill intervals; about 2× TTFT difference for requests of the same length | Introduce prefill service time budget, per-request quantum and cumulative fairness |
| Hy3 agent RSS and the expected size of keeping payload twice are close at about 20GiB; heap attribution unconfirmed | Separately fix not only active credit but also the byte retention/retirement contract of completion receipts |
| 8-stage 301.63 and 16-stage 7.64 differ in conditions such as resident256/16 and short input/100k mix | Do not approve as node-count loss rate, optimal value or useful TPS improvement; fix new per-topology baselines |

#### Implementation order and exit conditions

| Phase | Change unit / owner | Exit condition |
| --- | --- | --- |
| V1.1-0 measurement binding | OUTER/driver and adapter trace: effective configuration, per-request admission, eligible and blocked reasons, head monotonic-clock issue→settle and flight count, stage queue/native/forward breakdown. Bind an actual OUTPUT receive-time collection path on MI250 | Request cohorts, actual ITL, flight/byte lifetime and unclassified time can be recomputed from the same artifact. No global overlap/hop cost is settled without an inter-node clock error bound |
| V1.1-1 budget and termination | adapter B2/B3 admission and return reservation; core broker has a backend-neutral byte/receipt lifetime contract. Separately limit pending prompt, KV/auxiliary state, transfer payload, output and completion receipt. Bind settlement/reclaim for normal, rejected, partial transfer, cancellation and timeout | Exceeding a limit is rejected before side effects, duplicate replay semantics are preserved, unknown outcomes are preserved. Control progresses even at cap1, live/retired budgets are stable after long repetition, UNLOAD after all stages release. Required before expanding resident or the flight window |
| V1.1-2 batch composition | adapter scheduler/drive: add request cohort selection corresponding to `decode_member_cap` and a prefill row quantum as independent policies. ready decode service reservation, per-request cumulative service deficit/aging, prefill time budget from per-stage observed cost | Different decode cohorts are published with the same 16 requests. No starvation of either decode or prefill even with slow prefill. Immutable issued membership, decode outstanding ≤1, KV prefix and atomic verify/replay are kept |
| V1.1-3 pipeline window | adapter: verify prefill fragments 1→2→4→8 in steps. Node execution credit=1, global open-batch credit=N, edge row/byte and receiver credit kept separate. If needed, split the worker state machine so control/delivery can be handled during native work | Stage order and settlement order of fragments of the same sequence, and ledger/KV safety under out-of-order arrival, duplicates and cancellation. Experimental defaults are expanded only after confirmation on the actual consumption path/mutations. No concurrent calls on a shared context |
| V1.1-4 real-hardware promotion | Short comparison experiments per fixed topology → candidate selection → 100k continuous waves/long normal responses, offloading, long repetition. CUDA/Metal Hy3 and ROCm Step results judged separately | Pass the matrix below and the H5 repetition, holdout, SLO and quality gates. Failure raw data and cleanup preserved. Unsupported backends/models/features are explicitly unapproved |

V1.1-0 is the **first next implementation**. The first deliverable is the actual knob/request state/receive time/flight trace of the two sealed baselines, and
a consumption-path test that verifies that trace. The existing UTF-8 69–113-token failing input is also preserved and reproduced to bind the root-cause fix and
the Korean long-form regression. Success with 64 tokens/English is not read as a fix of that defect.

V1.1-2 starts within the fixed safety budget, and V1.1-3's window increase comes after V1.1-1 and fragment safety.
This plan contains no change that removes `queue.is_running()` or `outstanding>0` wholesale.
No call of the former was found on the current event path, and the latter is a decode dependency.
When changing the structure that waits for completion of per-step synchronous calls, the prepared execution authority, ledger commit and external effects are verified separately.

The draft implementation of batch selection is as follows. This is not current implementation fact; the policy is settled with counterexamples.

1. Build legal candidates from per-request KV prefix, unreturned fragments and reservation state. This is distinct from the simple total of ready rows.
2. Do not insert every request each time; select per-phase request cohorts. Apply the decode cohort cap and the per-request prefill quantum
   independently, and also check whether a mixed batch again occupies every independent request.
3. Allocate shares by the decode wait target and prefill's cumulative unserved amount/aging. If there is no ready decode, prefill uses that
   share. Predict the next native work time from recent per-stage cost as well as row count.
4. Publish only as much as satisfies node execution, total flights, edge/receiver bytes, KV/auxiliary state and the return budget together.
   Other cohorts are left immediately selectable at the next issue opportunity, and there is no unconditional wait to fill the batch.
5. Commit issue membership/token range and reservation atomically. Returns are handled individually according to stage completion and settlement authority, and
   separate lifetimes are tracked through output, release and receipt retirement. KV is not reused on a transfer credit return alone.

The prefill time budget is an expected target for choosing the size of the next job. It is not a guarantee of preempting the native kernel during execution.
The difference between expected and actual cost, the longest prefill wait and decode ITL are recorded together to detect starvation from large chunks or decode priority.

#### Experiment matrix — starting with small discrimination experiments

Each row changes only the one policy it names. Model, cut, backend, KV/offloading, resident, arrival sequence, sampling, output termination conditions and
logical/physical batch caps are fixed. Load time and inference time are separated. Short discrimination arms are designed with an advance 15-minute inference cap,
and failures/incomplete runs are preserved. The 3 repetitions are for candidate screening and do not replace performance approval repetitions.

| Search axis | Initial range | Refutation to check |
| --- | --- | --- |
| Independent decode cohorts | 16/8/4/2 requests at MI250 resident16; 8/4/2 at Hy3 resident8 | Even if smaller cohorts increase overlap, do TPS/SLO worsen through lower kernel efficiency? |
| prefill quantum | 64/128/256/512 rows, selected decode cohort fixed | Do long native steps and decode waits shrink, and do prefill TTFT/fairness worsen? |
| Per-request prefill window | 1/2/4/8 fragments, quantum/total window fixed | Does it eat into other requests' KV/edge budgets, and is same-sequence ordering safe? |
| Total open window | Values among 1/2/4/8/16 that the byte/memory budget allows | Do actual flights reach the limit, and does an extra window grow only queues/memory instead of throughput? |

The user-proposed `(fragment, issue rows, open)` settings `(2,256,2~4)`, `(4,128,4~8)` and
`(8,64~128,8~16)` are kept as a **combination search** after the single-axis discrimination above. None of them is an optimal value, and
the decode cohort cap is separate. Not all combinations are run for 6 hours each from the start.

The final load distinguishes (1) many short decodes mixed with 100k prefill and (2) continuous waves of several actual long inputs.
100k prefill completion is not claimed from a 100k context setting alone. KV is reserved on the device first, weights are placed in the remaining budget,
and CPU offloading is allowed, but planned/actual peak RAM and VRAM and rejection of insufficient configurations are verified.
Mac unified memory is not double-counted as two independent RAM/VRAM resources.

An average of 4–8 concurrent RPCs is only a search target for 16-stage, not a release gate or a declaration of GPU saturation.
H5's minimum of 8 paired pairs and 4 holdout pairs, median useful TPS improvement ≥5% with 95% CI lower bound >0,
TTFT p95 ≤1.10× and ITL p95 ≤1.05×, and the advance absolute SLO apply unchanged.
Fixed-length/ignore_eos loads are separated from normal EOS and content quality approval.

#### Scope boundaries

The required scope of this version is measurement binding, budget/termination, fair per-phase composition, verified bounded flights and real-hardware runs of the declared configurations.
Early physical capsule transfer is a separate candidate that changes the native codec/settlement contract. The scope is re-examined only if, after the changes above, a trace shows
waiting for full capsule return remaining the main cause; otherwise it moves to the next version.
Parallel sampler/shared context execution, expansion of unverified recurrent/hybrid fragments, the full persistent KV/K gate, and
approval of all models/backends are not added as automatic completion conditions of this fix. Anything outside the supported scope is disabled by default/unapproved.

The following is earlier execution history. Candidate files, failures and not-run records of that time are preserved and do not overwrite the new order of this plan.

### Historical v1.1 cluster comparison preparation (2026-09-11)

This isolated experiment tree retains the Hy3 fleet base `1a848a716`, including
its explicit physical-wire-v4 compatibility checks and approved OUTPUT receipt
timing, and ports only the bounded adapter selection/observation change from
`d5256af44`. Driver fixture initializers set the optional scheduling field to
None. It does not replace the fleet native binaries or relax identity checks.
The source is identical for baseline and candidate; only decode-member caps
change (Hy3 0 to 2, MI250 main source 0 to 4). Resident, native batch limits,
prefill fragment count, model cuts, KV placement and offloading stay fixed.
Short screening uses 256-token length termination and a 15-minute inference
deadline; it is not normal-response quality or H5 performance acceptance.
Combined-tree validation: `cargo test --workspace --no-fail-fast --locked
--target-dir F:/dev/p4/target/v11-hy3-tests`, exit 0, 1385 passed / 0 failed /
7 ignored, 58 summaries. Runtime comparison remains pending.

## 0. Current status — candidates preserved, separate fleet and upstream integration in progress

### 0.-6 Latest user instruction — MiMo on hold, run with Hy3 (2026-09-10)

Because the user chose Hy3, the MiMo loader fix/exception approval is not a precondition of the current run.
With Hy3 Q5_K_S 191.884 GiB, 5 hosts 6-stage, 102400 per session, resident8 and KV pool 819200,
the short consistency run `hy3-100k-smoke-lowport-1789027462065` achieved **2/2 EOS, completion, release and UNLOAD**.
The full text of the two power/energy problems showed limits in number, unit and temperature judgement. error/cleanup_error are null.
The topology/shape of the six stages and model/context/compute plan = actual allocation were confirmed.
Long inputs, 8 active sessions, slot reuse, saturation and performance are not yet approved.

The cuts are M42 [0,16)/[16,32), Spark [32,59), Mac [59,62), local 3090 [62,78), Ubuntu [78,80).
CUDA KV q4_0/q4_0, Mac Metal f16/f16; batch512/UBATCH256. After securing KV/compute on the device,
the routed experts of 15 layers each on M42, 15 layers locally and 2 layers on Ubuntu are placed in RAM. Unified memory is not double-counted.
The r16 local plan with the same cut was rejected. This is not generalized to say r16 is impossible with other cuts.
The first actual LOAD failed with EINVAL on the Mac native53021 connection and was preserved. A dedicated agent was started fresh and
the retry used native23021/23022. An ephemeral conflict is a candidate cause, not approval of reconnect durability.

The long single run `hy3-100k-single-1789029008524` also went 31643 input → 2316 generated → EOS, release and UNLOAD.
The arithmetic, units, citations and alert verdicts of the four records are correct, but because of mixing alert thresholds with safety limits, overstated causal wording and so on,
**overall response quality is unapproved**. The actual 1418 words are shorter than the requested roughly 1800–2500 words.
TTFT 1162.465 seconds, generated token receive interval p50=513ms/p99=691.6ms. 123 of the 124 prefill UBATCHes had 256 rows.
The single request's decode of 1 row, and the overall fill ratio computed by summing it, are not read as a bottleneck.

The next 16 requests are sealed as a **diagnostic load**. The same prompts, including inputs with unapproved quality, and
the minimum 1024 generated, EOS and consistency criteria are kept, and normal response approval is separated from raw throughput.
Input lengths are 31643/63682/91696 tokens, 966748 tokens and 4614390 bytes in total, and up to 99888 tokens together with max8192.
8 requests are submitted at 8-second intervals from 0–56 seconds, and the next 8 at 300–356 seconds. The inference limit is 6 hours and the observer cap 8 hours.
prefill_fragments default 1 and the outstanding dependency are kept, and arrival offsets create opportunities for independent batch progress.
OUTER's receive time per approved OUTPUT includes transfer effects and is not the GPU completion time.
Weights placed in RAM may still have large-batch computation moved to CUDA, so it is not assumed to be CPU computation.
Full-file hashes of the 6 model files and per-machine hashes of libraries during execution were confirmed. PCIe samples are also preserved in the next run.
An exact head ledger flight time distribution and a repeated saturation comparison are separately incomplete.
See the [Hy3 evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#hy3-100k-2026-09-10).

The 16-request run `hy3-100k-wave-diagnostic-1789033111493` confirmed 6-stage loading, plan = actual and library hashes, but
stalled during prefill and was **stopped as a diagnostic**. Delivered 16/completed 0/released 0, and all 82 UBATCHes observed at head had 256 rows.
While M42 call completion 41/41 did not grow for about 508 seconds, only the local 3090 showed high activity/PCIe transfer and 112 MiB free.
Memory pressure is the leading hypothesis, not a confirmed cause. The verification native was terminated deliberately, so the subsequent 10054
is an induced error. The partial artifact, first error, busy UNLOAD and missing evidence were preserved. Utilization over completed RPC intervals only
excludes the stalled intervals, so it is not quoted as overall inference utilization.

**Final verdict (2026-09-11):** `hy3-100k-wave-headroom-1789035735558` was run up to the original 6-hour
limit chosen by the user. 16 delivered, **4 EOS, completed and released / 12 incomplete**, and the 8 requests of the second wave had OUTPUT0.
The first error is deadline, and busy UNLOAD is a separate cleanup_error. 16438 approved OUTPUTs and partial outputs were preserved.
Missing request/execution row counts are not reconstructed, and raw/quality-approved TPS is not computed. The 16 calculation items in the full text of the 4 completed requests
are correct, but because of unsupported causal exclusions and requests falling short of the length (1110–1201 words), overall normal responses are 0/4.

For the 8 requests that had actual output, TTFT p50=94.85 minutes/p90=166.97 minutes; the other 8 were not observed.
Request arrival→completion p50 for the 4 completed was 309.34 minutes. All 2137 physical prefill-only batches collected by head had 256 rows,
325 of 328 mixed had 256 rows, and the 3783 decode-only had an average of 4.156 rows/max 7 rows.
**Prefill width was full, but the completion, latency and decode utilization goals for continuous requests were not met.** The full head ledger flight
time distribution and wait attribution are incomplete, so these numbers alone do not settle the cause among scheduler, transfer and CPU.

The six stages of v3, which moved the local layer77 experts to RAM, had plan = actual, and the local device requirement was
20203350016 bytes (KV15099494400). A minimum headroom of 2548 MiB over the whole observation window was kept and inference progressed.
However, the earlier run used a reused agent and this one a fresh agent, so **it is not a strict one-variable comparison, and the effect of the move alone is unconfirmed**.
Spark agent RSS went from 7.54→20.55 GiB after observation started, and minimum system available RAM was 2.73 GiB. The broker ledger that keeps full Events
under a count limit is a confirmed candidate for memory growth, but this is not proof of heap attribution.

The run source/input/binaries were kept until the end. After collecting raw data, only the relevant verification native/agent were cleaned up, and
the Mac-dedicated GUI agent was restored as new PID1560/52004 LISTEN. Process termination is not an UNLOAD success.
The existing work on the local 4080 was kept. This record closes the experiment and is not a product release or a declaration of reaching the final goal.

**First action for the next version:** Before repeating long loads, (1) close the byte budgets of broker/admission/return, safe receipt
retirement, and the post-deadline cancel→drain→release→UNLOAD path with actual consumption tests.
(2) Verify short multilingual normal responses on the same deployment. The fact that this English output had no UTF-8 errors is
not evidence of a fix for the separate MI250 69–113-token error. (3) With a fixed cut/initial state, measure short single→concurrent 2/4/8
step by step, attribute per-prefill/decode wait, CPU/transfer and head flights, and then re-run the same 100k arm.
Raising resident or changing Metal KV/cuts is done as a separate arm after passing actual memory/normal response comparisons.

### 0.-5 Earlier user instruction — largest successful model, 100k per session (2026-09-10)

The current priority is to load the largest of the models that previously had a successful actual LOAD, **MiMo-V2.5 UD-Q5_K_S, 201.446 GiB**,
on the existing M42, Spark, Mac .21, this PC and Ubuntu, and verify long input/generation waves.
Restoring TUF/Mac .20 is not made a new serial precondition of this run. The Mac-inclusive 122B result below is an earlier, separate success.

**Current verdict: RED before loading.** A 5-host 6-stage no-alloc plan was run with the CUDA and Metal binaries of sealed product source `9ad366f90`,
and all of them exited 7 with `sliding_window_pattern ... expected 48, got 51`.
MiMo's earlier success is LOAD evidence on the previous pin, not proof of support on the current pin. There is no new memory plan, actual LOAD, inference or TPS.
For the cause and raw data, see the [MiMo preflight verdict](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#mimo-100k-preflight-2026-09-10).

1. **Fix the loader regression first.** The new common loader reads the NextN count first, but the MiMo SWA array reader
   still expects the main-body layer count. The candidate fix is a one-line change of that reader's `n_layer()` to `n_layer_all`.
   The current candidate is not built and not applied. It can be adopted without conflicting with the current model isolation check only once an official upstream change/PR basis is obtained,
   or the user explicitly approves a local verification exception for this change. It is not bypassed with fake PR provenance, truncating the GGUF array,
   or relaxing the common loader's array length check. The completion conditions are 51/3 normal, normal without MTP, rejection of a wrong array,
   actual GGUF consumption and independent recompile mutations, CUDA/Metal, and the existing 122B regressions.
2. **Plan 102,400 tokens per session and resident 8/16/32.** This is the input+output limit, and the total KV pool is
   819,200/1,638,400/3,276,800 tokens respectively. `context_size=102400`, `total_context_size=102400*R`,
   native `--ctx-size=102400*R` and `--n-seq-max=R` are verified together. The session count is not raised while leaving the pool at 100k.
   The initial cut [0,8)/[8,16)/[16,36)/[36,40)/[40,46)/[46,48) is an **unapproved candidate**.
   KV, compute and headroom are secured first, weights go into the remaining device space, and excess routed experts go into PC RAM.
   This is not read as permitting KV host fallback. The planned/actual context buffer locations are confirmed, and the host total of the two M42 stages is checked.
   Unified memory on Spark and Mac is not double-counted. Before an actual plan, the fitness of this cut and the concurrent session cap are not settled.
3. **Use meaningful long inputs.** 32 documents were checked with the real tokenizer at 31,645–92,165 tokens,
   and 32/32 are identical to the model's Jinja template. They are power/energy calculations and comparison reports that cite records at the beginning, middle and end, with an independent answer key.
   Maximum generation is 8,192 tokens (longest input+output=100,357), and complete EOS and at least 1,024 generated tokens are accepted separately.
   `length` or short answers are not turned into success. The 4 calculations, units, sources, leaps and conclusions are reviewed in full, and quality-passing TPS is separated.
4. **The measurement order is normal response → long single input → concurrency → sustained waves.** The short normal gate does not substitute for 100k load approval.
   Start with a comparison at resident 8/16/32 with batch 512/UBATCH512 fixed. After fixing an acceptable resident, compare the head agent's
   `P4_STAGED_MAX_OPEN_BATCHES=1/2/4/8`. This value is an agent environment variable, not a native NodeConfig environment.
   The `outstanding>0` dependency is kept. The length mix of the 32 inputs creates opportunities for prefill/decode to be ready concurrently, and
   at least 2 slot-reuse waves and 3 repetitions are secured. The input selection, arrival interval and reuse of later waves are sealed after the preceding instrumentation.
   Each run limits submission count/bytes, and no approval of full-service B2/B3 boundedness is claimed.
5. **Saturation is judged by actual progress.** Preserve per-prefill/decode/mixed physical width and the ratio meeting 512, the ready/eligible difference,
   the time distribution of flight count based on head issue→retirement, stage queue wait, RPC and transfer, per-machine GPU/RAM/VRAM and swap, and
   per-request TTFT, actual consecutive token interval, starvation and completion/release/UNLOAD. The current `StageSpan` alone cannot settle the exact head ledger
   flight count, so that instrumentation is an open prerequisite. Host clock error is also recorded.
   RPC overlap and GPU kernel active ratio are not substituted for compute saturation. The range where improvement stops within repetition variance is
   reported as a plateau of the tested range, not a global optimum.

This phase covers only execution preparation for the user's request and preservation of the actual RED. It is not a product fix, 100k loading or completion of a continuous load.

### 0.-4 Earlier user instruction — re-test including Macs (run ended 2026-09-10 16:02 KST)

**The CUDA and Metal mixed 122B run on five physical hosts — M42, Spark, Mac .21, this PC and Ubuntu — passed.**
A new CREATE/DELETE round trip succeeded on the same Mac agent PID78595/binary. This turn did not change Mac permissions, signing, executables or firewall, and the cause of the external recovery is unconfirmed.
After the small `gemma-five-hosts-cuda-metal-1789022821079` completed 8/8 with EOS, release and UNLOAD, 122B was run without an agent restart.

122B `122b-five-hosts-cuda-metal-1789022821180`: **32/32 completion, EOS, release and UNLOAD**, error/cleanup_error/evidence_missing null.
The cuts are M42 GPU0 [0,8), GPU1 [8,16), Spark [16,29), Mac Metal [29,37), this PC's 3090 [37,45), Ubuntu [45,48).
resident4, 8 waves of 4 requests each, max512, the explicit physical-wire-v4 candidate; Mac KV is f16/f16 and CUDA q8/f16.
The topology/shape of the six stages and host/device model/context/compute plan = actual allocation were confirmed. The candidate files loaded during the run also matched their per-stage hashes.

Inference window 283.612 seconds, decode 3,339 rows / **11.773 row/s**, TTFT p50 115.987 seconds and p90 218.912 seconds. LOAD/UNLOAD is excluded, and this is not a useful TPS approved as normal responses.
The full-text review of the 32 requests left the mechanical-friction analogy in 8 heat explanations. Overall normal response, sustained load and performance improvement are not approved.
The Mac global AGX Device Utilization average of 62.38% is a driver counter over 262 samples in its own RPC window. Its meaning differs from NVIDIA kernel-active samples, so they are not summed or read as SM occupancy.
What passed this time is the mixed run of this candidate, model and cut. TUF and Mac .20 did not participate, so this is not completion for all machines or H0–H7.

The four CUDA verification agents and five monitors were cleaned up, and the disappearance of the Mac native was confirmed. The Mac agent managed by another Codex was kept.
Product source `9ad366f90` and the sealed binaries are unchanged. Separate app/configuration changes in the main checkout were not touched; records are written only in the verification checkout.
108 raw data files are preserved in `F:/dev/p4-releases/mac-included-20260910-1602.zip`. Hashes, responses and measurement scope are owned by the
[mixed-run evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#mac-included-five-host-retry-2026-09-10).

The first next action is the cut/memory verdict including the non-participating machines after TUF authentication and Mac .20 NAS recovery. Separately, the normal response baseline, B2/B3, wait causes and sustained load must be closed.
Mac .21 communication recovery is no longer repeated as a blocking precondition. The following is the record from the earlier run.

### 0.-3 Earlier retry — full-machine re-test after 15 minutes (2026-09-10 14:03–14:40 KST)

The scheduled retry was performed. **The full-machine and CUDA/Metal mixed runs are still BLOCKED**.
Mac .21 received a new CREATE, but its response send at 14:31:28 was rejected with NECP/error65;
TUF .17 rejected SSH authentication, and Mac .20 has no SMB mount and no P4 agent. The Mac agent managed by another Codex was preserved.

42mob interactive login and S: access on M42 were restored. The sealed Windows runtime was deployed to a separate path and its 13 files were hash-verified.
The dedicated TCP 52004 rule was limited to that executable and 6 target IPs. On this PC, 52004 is an OS-reserved port, so an actual P4 round trip was verified on 51054.
The existing app, v0.9.0 files and block rules were preserved. The Mac rejection was not avoided by renaming the program or bypassing permissions.

122B UD-Q5_K_S was actually run on the **four reachable CUDA physical hosts**: M42, Spark, this PC and Ubuntu.
The five-stage cut is [0,8)/[8,16)/[16,37)/[37,45)/[45,48), resident 4, 8 waves of 4 requests each.
Run `122b-four-cuda-hosts-1789017057291`: **32/32 completion, EOS, release and UNLOAD**, error/cleanup_error/evidence_missing null.
Inference window 220.856 seconds, decode 3,362 rows/15.223 row/s, TTFT p50 83.673 seconds and p90 167.304 seconds. LOAD/UNLOAD is excluded from this time.
The full text of all 32 responses was read, and because of the mechanical-friction explanation in the 8 heat responses, overall normal response, useful TPS and service approval are withheld.
The average GPU samples in each host's RPC window are M42 two cards 20.2/20.6%, Spark 21.8%, this PC's 3090 22.9% and Ubuntu 7.0%. These are not evidence of SM occupancy, saturation or improvement.

The topology/shape and host/device model/context/compute plan = actual allocation of the five stages were cross-checked.
The preceding small arm preserved the first transfer failure/8 incomplete/UNLOAD busy, then all verification agents were started fresh and 8/8 completed and released.
Passing normally after a restart is not approval of peer reconnect durability. The product source was not changed this time, and past unit tests are not counted as re-run.

154 raw data files and their hashes are preserved in `F:/dev/p4-releases/all-hosts-retry-20260910-1436.zip`, and the files inside the ZIP were re-verified.
This is a local handover; the gate for long-term re-inspection on another machine is not met. For details, all failures and hashes, see the
[evidence document](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md#scheduled-all-computer-retry-2026-09-10).
The four verification agents, model processes and four monitors were cleaned up, and the scheduled re-test was stopped. The product source is still `9ad366f90`.

The first next action is restoring the **actual p4-agent round-trip response** on Mac .21, and restoring the TUF account/key and the Mac .20 NAS.
After that, judge the originally requested M42/Spark/Mac mixed small gate → 122B → all-machine waves.
Immediate attribution/preservation of transfer failures, reconnection and the normal response baseline are separately incomplete, and are not closed by a simple restart or raising resident.

### 0.-2 Additional user instruction — all machines and latest upstream (2026-09-10)

The v0.9.0 candidate and existing gates are preserved. The user connected Spark, TUF, two Mac minis, the Ubuntu laptop,
this PC and others to the NAS, and instructed: individual model runs → a large model run spanning all computers → a successful latest llama.cpp
update. This instruction takes precedence over the earlier single-host resource limit and the next-version waiting order.
The development checkout is `F:/dev/p4-fleet-20260910`, branch `codex/fleet-latest-20260910`, and the release candidate is not modified.

1. Confirm actual model reads from the NAS and each machine's tools, memory, services in use and ports. TUF SSH authentication,
   Mac .20 NAS authentication and M42 interactive logon wait for user input, and the remaining work continues.
2. Replay the compatibility patches on the latest observed pin `434ddbbc0`, and verify CUDA, Metal and CPU builds and actual consumption-path regressions.
   Verify the upstream replacement of the existing split input patch with boundary inputs. Replay alone does not mean adoption.
3. On each machine, actually run a suitable model from the NAS and record complete responses, placement, release and memory used.
   Unified memory on Spark/Mac is not double-counted as host and GPU.
4. Preserve the 19001 service in use and verify inter-machine connectivity on available P4 ports. Distribute the large model across all target computers with a legal cut and
   actual per-stage plans. Do not substitute a small model or success on some machines.
5. Judge by normal prompts, complete responses, continuous waves, actual placement, run IDs and binary hashes.
   Record existing constraints such as the absence of B2/B3 budgets, and keep service approval of raised resident as a separate gate.

For current facts and open gates, see the [fleet/upstream evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md).

The latest native build and CTest 16/16 passed on Windows, Spark, Ubuntu and both Macs. Final Rust is 1,378/0/7 (58 summaries), and Node is 159/0.
Saved-plan CLI compatibility, CPU_REPACK host accounting and double counting of shared no-alloc compute were verified with actual consumption and independent mutations.
Windows, Spark, Ubuntu and Mac .21 passed individual runs of NAS models. The local-copy diagnosis on Mac .20 is not NAS approval.

122B passed **8 × 4 requests = 32/32 completion, release, EOS and UNLOAD on two physical hosts, Spark and Ubuntu**.
Source `9ad366f90`, resident 4, inference window 115.422 seconds, TTFT p50 47.405 seconds and p90 88.278 seconds.
The inaccurate analogy and waiting in the wire-heating responses are noted, and overall normal response, service and performance improvement are not approved.
The average GPU kernel-active samples in each host's stage RPC window are Spark 83.9% and Ubuntu 13.5%. These are not SM occupancy or saturation.

The explicit `physical-wire-v4` candidate cross-checks upstream, patch and native codec/representation, and preserves per-stage identity.
The default exact-build rejection is kept, and it passed consumption tests, 4 independent mutations, and 8/8 runs on each of two CUDA hosts and one Metal host.
At that time, **the CUDA/Metal mixed real-hardware run was BLOCKED**. The Mac .21 kernel rejected the agent's external TCP with NECP/error 65.
Python/nc port connection success is not approval of agent communication, and the run at that time stopped before the CREATE response, so no inference was submitted. The later recovery of actual responses and the passing mixed run are updated in §0.-4 above.

The current resume condition follows §0.-4 above. The Mac .21 mixed run passed; TUF SSH and the Mac .20 NAS remain. M42 login/NAS and this PC's candidate communication path were also restored.
After access to the non-participating machines is restored, judge legal cuts, the small gate and the 122B waves on all target machines while keeping the existing rejection criteria.
Until access is restored, additional partial-host runs are not turned into all-machine approval.
The current candidate's source, per-platform runtime and 329 raw files of success/failure/mutation/memory data are preserved in
`F:/dev/p4-releases/fleet-20260910-wire-candidate`. The candidate manifest in the evidence document owns the hashes.
Experiment-owned agents/stages and monitors were stopped, and the existing app and v0.9.0 candidate were preserved. No formal tag/push was made.

### 0.-1 Closure status of this version (2026-09-10)

Code fixes, local verification and packaging are closed. **Formal release approval is distinguished from next-version development.**
A scheduled Windows Update restart interrupted the latest r256 real-hardware run, and after the restart there is no 42mob login session.
After login is restored, only the two remaining gates need a verdict. The 09-07 stop/WIP records below are history from that time, not the state of the current HEAD.

| Item | Current result |
| --- | --- |
| Fix commit | `3302591fc`: PLAN/ACTUAL split, Ninja Release/Debug passing, copying the CUDA runtime next to the actual server |
| Local verification | workspace 1,374/0/7, Node 146/0, CTest 15/15, docs-lint 91 clean, compat 26 valid, private headers 81 clean |
| New 0.9.0 agent/drive real-hardware run | smoke, 2B pressure, 35B r96 PASS, completion/release/UNLOAD |
| Remaining gates | r256 interrupted by OS restart (failure preserved), must_refuse not run — **BLOCKED** |
| Actual candidate archive | runtime/source/evidence ZIP, raw data of 10 runs, failure/success and mutation logs, SHA256SUMS/RELEASE-STATUS.json |
| tag / remote | Existing unpublished tag preserved in an archive ref; formal re-seal/push on hold. agent/stage stopped, existing deployment backup kept |
| Not achieved | H0–H7, multi-host, service approval, TPS improvement, approval of raised resident |

The first remaining action is **restore 42mob login and S: access → judge r256 and must_refuse with the same binary → final tag**.
For source, run IDs, hashes and candidate location, see the [release notes](release/v0.9.0.md) and the
[gate evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md). Failing inputs, caps and the judge are not relaxed.

After that, the **next version** starts with the remaining no-alloc model/backend matrix of P-3d item 2.
The per-stage host/device plan = actual cross-checks for the 2B and 35B r96/r256 are done; r160 and other combinations remain.
Next is B2/B3 admission and return budget → normal response baseline and cause trace → H5, and service evaluation of raised resident comes after B2/B3.

### 0.0 2026-09-07 evening — actual check results after the user's resume instruction (verified)

The user instructed "check for real, then decide to continue or change; report TPS, batch saturation and GPU utilization from measurements", so the unverified items of §0.1–0.6 above
were run. Raw data, hashes and commands are owned by the
[2026-09-07 evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-07-head-verification-and-3090x2-ladder.md).

| Item | Result | Status |
| --- | --- | --- |
| HEAD `2ed9b71d4` release build | Success | Verified |
| HEAD `cargo test --workspace` | As of 2026-09-07, **compile failure** (`issue_witness_tests.rs:409`, `RequestState` implements only `Deref`), 0 tests run. **Fixed on 2026-09-09 with `input_mut_for_test()`: 1,361 passed / 0 failed / 7 ignored** | Resolved |
| Workspace excluding that crate | 849/0/0 | Verified |
| Staged adapter in an independent worktree after a 1-line test fix | 512/0/7, **cap1 actor ring progress test passed**, cap8 passed | Verified (not HEAD itself) |
| cap1 candidate removal mutation (disabling `forward_independent_front`) | cap1 fails, cap8 passes | Verified (1 mutation) |
| Remote 3090×2, HEAD Release binary, same launcher as 09-04 | **Before quality verdict**, 35B VRAM-only 117.07 gen TPS (= 38,148 generated tokens / 325.868 s). The decode row rate of the same run is 116.87 row/s (= 38,084 / 325.868); the two are different quantities. Same as the 09-04 baseline of 116.9–118.5; 2B 2-stage 189.75, 31B dense 2-stage 71.59. 35B is 109.70 excluding 4 rejections | Verified (non-regression observation, not a paired A/B) |
| GPU utilization, batch saturation | Average GPU0/1 over the stage execution window excluding load and cleanup: 2B 32.9/38.1%, 35B 29.3/30.7%, 31B 42.5/34.4%, 35B offloading 21.8/21.6% (full capture 15–29%). 0% samples in the execution window 1.0–29.1%. 35B decode averages 15.80 rows, prefill 374.37 rows | Verified (not improved, idle cause not decomposed) |
| `pressure` 512 requests | On 2026-09-07, both hosts rejected UNLOAD with `unload is busy; active_owners=224/256`, and the first error was masked, so no verdict was possible. **Verdict completed 2026-09-09:** the first error is `stage control batch total receipt budget is exhausted` on the release/settlement path, and the UNLOAD rejection is its consequence. Release completion across all stages was blocked by a capacity limit. That budget was tied to per-command caps in P-2 item 5, and the same scenario passed 512/512/512 both times on 3090×2 | Judged (§0.7 P-2 item 3), fixed and re-judged (item 5) |
| RAM offloading arm (remote, expert→CPU `--no-mmap`) | 35B MoE 43.19 gen TPS accepted, judge 64/64 (ChatML), 0.37× the pre-quality-verdict 117.07. Qwen3.5-122B-A10B got as far as loading two stages (host 38.3/39.0 GiB), but the tail exited 5 at `stage_memory_plan.cpp:358` with host compute plan ≠ actual → **BLOCKED**, TPS not measured. S: mmap offloading stalled on page faults (1 stage execution in 30 minutes) | Verified/BLOCKED |
| Harness and drive defects (2026-09-08 review) | The first inference error is masked by the UNLOAD failure, `stopChild` misjudges a signal exit as a stop failure (reproduced in 13 ms), and stages of a failed run remain on the remote agent | Verified (defects) |
| `judge.mjs` semantic verdict | Heuristics for length, Korean ratio, terminology, repetition and stop. For 31B, 32/32 had thought markers and a `length` stop, and 3 passed with a truncated code fence | Verified (not semantic approval) |

**Decision: do not change the plan and continue B1/B2, but change the first action.** The candidate actually fixed cap1 and did not break
performance. However, (1) HEAD does not compile its tests, (2) `pressure` ends with UNLOAD busy and the cause cannot be
identified, and (3) throughput and GPU utilization are the same before and after this change. The resume order puts the following before item 1 of §0.5.

1. ~~Fix the assignment in `issue_witness_tests.rs` with `input_mut_for_test()` to restore the full HEAD `--workspace` count.~~
   **Done 2026-09-09.** 1,361 passed / 0 failed / 7 ignored, staged adapter lib 512 passed.
   There is still no `DerefMut`, and the product path did not change.
2. Fix the harness and drive defects that mask the verdict first. Make `run/mod.rs` preserve `InferenceResult::error` and
   partial results before UNLOAD, make `run.mjs::stopChild` accept a signal exit as a stop, and clean up
   remote stages on the failure path. Leave a failing test for each first.
3. Then re-run `pressure` to judge whether UNLOAD busy is a release leak or a normal rejection after an inference abort.
   Whether `stage_owners`/`stage_frontiers` fail to release after settlement, and whether the check in `2e9451a5c` is excessive, are
   decided by that re-judgement. Before the verdict, it is called neither a regression nor normal.
4. Fix the problem of harness `prefill_mix_35b_2stage` sending gemma turns to a ChatML model before H1 acceptance.
   H1 is not passed on `judge.mjs` heuristics alone. Marker and truncated-response checks are defined together.
5. The tail stage host compute buffer plan ≠ actual verdict (`stage_memory_plan.cpp:358`) that blocked RAM offloading expansion (resource step 3 of B6/B7)
   was resolved on 2026-09-08 by `0025-noalloc-reserve-size-max.patch`
   ([evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md)).
   **Only the load rejection went away; normal responses and throughput for the 122B class are still not measured.** Because of `p4-event-drive`'s 2-node minimum,
   a 1-stage offloading baseline cannot be expressed now.
6. Then proceed with §0.5 items 1–6 as is. GPU idle and ubatch fill are targets of B4/H5, not achievements of this candidate.
   Claiming idle requires **a trace that captures both ready legal rows and device idle** in the same analysis window.
   Full-capture utilization and phase-mixed ubatch fill are not that evidence.

### 0.1 Conclusion and the boundary of solid evidence

**The consistency foundation and reproduction of actual defects advanced. However, complete distributed in-flight batching, resolution of the recent fix's
deadlock, and useful TPS/GPU utilization improvement are not yet proven.** The growth in test count and WIP commit count
is not progress toward the user goal. After the last verification there was a stretch where only unverified work grew.

| Category | What can be stated with certainty | What cannot be stated |
| --- | --- | --- |
| Baseline `a9e1967fc` | Shared request settlement function, per-request fairness counterexamples, actual tail codec check. Rust844/0/7 at the time | Full return atomicity, completion of an actual distributed scheduler |
| Follow-up verification sources 09/12 | The settlement, ledger and worker consumption regressions below and bounded ACK progress. Rust1253/0/7 each | Deadlock freedom in the cyclic network, all resource budgets, a pass on the latest HEAD |
| Last run 13 | Rust1254/1/7, cap1 deadlock RED and a positive control with the same input at cap8 | A claim that the current fix candidate fixed the RED |
| 7 WIPs after the RED + current candidate | Code and tests are written and exist in Git/the working tree | Compile, regression and mutation passes, throughput/memory improvement |
| Final product outcome | Earlier small-model/35B single-host real-hardware data exists | Real-hardware results of this change, or final approval of very large models across multiple computers |

The last run was `cargo test --workspace --no-fail-fast --locked`,
2026-09-07 04:29:42–04:32:06 UTC, exit101, 57 summaries. The only failure is the
final `normal_progress` assertion of `event_actor_ring_saturated_normal_ingress_must_progress_without_external_dequeue`.
This result is attributed to the 400 inputs sealed by adding the actor counterexample in `f13e2560b`
(later counterexample-preserving commit `393a6c23e`). It is not a result of the latest HEAD.

Raw files `target/capacity-slice-20260907-13/workspace-result.json` and `workspace.log`,
source SHA256 `4a353e02335162a53371df334f8bd55b53742745392178cfb99ec1dc8ddb49eb`,
log SHA256 `a2889eef868a1b68ab732df88c7afd46ff4aa38416b0ad31848886eb318f9ce7`.
Detailed run/EXE/mutation data is owned by the [settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
The `target/` raw data is kept on this machine and is not long-term evidence reproducible on another machine.

### 0.2 Progress actually verified

The following is limited to the relevant counterexamples up to the last verification. It is not applied retroactively to the whole current WIP.

| Progress | Actual consumption path, representative tests/evidence | Remaining limits |
| --- | --- | --- |
| Full pre-validation of returns and publication cross-check | `worker/release.rs`, settlement/publication ledger; T10 ledger preservation, T11 full rejection on one member's error, and T12 no consumption of the next fragment from a previous execution passed in run 13 | Not complete through all replay/native side effects and crash convergence |
| Publication state and stage execution authority | Shared issue/settle, FlightLedger, Prepared/AwaitingNative/Uncertain, consumption regressions for incarnation, BindLoad, physical receipt and stage KV frontier | Not a proof of end-to-end credit/remote acceptance or persistent exactly-once |
| Protection of the actual model path for shared settlement | The malformed arrival test through `Simulation::advance` also passed in run 13 | The fake engine model differs from actual transport/memory cost |
| Expanded actual worker consumption tests | Regressions for general 2/4/8-stage, speculative 2/4-stage, OUTPUT after head approval, control/release identity and busy UNLOAD are in the default run | fake native/post-LOAD scope; not a demonstration of llama/CUDA/remote normal responses |
| Actual production/consumption binding of output and release | Consumer cross-checks of head OUTPUT, per-request RELEASE notification, publication witness, observation and string byte codec | Not a proof of peer/source authentication, natural-language quality or actual GPU placement |
| Bounded ACK progress during completion Full | `Worker::run`/`ack_service.rs`; genuine ACK test and Full non-ACK FIFO recovery, pre-rejection/post-intent preservation of the pending receipt ID obligation. Detection of 5 independent mutations recorded | Does not solve the cycle where the non-ACK front and EventNode/broker are also full |
| Locking down the deadlock in the actual cycle, not a unit function | `actor_ring.rs` uses the actual EventBroker→EventNode→adapter→Worker. cap1 stalls, cap8 completes the same input. Exact recovery after externally draining C1–C6 | Evidence of reachability of the deadlock cause, not success of normal progress |
| Handoff, layer contracts, Git preservation | Organized B/T/I/H/K responsibilities, event execution path, source/EXE binding and error counting rules; created checkpoints of cumulative changes | Document and commit volume is not execution evidence, and long-term preservation of raw data is incomplete |

### 0.3 Written but not verified

The 7 WIPs below after `393a6c23e` total 55 files +7589/-709 (including documents and tests).
The amount of source is not evaluated as progress. Each change has not been compiled, tested or mutation-tested, and whether to keep it
must also be judged in the next integration verification. User changes are not deleted just because there is no verified result.

| Commit | Change written | Current status |
| --- | --- | --- |
| `7f402aba5` | Owned completion store and reservation foundation | Actual full delivery connection incomplete |
| `bcbadf101` | Immutable preservation of committed outbound payloads | Written candidate |
| `d8fff7d27` | Separation of delivery slots and result retention space | Not a proof of full byte/RSS budget |
| `2b1d1d539` | Prepare/commit atomicity of PREFILL admission | Actual consumption regression written, not run |
| `658c9cded` | Returning the original on broker/adapter/node terminal rejection | Candidate for raw Event ownership preservation, not claim transfer |
| `f5aa09675` | Direct response FIFO and notification boundary | Written candidate |
| `6fe10eb10` | Immutable sharing of request input | Candidate for reduced copying, performance/RSS effect not measured |

The candidate stopped afterwards is a neutral EventNode/broker fix that **delivers the completion front of an independent correlation after securing an actual destination slot**.
The order of the same `(source, correlation)` is kept, and the payload is not
interpreted. Code connected through `NodeAdapter`/mailbox, Llama delegation, broker receipt, terminal original preservation and the actual actor wrapper,
20 regressions (broker12/mailbox4/EventNode4) and an actor prevention witness are written.
This is only a static connection check, not an observed cap1 pass. The corresponding section of the contract is also an **unverified candidate**.
The final verdict of 14 original inputs, 6 outputs, 8 SESSION_READY, cap1/cap8 and external dequeue0 is kept.

### 0.4 Why progress became unclear, and approaches that were stopped

- Before closing a single actual failure, generic ownership/memory migrations kept being added as preconditions.
  Even a potentially useful foundation change cannot count as resolving the current counterexample without passing the consumption path.
- Without distinguishing local function/ACK GREEN from whole-actor progress, the same kind of defect is found late.
  The latter is now locked down as RED, but WIPs accumulated afterwards without an integration pass.
- The "next" changed in each long chronological record, blurring the current state and the completion boundary. Current instructions are gathered
  in §0 alone, and history is preserved but not reused as an execution order.
- A last fix, no exceptions, or success within three attempts cannot be promised without proof. Instead, counterexamples, assumptions, owners and
  exit conditions are fixed before execution, and unexpected failures are not hidden by relaxing expected values or naming a new round.

### 0.5 Work after resume — priority of the existing B phases

**Not executed now.** After a resume instruction, proceed in the order below without adding new preceding refactors.
The exit conditions of each whole phase are owned by §6, test details by the verification protocol, and per-layer responsibilities by the isolation contract.

1. **Start with the verdict on the current candidate (B1/B2).** Cross-check HEAD/changes and the 7 WIPs against the new candidate, and explain which edges of the actual wait cycle
   it removes. Without adding slots, SESSION-only bypasses or deleting failing inputs, seal normal progress at the same
   actor cap1, the cap8 control, and order/original/duplicate/terminal regressions. Do not turn this into work that only builds more auxiliary APIs.
   The 3-round verification constraint remains at **2 used, 1 unused**.
2. **Close the remaining integration safety (B1/B2/B3).** Passing that cap1 is not general deadlock freedom either.
   Judge on the actual consumption path cycles within the same ordering domain, cancellation/Close/Drain, required result and return space during saturation, a capacity wake to replace
   the 1ms retry, queue/retained/byte lifetimes, and convergence of duplicates, reconnections and unknown executions.
   Undeclared native result bounds and remote acceptance are not replaced with arbitrary numbers.
3. **Complete the admission and flight budget (B3).** Distinguish pending/token/byte/KV cells and reserve them in actual memory.
   Decide the sole owner of multi-node reservations including prepared, and of edge row/byte credit.
   Lack of space must be a bounded wait/explicit rejection, and return, cancellation and timeout must be idempotent settlement. Slot counts or ID headroom are not
   called a host RAM/KV budget.
4. **Complete the batch policy (B4).** On an actual worker/reference that uses the same state transitions, verify full runnable
   reselection, ordinary/equal-width/atomic constraints, decode dependencies, chunked prefill and per-request fairness.
   `min_batch_rows`, `max_open_batches`, `max_issue_rows` and multiple prefill fragments are not
   assumed to be optimal values. Rows that are ready but unpublished are explained by reason, such as KV, credit, shape or dependency.
5. **Complete actual execution and the update boundary (B5, in parallel with safety work only as far as needed).** Confirm product LOAD enforcement of
   model/build/ABI/layout/capability, binding of actual devices and host fallback, declared backend conformance, and
   native private/common indirect include/link and public signature isolation. The pure ledger/policy must
   not see llama private types, and upstream adaptation must end in the allowed compat module.
6. **Then judge outcomes (B6/B7/B8).** On `S:\models` and the approved 3090×2, first prove strong continuous
   waves and normal responses for VRAM-only models, then extend to RAM offloading models. In topology-fixed paired
   A/B and holdout, judge useful generation TPS together with SLO and GPU computation, and pass soak/fault and repetitions without restarts.
   The current two GPUs are one physical host, and the final multi-computer proof needs separate resources.

Persistent KV/snapshots (the K branch) have foundation code, but the full target contract is incomplete. Connect the features the workload requires
to the fault gates first, and do not make all of the past U/P series serial preconditions again.
Single GPU/replica comparisons are only for cost diagnosis, not an approval criterion for removing distributed nodes that are needed for capacity.

### 0.6 Verification and reporting constraints for the next session

#### Real-hardware measurement is done only on 3090×2 (2026-09-09 user instruction, standing)

- **Measure on `m42-server2` (RTX 3090 ×2).** Do not measure on the development PC (`hikaTR`, 3090 + 4080).
  The card configurations differ, so values from the two machines are not mixed in the same column. The development PC is only for builds, documents and tests.
- **An SSH session cannot see the remote `S:`. But the apps we launch on that host can.**
  Drive mappings are separate per logon session, so even when someone is logged on to the remote host, the SSH session's
  `Test-Path 'S:\models'` is False. This is not about whether someone is logged on.
  So the agent is launched as an **interactive scheduled task** (`remote-agent.mjs start`), and the stage server inherits the `S:`
  that process opened.
- **The model path is the same as this PC's `S:` path.** That is, `S:\models\...` as written in the scenario can be used as is, and
  there is no need to rewrite the path for the remote host. If `S:` is visible here, the same string opens there too.
- When something needs checking, do not decide by asking SSH for `Test-Path`. SSH not seeing it is normal and
  is not evidence of whether loading is possible.
- If the host is at the login screen, interactive tasks do not run. Only then, as an exception, move the weights to the remote local disk
  and use `run.mjs --model` and `remote-agent.mjs --no-session` (S4U). **This is not the default path.**

- A new session reads §0 and the verification/isolation contracts first and compares them with the current HEAD. **Before the user's resume instruction,
  do not run the last verification round or add implementation.** Do not misrecord the current stop as BLOCKED/finally complete.
- First seal the source, inputs, verdicts and per-failure-cause removal mutations to verify. Do not edit the same checkout during a run.
  Verify mutations with an independent copy and actual recompile/EXE binding. Compile errors/timeouts are not counted as detection of the intended
  invariant. Not run, ignored and feature-excluded tests are not counted as passed.
- Report the full result together with cap1 normal progress, the normal control, order/duplicate/original preservation, and failure when the fix is removed.
  If an unexpected failure occurs, record its cause first. Do not relax caps, goldens, judges, inputs or starvation criteria, or
  restart the three-round budget. Do not assume the current candidate will pass.
- Each checkpoint distinguishes **verified/unverified** in its title and status. Preserve all required operational code, regressions and owned documents,
  and check for leftovers in the working tree. Models, binaries, builds and temporary raw text are ignored, and a clean Git does not
  mean tests passed or long-term evidence is preserved.
- A final outcome report must include normal prompts and full responses, strongly overlapping waves, source/binary/model/placement
  identity, and error, settlement and credit data plus useful TPS/SLO/GPU data. Do not write "optimal batching implementation complete"
  on the basis of local safety passes alone.

### 0.7 In-flight pipeline performance work order

This section owns only **the order of work that raises throughput**. Remaining safety work is still owned by §0.5, and test verdicts by the verification
protocol. The figures below were carried over by reconnecting, per request and per group, the preserved spans of `20260907T093641Z-5eeea50c` (remote 3090×2, 35B 2-stage, VRAM-only,
resident 32, ctx 2560/seq). The raw data's baseline is
`2ed9b71d4` + the preserved dirty diff, and the native patch set is `3cfc636181e4`. **This is not a re-measurement after `0025`.**

> **2026-09-09 correction record.** The first version of this section (`daae4c8fd`) got four things wrong.
> ① It cited an already-withdrawn correlation (r=0.891) as the basis for the width policy. ② It called a product of values from different sets
> an identity. ③ It read RPC interval overlap as concurrent GPU computation. ④ It recorded the result of an intended policy
> as a defect, and the 191 TPS computed on top of that as an expected gain. Below is the corrected version. Not deleting the wrong version
> and recording what was wrong and why follows the same discipline as the 09-03 evidence document.

#### P-0. What current throughput decomposes into (measured)

| Item | Value |
| --- | ---: |
| Wall clock T until release | 325.868 s |
| Physical batches (decode / prefill) | 2,459 (2,410 / 49) |
| decode rows / generated tokens | 38,084 / 38,148 |
| decode batch/s × decode mean rows | 7.395633 × 15.802490 = **116.869 decode row/s** |
| Generated tokens / T | **117.066 TPS, before quality verdict** |
| decode head RPC mean / tail RPC mean | 81.52 ms / 89.68 ms |
| decode idle mean | 49.36 ms |

**This is not an identity but a rearrangement of factors.** It only writes `38,084 / 325.868` as `(2,410 / 325.868) × (38,084 / 2,410)`,
and proves nothing about why that rate occurred. The previous version's `7.55 × 15.80 = 119` multiplied total batch/s by
the decode-only mean width, so it is the rate of no set at all. It is not used.

The sum of `stage_ms + idle_ms` is 322,479 ms (mean 131.142 ms), and `T / 2,459` is 132.521 ms, leaving 3,389 ms.
The difference mixes integer-ms truncation and the preparation interval between idle end and native start. The two values are not written as equal, and
this difference is not called a separate bottleneck either.

The offloading control (`20260907T102817Z-8438348c`, experts on CPU) had stage p50 233 ms, period 354.3 ms and 43.19 TPS.
**The prompt template differs, so it is not a causal comparison that changes only placement.** The two paths are not grouped under the same item.

#### P-1. What is actually known about width — the cited correlation was already withdrawn

The previous version excluded width from the goals based on the r=0.891 (overlap) and r=-0.357 (width) comment in `worker/drive.rs`.
The [09-03 record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-03-load-and-batching.md), which is the raw source of that comment,
retracts that interpretation immediately below: `Everything in the paragraph above is backwards.` The title of the follow-up section is
`The correlation was reverse causation (2026-09-04)`.

- In 8 crossover runs that directly limited width with `P4_STAGED_MAX_ISSUE_ROWS`, stage overlap rose to 95.4% and
  GPU utilization was also highest, but total row/s fell from 544.25 → 198.10.
- Controlling for width flips the sign: width **+0.898**, overlap **-0.060**.
- Fit: **tail step = 34.2 ms per batch + 1.051 ms per row.** The curve keeps rising up to width 98, and wide batches
  are 3.4× more efficient per row than narrow ones. Overlap rose because busy stages repeatedly paid the fixed cost.
- The breakdown of that fixed cost is also already measured. At the tail, **sampling at 46.9 ms is larger** than `llama_decode` at 40.1 ms.
  Per row, it is transformer 0.11 ms versus sampler 0.29 ms, so the sampler is 2.7×. The vocabulary is at least 249,157, and
  `common_sampler_sample` builds a candidate array of that size per row on a single thread.
  The instrumentation is still in the code (`server_physical.cpp:34`, `P4_STAGED_TRACE_STEP`).

**So the conclusion reverses.** Overlap and UBATCH fill ratio are still not goals, but width must not be dismissed either.
**The lever is the per-batch fixed cost, and the ways to reduce it are to amortize it over width or to fix the sampler itself.**
That comment was replaced with the 09-04 result in `2bc8f93cd`. No withdrawn citation remains in the code.

#### P-2. Measurement trust that must be restored first (same items as §0.0 items 1–3)

These are finished before starting performance verdicts, because all of them **make the results of performance experiments unreadable**.

1. ~~Restore HEAD lib test compilation with a test-only accessor.~~ **Done 2026-09-09** (`2e865ff51`).
   From the state of exit 101 with E0594 and 0 tests run, it recovered to **1,361 passed / 0 failed / 7 ignored**.
   The scheduler was not touched, with no regressions. Remaining items 2–3 are the remaining prerequisites for performance measurement.
2. ~~Fix `run/mod.rs` to preserve `InferenceResult::error`, partial results and settlement state before UNLOAD,
   and record primary/cleanup errors separately. Also fix the signal-exit misjudgement in `run.mjs::stopChild`.~~
   **Done 2026-09-09** (`d8203e373`).
   - Teardown was separated and its return type is `Option<String>`, not `Result`. Since it cannot be propagated with `?`,
     **reintroducing the original defect is a compile error** (confirmed with E0277). The type blocks it, not a test.
   - `error` (the run's own first failure) and `cleanup_error` were separated, and the artifact is always written.
     **The UNLOAD guard is unchanged.** A cleanup failure still fails the run, and a test with a positive control locks this down.
   - `stopChild`: `kill()` sends a signal, so even a normal exit has a null `exitCode`. In this environment it was
     reproduced as `code=null signal=SIGTERM`. **Every normal stop was reported as a stop failure**, so
     `agent_stopped=false` was failing runs.
   - 7 tests (event-drive 3 + stopChild 4). Each one's discriminating power was confirmed with mutations. Workspace 1,364 passed.
3. ~~Re-judge `pressure` (resident 256).~~ **Verdict completed 2026-09-09.**
   Right after the fix, it was re-run **twice on 3090×2** (with a staged copy of the weights, and with the scenario's original `S:` path).
   Both runs left artifacts, the two errors were separated, and the same failure was reproduced.
   - `error` = **`stage control batch total receipt budget is exhausted`**.
     The only product callers of `validate_control_batch` are `release.rs:324` and `settlement.rs:246`, so
     what hit the limit is **the release/settlement path itself**.
   - `cleanup_error` = `unload is busy`. Its work snapshot has **all in-flight work at 0**, and only
     `active_owners`/`active_frontiers` remain (224 and 256 in the two runs). Of the 512 requests, 96/64
     received a release member, yet `released` is **0 on both**. This is in the same range as the `224/256` of the preserved 09-07
     run.
   - **Verdict: the UNLOAD rejection is a consequence, not the cause.** Abandoned state is not leaking; **release was blocked by a capacity
     limit and could not start.** The limit is the combination of `MAX_CONTROL_BYTES` (1 MiB, reserving the worst-case response per row)
     and `MAX_RECEIPT_BYTES` (64 MiB, cumulative cap) in `ownership.rs`, and a control batch is **allowed up to 63 entries
     and rejected from 64** (the boundary when no receipts are retained). A full-width release at resident 256
     structurally exceeds this.
   - **Correction: `released_count=0` does not mean "release never ran".** The head actually performs its own stage's release in
     `CommittedEffect::Release` of `worker/effects.rs` and then
     forwards it, and that path uses a single-sequence individual check, so it does not hit the batch total check.
     The correct statement is **that release completion across all stages was not confirmed**. The work
     snapshot carried by the UNLOAD rejection is also the state of that one node, and that artifact cannot judge which stage progressed how far.
   - Undetermined: the command type and batch width rejected **in that run** (not in the artifact; 63 is the boundary of the cap accounting
     settled from the constants and wire encoder, not that run's width), why the completion count and owner count
     fluctuate between runs, and whether the preserved 09-07 run took the same path. [Evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-measurement-trust-recovery.md)
   - **Therefore §0.5 item 3 (admission and flight budget) is confirmed as a prerequisite for raising resident.** Raising resident before
     establishing the budget hits the same wall again. P-4's resident axis comes after that.
4. ~~Replace the withdrawn correlation citation at `drive.rs:130` with the 09-04 result.~~ **Done 2026-09-09** (`2bc8f93cd`).
   Only the comment changed, and `cargo check --lib` passed.
5. **Tie the control response budget to per-command caps.** Code, tests, mutations and **real-hardware `pressure` re-judgement completed 2026-09-09**.
   - The budget check was reserving `MAX_CONTROL_BYTES` 1 MiB for each new control rather than the actual response.
     Now each command declares **the cap its own contract proves**, and the individual check and batch total check use the same value.
     RELEASE requires a response identical to the request, so its cap is the request length; SETTLE's is `physical_capacity × 4 + prefix + 4`.
     `commit_control` settles with the actual length of the arriving response. The caps (1 MiB, 64 MiB) were not raised,
     resident was not lowered, and the budget structure of the common core was not touched.
   - The rejection message carries the member count, existing retained amount, additional reservation, requirement and cap, and the caller prepends the command type and width.
     From the next run, the rejected batch can be identified from the artifact alone.
   - 3 tests added (1 full-width batch at the production cap, 2 on the actual consumption path) and 2 existing ones updated.
     Workspace **1,367 passed / 0 failed / 7 ignored**. The 3 mutation rounds were confirmed in a separate worktree
     with recompile sha256 (individual only / total only / both → 2, 3 and 4 failures).
   - **Real-hardware re-judgement: 512/512/512 passed both times on 3090×2** (`20260909T034439Z-4ce8e2b1`,
     `20260909T035149Z-9014d441`). Both `error` and `cleanup_error` are `null`, so idle UNLOAD also passed.
     512 requests used 256 physical slots at 2 per slot, and all were released with incarnations 1–512.
     The staged server, DLL and launcher hashes are the same as the failed run, and **the only thing that changed is the adapter binary.**
   - **The verification scope is RELEASE.** Both runs had 0 Verify/Replay rows, and there is no settlement record in the artifacts.
     The evidence for the SETTLE batch path stays at the consumption-path test and mutation level, and its real-hardware verdict is still pending.
   - **This is not a performance approval.** `pressure` never completed before the fix, so there is no baseline to compare against.
     The TPS of this run is not cited as an improvement.
     [Evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-control-receipt-budget.md)

#### P-3. What was being waited on — alternating 30-row and 2-row groups

The previous version wrote "it idled with 86 ready rows". **That was wrong.** `ready_rows` is a snapshot at the next planning point and
includes all remaining prompt tokens, so it is not the number of rows publishable throughout the wait. Connecting the preserved spans to requests
shows a different picture.

| decode width | Count | head RPC | tail RPC | Preceding idle |
| ---: | ---: | ---: | ---: | ---: |
| 2 rows | 1,077 | 37.31 ms | 41.65 ms | 0.54 ms |
| 30 rows | 951 | 126.63 ms | 138.05 ms | 101.38 ms |

**The 32 resident requests split into two groups of 30 and 2 that are published alternately.** The idle between the 945 30→2 transitions
averaged 0.46 ms, and the 943 2→30 transitions averaged 102.16 ms. A mean of 15.80 rows does not mean uniform 16-row batches.

1. The 1,165 idles of 50 ms or more are **98.85%** of all idle time. Of those, the 1,164 decodes had
   `ready_rows == published rows` at publication (a positive remainder in 1 of 1,165). **No rows were left behind.**
2. These 1,164 started the next plan on average **1.36 ms** after the end of their requests' last preceding tail RPC, and
   **0.88 ms** after the tail forward. 1,162 were within 5 ms. **The scheduler was not late; there was
   something to wait for.**
3. Of the 120.941 s total gap between head end and next plan, **118.613 s (98.08%)** overlaps a tail RPC.
   **This does not mean GPU computation overlapped.**
4. During **105.188 s (86.97%)** of the same gap, **all 32** resident requests had published decode at the head, but their
   tails had not finished yet. Because `state.rs:351` makes the next row of a request with `outstanding > 0` unpublishable,
   there are actually no rows to publish in this interval.
5. Of the 2,458 batches excluding the first, **2,454** started the next plan before the previous batch's tail ended.
   "Publishing before the previous batch returns" is already happening. It is not listed as a candidate again.

**Priority-1 hypothesis: an asymmetric cycle where, while the large group is at the tail, the head finishes the small group first and then waits for
the large group's next token.** The evidence still missing is the full admission/eligible state across all gaps and native-internal
cost. Not all remaining gaps are classified as scheduler defects. The following are recorded on the same timeline of one run.

- Per request, `preceding tail end → forward → head receive → settle → eligible → next native start`.
- The eligible/admitted/in-flight counts and rejection reasons for every unpublished interval. `idle_gated=0` only means there were no rejections from
  the two gates, not that every unpublishable reason was measured.
- With the existing STEP trace, split head and tail into parse, decode, sampler and encode at width 2 / medium / 30.
  **Nothing new needs to be built.** The timer is already in `server_physical.cpp:34`, and the switch is `--step-trace`
  (→ `P4_STAGED_TRACE_STEP=1`) in `remote-agent.mjs:75`. Seeing causes at 1 ms granularity requires both a unit finer than
  ms truncation and a trace-overhead control.
- Measure after newly sealing a 35B VRAM baseline with the correct ChatML template and normal response checks fixed.
  The current 117.07 is a pre-quality-verdict value with heuristic judge 60/64.

6. **Preserve partial results of failed runs.** **Done 2026-09-09.**
   - The 4 failures of the 09-09 saturation experiments (2-stage load rejection 2, duplicate node 1, 4-stage inference abort 1)
     did not leave `artifact.json`. `inference::drive` propagated failures inside the loop with `?`, so the collected
     requests, outputs, completions and observations vanished entirely and never reached `assemble`. That is why **it is still unknown how far
     those runs got approved and what the first cause was.**
   - Now a failure after submission starts is a result. The first error is kept, what was approved is preserved, rejected
     events are not accepted, and the run still ends as a failure (`passed=false`, exit code 1).
     `submission` (delivered/uncertain), `evidence_missing` and a `submissions` summary were added to the artifact.
   - Tests that inject a connection cut, deadline expiry and an invalid follow-up event on the actual consumption path,
     a test that layers an UNLOAD rejection on top, a test that splits delivered/uncertain/unsubmitted with a write failure mid-wave,
     and **an integration test that attaches the actual CLI binary to a local TCP peer to lock down `execute → teardown →
     JSON write → exit code 1`.**
     Attribution follows the evidence — if the evidence is complete, row counts are attributed regardless of failure or non-release, and
     they stay 0 only when `evidence_missing` is not `null`.
   - **The first cause of the 09-09 failures is still undetermined.** This fix only ensures that data to judge by remains if the same thing
     happens again, and it is not closed by a successful retry. There is also no evidence that the new tests' injection points are the same
     point as that failure.
     [Evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-partial-result-preservation.md)

7. **Fix recurrent allocation in planning mode.** Code, application and compilation **completed 2026-09-10**, real-hardware gate **passed**
   (planning pass RS 0.00 MiB at r96 and r256, r256 admitted and completed, overflowing configurations still rejected —
   [release gate](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md)).
   **The per-stage host/device plan = actual for those 2B and 35B were cross-checked in the closure audit.**
   r160 and non-regression on other models/backends are for the next version. Completion of r256 on the new binary still needs a re-verdict because of the OS restart.
   - Upstream `llama_kv_cache` uses a size-0 dummy buffer under `no_alloc`, and `memory_breakdown()` also
     reports the expected size including alignment. `llama_memory_recurrent` **does neither**, so it actually allocated recurrent state
     while building the plan, and then compared the reduced `free` against `required`, which already included that cost.
     Compat patch `0026-noalloc-recurrent-residency.patch` aligned both places.
   - **The fit check was not removed and `free` was not adjusted.** Configurations that truly lack space are still rejected.
   - Application of 26 patches, boundary checks, the prepared-tree diff hash (`f37f181c…`) and ABI symbols passed, and
     `llama.dll` linked successfully in CPU Release. The fixed pin checkout was not touched (work was in a separate worktree).
   - **Next step and completion condition:** confirm that actual allocation of RS for planning is gone, distinguished from backend initialization cost,
     cross-check host/device plan = actual at r96, 160 and 256, non-regression for attention, recurrent and hybrid,
     **continued rejection of truly insufficient configurations**, and a separate pass of actual loading, peak memory, inference and UNLOAD at r256 after the fix.
     [Evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-noalloc-recurrent-residency.md)

#### P-3d. Decided work order (2026-09-10)

| Order | Task | Completion condition |
| ---: | --- | --- |
| ~~1~~ | ~~Preserve partial results of failures~~ | **Done** (P-2 item 6). Attribution follows the evidence, and it is locked down up to the actual CLI writing the artifact and exiting with 1 |
| 2 | **Verify no-alloc recurrent behavior** | The five completion conditions of item 7 above |
| 3 | **Connect the B2/B3 admission and return budget** | Secure pending count, bytes and tokens and the required return space from before the first admission write. Preserve ledger, reservation and output authority under rejection, cancellation and failure, with exactly-once settlement. Existing requests proceed to return even during saturation |
| 4 | **Normal response baseline and cause instrumentation** | Seal complete responses and a fixed arrival pattern, record per-unpublishable-reason waits together with STEP trace parse/decode/sample/encode, then repeat A/B |
| 5 | **Evidence-based optimization** | Per H5, change only one policy on the same model, topology, resident, KV and input. 8 paired repetitions and holdout, median useful TPS improvement ≥5 %, confidence interval lower bound >0, TTFT and ITL SLO |

**Raising resident comes after item 3.** A successful r256 load is not service approval.
**The `outstanding > 0` check in `state.rs` is kept** — the next decode needs the result of the previous token, so
removing this check to increase eligible rows is not an optimization but a dependency violation.

#### P-3e. Closure of this version — local complete, external real-hardware BLOCKED

The user decision is v0.9.0, with 0026 included if the required real-hardware runs pass. After passing the initial gate, the closure audit
supplemented the analysis tools, the official Release builder, the binary rebuild and raw data archiving (`3302591fc`).

| Closure unit | Status |
| --- | --- |
| Code, regressions, build | Done. 7 regressions and independent mutations, workspace 1,374/0/7, all Node 146/0, CTest 15/15 |
| New binary real-hardware runs | smoke, pressure and 35B r96 passed. r256 was interrupted by a scheduled Windows Update restart, and the partial artifact is preserved |
| Documents, raw data, package | Preserved as a candidate. The misreading of PLAN/ACTUAL under-reporting was withdrawn, raw data of 10 runs included, incomplete gates stated |
| Formal tag and release approval | **BLOCKED**: after 42mob login is restored, re-judge r256 and must_refuse → update evidence → annotated v0.9.0 |

The existing unpublished tag is preserved with an archive ref and raw data. The new candidate is not turned into a formal pass, and it does not inherit the initial binary verification.
Remote OS updates, login and driver settings were not changed. Push requires separate permission.
The next version's B2/B3, normal response baseline, cause instrumentation and optimization are not added to this closure.

#### P-3b. Measured baseline of `pressure` (2026-09-09, verified)

`pressure` completed for the first time, so this scenario now also has a measured baseline. It was recomputed from the two runs, and
the numbers are owned by the [evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-pressure-measured-baseline.md).
Reproduce with `node test/benchmarks/p4-4node/measure-run.mjs <run dir>`.

| Metric | Measured | Against goal |
| --- | --- | --- |
| Responses | 512/512 are Korean-language TypeScript explanations with no repetition collapse. But **512/512 were truncated at `max_tokens` 200** | **Cannot be used as normal response evidence.** A separate run with a completion budget and stop conditions is needed |
| Generation TPS | 382.15 / 401.75 | No comparison baseline (it had never completed before) |
| UBATCH fill ratio | **15.95 % / 17.02 %** (mean width 81.65 and 87.13 / 512) | Observed value. Fill ratio is not a goal metric (P-3c) |
| Max decode width | **160** (with resident 256) | The ceiling is not the policy — `idle_gated=0`, `ready_rows_left` p50 and p90 are 0 |
| Kernel active window mean | gpu0 18.8 %, gpu1 28.2 % (power 93 W / 177 W) | The fraction of time a kernel was up. Not SM occupancy, so saturation is not stated from this alone |
| Stage occupancy | node0 40 %, node1 and 2 about 30 %, **tail 76.6 %** | The tail is the constraint |

**The location of the width ceiling moved.** The 09-04 35B analysis was "no rows were left behind", and that holds here too, but
this time the maximum of `ready_sequences` itself is 160. `phase_within` in `state.rs` excludes requests with
`outstanding > 0` from decode, so with 4 stages and `depth_mean` 3.18, a large share of resident is always
in flight. **The next measurement records the sequence count per unpublishable reason directly to settle this decomposition.**
A `ready_sequences` maximum of 160 is a result, not a cause decomposition.

#### P-3c. 10 saturation and utilization experiments (2026-09-09, verified)

Ten runs on 3090×2, changing one thing at a time. The numbers are owned by the
[evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-saturation-and-utilisation.md).
All ten runs completed 512 and released 512, and both errors are `null`.

- **The kernel active ratio differed greatly between 2B and 35B.** 2B was 21.4–32.9 %, 35B 36.8–50.7 %, and power
  99–160 W versus 175–213 W. However, the two configurations differ together in architecture, quantization, flash attention, KV format and resident,
  so **"model size determines utilization" is a still unverified hypothesis**.
- **One card, one stage.** With resident fixed and only the split changed, 4 stages are 33 % slower (275.35 versus 183.00).
  From this single run, 2 stages are taken as the **baseline candidate**. 4 stages looked good at resident 256
  because of resident, not the split.
- **`min-batch-rows 64` is excluded from the adoption candidates.** On the same input it raised the fill ratio from 9.09 → 25.14 % and
  dropped TPS by 19 % and kernel activity by 40 % (overlap 73 → 3 %, depth 3.29 → 1.03).
  **Fill ratio is not a goal metric.**
- **Arrival pattern changes performance greatly — classified as input sensitivity.** Even at resident 96 → 160, decode width was only
  32 or 64, and larger bursts widened it (4 requests/125 ms 223.10 → 32 requests/1 s 289.79 →
  64 requests/2 s **313.80** → 128 requests/4 s 302.49). **From 64 → 128 requests, width rose from 51.12 → 53.65 while TPS
  fell, so width alone does not explain it.**
- **The load verdict over-rejects configurations.** `stage_memory_plan.cpp` builds the planning model with `no_alloc=true`,
  but recurrent buffers are actually allocated during planning. It then compares the reduced `free` against the `required`
  that includes recurrent (`free + RS = 22.76 GiB` across 5 plans). **Because this is the staged server, it was not
  fixed in this session.**

**313.80 TPS is the best single-run value among the tested conditions, neither a hardware limit nor an optimal value.** The earlier statement
"35B saturates at about 276 TPS" is withdrawn because later runs refuted it. All 11 successful runs are **fixed-length load tests** ending with
`length` at 200 tokens per request, and the **median TTFT of the default r160 is 93.9 seconds** (it is not a measurement of admission wait alone,
so it is not attributed to an admission bottleneck). This is not normal service approval.
The next order is owned by the last section of the evidence above.

#### P-4. Changes made only when attribution results justify them

| Candidate | Measurement that would justify it | Status |
| --- | --- | --- |
| decode group balancing (30+2 → even) | P-3 confirmed the asymmetric cycle as priority 1 | **First candidate.** See the size estimate below. `MAX_ISSUE_ROWS` also limits prefill width, so using it as is cannot isolate the effect |
| Batch fixed-cost reduction (parallelize the sampler **safely**) | Reconfirm the sampler share with STEP trace on 35B | Half of the tail cost on the 09-04 2B. A gain regardless of width. **The implementation already exists but is disabled by default** — see below |
| Raising resident (32 → 64/128/256) | **After first establishing** the admission and flight budget of §0.5 item 3 | **Independent experiment axis.** The release-path receipt budget that blocked the 09-09 `pressure` (control batches allowed only up to 63 entries) was resolved in P-2 item 5, and a resident 256 real-hardware pass was confirmed. **But the admission and flight budget (§0.5 item 3) still remains** — the admission path still has no pending count, byte or token budget. It is not bundled into one arm with a policy change. There is also no evidence yet that width is linear in resident |
| Replace the 1 ms retry with a capacity wake | If the retry interval shows up in the idle distribution | Shared with §0.5 item 2 |
| Decompose the 233 ms stage on the offloading path | After re-measuring with the same template | Separate verdict |

**The actual state of sampler parallelization.** `P4_STAGED_SAMPLE_THREADS=N` already exists, and in the 09-03 2B 8-run crossover
generation tok/s went from 194.06 → 213.45 (+10.0%) with non-overlapping distributions. But **the default is 1, and
it must be.** Each row has its own sampler, but every worker calls `common_sampler_sample` with the same `ctx_`, and
upstream goes through `llama_synchronize()` inside it (updating `t_eval_us`, `n_eval` and `n_queued_tokens` without a lock)
and `get_logits_ith()` (`output_reorder()` swaps `logits.data` rows in place).
**This is a data race on the buffer the answers are read from.** A judge pass is not evidence that there was no race.
So the content of this candidate is not "parallelize" but **"synchronize once on one thread, make an immutable copy of the logits,
then run independent samplers"**, proven first with a fixed seed, a fixed batch composition and thread sanitizer.
Another serial cost left by 09-03 is sampler creation for each new sequence, which uses the sampler table and is therefore
inherently serial. It is a separate target.

**Not done:** restoring prefill/decode mixing. Mixing 0/2,459 is policy, not a defect. The raw data HELLO has
`equal_sequence_ubatch=1`, and `plan_equal_ordinary` at `scheduler.rs:280` splits the two groups to prevent the problem where a single decode row made the common width
1, so that prompts with thousands of ready rows were sent one row at a time. Reverting it would bring
that problem back.

**Size estimate for balancing (not a promise).** A linear fit on this run's own two groups gives a tail of
`34.76 ms + 3.443 ms × rows`. The intercept is nearly the same as 09-04's 34.2 ms, and plugging in width 15.80 gives 89.17 ms,
matching the measured 89.68 ms. Because the cost is linear in width, **redistribution that keeps the batch count gives no gain.**
The gain comes only when the batch count drops. Gathering 38,084 rows at width 32 gives 1,190 batches with a tail of 144.93 ms,
so the tail alone would allow at most 220.8 decode row/s (currently 116.87). This value depends entirely on three assumptions: ① the fit extrapolates to width 32,
② the two groups can actually be merged, and ③ the tail is the only constraint.
**It is a size that justifies an experiment, not grounds for promotion.** The current tail total is 66.3% of wall clock, so it is not yet saturated.

#### P-5. Verdict discipline

- Comparisons follow verification protocol H5. On the same model, requests, topology, KV capacity, resident and offload layout, change only the policy,
  and require all of: at least 8 paired runs, 4 holdout pairs, median **useful** TPS improvement ≥5%, a positive paired 95% CI lower bound,
  TTFT p95 ≤1.10×, ITL p95 ≤1.05× and the absolute SLO.
- **Do not promote on GPU utilization alone.** As 09-04 showed, utilization and overlap rise even while the fixed cost is being
  paid repeatedly. If useful generation TPS drops while utilization rises, reject.
- When citing utilization or overlap, state the definition and analysis window together. `two_or_more_open_pct` (31.6%) is **overlap of stage
  server RPC intervals**, not device time. Tail RPCs include CPU sampling and copying.
  The fraction of time with at least one stage open is 99.3%. The capture including load and cleanup also differs from the stage execution window
  (35B is 16.1/16.7% and 29.3/30.7% respectively).
- For throughput, state the formula and whether a quality verdict was applied. Semantic checks are distinguished from simple keyword passes.

## 1. Product goal to complete

**Run very large models on nodes distributed across multiple physical computers, continuously accept strong request waves
while keeping normal prompts and responses, and maximize useful generation TPS and GPU utilization.**

- Launching four processes on two GPUs in one host is not a multi-computer proof.
- Small models and 35B are for development, isolating confounders and regression baselines. They alone cannot complete the very-large-model goal.
- Node count is a constraint set by model weights, KV capacity, legal cuts and machine/device placement. Raising performance by reducing
  required nodes is not evidence of improving the fixed-topology batching strategy. Multiple nodes on the same device are also supported.
- Adding nodes is not assumed to grow capacity linearly, because of weight duplication, shared KV and the tightest stage.
- "Optimal" is the best verified within the declared model, hardware, SLO, workload and search range. No global optimum or 100% GPU is promised.
- Improve useful generation throughput and useful GPU computation together. Do not lower TPS by adding small batches, recomputation or polling to raise utilization.
- **The only final outcome evidence is a strong-wave real-hardware run across multiple computers that preserves normal prompts and full responses.**
  Deterministic tests and mocks are required safety gates for entering that run, not proof of the final outcome.

### Currently approved resources and real-hardware expansion order (2026-09-07)

The user-specified model candidates are **all of `S:\models`**, and the real-hardware GPU budget is **two RTX 3090s**.
RAM offloading is also allowed, and after **sufficient verification with VRAM-only, the work extends to larger models that need RAM offloading**.
This order is the per-resource verification order within B6/B7, not permission to skip the B1–B5 safety gates.

1. Build an inventory of the whole model directory. Group split GGUFs as one model, and distinguish auxiliary/non-generative artifacts such as mmproj/LoRA/embedding.
   Record audit status, required resources and the reason for selection or exclusion for each logical model/variant.
   Support/runnability is not approved from file size or name alone, and unsupported memory families are not opened for performance testing.
2. **VRAM-only baseline**: pass the resource-step gate of verification protocol H0 with an audited model whose actual weights, KV, compute/transfer buffers and resident concurrency
   fit within the two GPUs. Large-model placement fitness is not judged from small models alone.
3. **RAM offloading expansion**: after passing the previous gate, evaluate larger candidates within the actual GPU+RAM budget.
   Distinguish CPU computation, host-resident weights/KV, staging/pinned buffers and transfer, and seal the placement.
   Record each candidate's load, normal response, wave, memory and performance verdicts separately. Normal rejection, resource shortage and not-audited are not passes.
4. Per-resource-step policy A/B fixes model, quant, placement, context, resident and workload.
   The TPS difference between a VRAM-only run of one model and a RAM offloading run of another is not computed as a batching policy effect.

A read-only check found that the two 3090s are in **one physical host, M42-SERVER2**. Results on this fleet are
single-host/two-GPU verification. The long-term multi-computer goal and H6 are separate, and other hosts are not added
arbitrarily, nor is RAM usage counted as evidence of a second computer. Approved local safety work and
real-hardware verification within current resources are not stopped because H6 is unmet either. Full completion of the final goal is distinguished from completion within current resources.

Access to `S:\models` is confirmed under the run account. Model absence is not declared, nor the path changed arbitrarily, merely because
S: is not visible in a non-interactive SSH session. Connection information documents are kept outside the repository, and passwords/tokens are
not copied into specs or logs. Model selection and budget are fixed in the verification protocol H0 spec.

On 2026-09-07, the current local run account could read S:. A preliminary list of GGUF paths/sizes/mtimes is
preserved in `target/model-file-inventory-20260907-01.json` (156 files, 63 groups by filename).
The provisional filename-based classification is 40 model candidates / 1 embedding / 22 projectors, with no missing shard numbers.
This is not proof of GGUF headers, full content hashes, family audit, memory plans, remote account access or load success, and
it is not a full non-GGUF list either. It is not read as completing H0's logical model/variant inventory.

## 2. The first 30 minutes of a new session

1. At the repository root, confirm the baseline with `git status --short --branch` and `git log -5 --oneline`.
   Audit changes since the baseline commit in the source paths of §4, and preserve dirty changes left by others.
2. Read this document, the verification protocol and the isolation contract. Do not read every historical document from the start to reconstruct the past order.
3. Check `entrypoints/agent/src/main.rs::main`. The default is `event_runtime`, and
   `P4_AGENT_SERVICE_RUNTIME` is the old Chain/Hop path. The latter's fairness/queue tests do not count as proof of the current path.
4. Cross-check the baseline commit audit of §3 against the last progress record. Do not claim to rediscover counterexamples already locked down;
   first lock down the currently remaining counterexamples on the actual worker/native consumption path.
5. Check the stop/verification budget of §0 first. Only after resume approval, run the fixed verification and record actual runs/exclusions/failures.
6. After confirming the evidence for the first unpassed phase B0, continue from the B1/B2 remainder in §0.
   Do not ignore follow-up implementation and regress to pre-fix tests, and do not assume all of B1 is done.

Model candidate paths and the current GPU scope follow the user specification in §1. Inputs not yet settled are
file access under the run account, full artifact identity/memory plans per candidate, and additional multi-host resources.
Paths, IPs and accounts in the existing harness are past configuration values, not examples. Re-check availability and permissions.
Missing inputs do not automatically substitute a small model or a single host as the final target.

## 3. Audited current status

`Confirmed` below is limited to the baseline commit. Past GPU figures are not re-measurements of this HEAD.

| Area | Verdict | Basis and remaining work |
| --- | --- | --- |
| Current execution path | Confirmed | `main.rs::main` → `event_runtime`; the actual event worker is used |
| Batch selector | Partially implemented | `scheduler.rs::plan_equal_ordinary`: per-cohort sequence ID rotation, patience; `plan_ordinary`: ordinary row allocation. Not a full distributed scheduler |
| Per-request starvation counterexample | That counterexample resolved | All progress in 17 prefill, decode 1, capacity 8, 900 plans. Includes the maximum-gap end interval; not a real-time TTFT guarantee |
| Settlement sharing | Partly confirmed | Worker and simulator call `node/state.rs::RequestState::settle_fragment`. Only part of outstanding, prompt cursor and ready is shared |
| Full settlement atomicity | **Unresolved R-A** | `worker/release.rs::Worker::tail`: removes the open execution before validation and mutates some of several requests first. The worker can continue after publishing an error |
| Approval boundary of output effects | **Unresolved R-D** | `worker/drive.rs::Worker::emit_tail_results`: the tail publishes the head return after OUTER output. Making the head atomic alone cannot prove that external side effects of a malformed return are 0 |
| Publication-return identity/idempotence | **Unresolved R-B** | A previous execution's return consumes the next fragment; a partial prefill without an outcome can also accept another sequence and position range |
| Shared-path regression test | **Unresolved R-C** | `simulator_tests.rs::the_worker_and_this_model_settle_through_one_transition` is a direct function test. Review counterexample: 11 still pass even if the actual Simulation call is replaced with separate bookkeeping |
| simulator | Fixed-latency completion model | Uses the actual selector. Does not go through all of admission, RPC, splitting, credit and release/shutdown |
| worker tests | Tail point only | 3 rejections of an encoded CapsuleSet: normal/excess rows/unpublished. No fake stage or full worker loop yet |
| Open batch ledger | Partial | A per-logical-batch execution set exists. Not bound into one transaction with request settlement |
| backpressure | Partial | Bidirectional preservation and Full/Closed distinction exist. Capacity notification, complete drain and cancellation verification remain |
| admission/credit | Incomplete | Needs KV cell reservation beyond slots, bounded pending, multi-node reservation and complete edge row/byte credit |
| Native compatibility boundary | Partial | `src/` isolation and a pin/patch queue exist. `common/` remnants, product identity enforcement, actual placement binding and backend conformance remain |
| Persistent KV/snapshots | Foundation implemented + target contract | Files, receipts and coordinator exist. Not all of the new namespace, identity, convergence and snapshot contracts are implemented |
| Real-hardware harness | Development foundation | `test/benchmarks/p4-4node/` is tracked. The current remote configuration places stages within one remote host; a final runner for arbitrary multiple hosts needs work |
| Final very-large-model wave | **Unproven** | No completion evidence that simultaneously satisfies normal responses, multiple hosts and sealed repeated comparisons |

The table above is the initial audit of the baseline commit. Afterwards, the pre-fix failure counterexamples were sealed as RED, and the next implementation slice
fixed publication authority and return/effect transitions. **The initial table or the RED counts are not reused as the current status.**
The actual reach of each counterexample and the latest full counts and commands are preserved in the
[2026-09-06 audit record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
C++/real-hardware promotion and CPU-only Rust verification are separate. External fixture features are not included in the default count.

### Past conclusions not accepted

- "2 stages are faster than 4, so create nodes only up to the GPU count": a generalization of a specific batching experiment. Do not use it as a product rule that ignores capacity requirements.
- "RPCs overlap, so multiple GPUs computed concurrently": host service spans and device kernel spans are different.
- "A hop on an empty tail takes 2 ms, so transfer is always cheap": an observation of a specific model on one host/link. Do not generalize to very large models or multiple machines.
- "TTFT is similar for 30 rows and 1,232 rows, so admission is the cause": do not settle causation before decomposing arrival, wait, tokenize, prefill and first-output times.
- "0 mixed batches means depth 1" and "no U+FFFD in the output means the semantics are fine": neither is a sufficient condition.
- "If unit tests are green, current real-hardware quality is green too": the gates for code safety, numeric regression, natural-language quality and performance are separate.

## 4. Responsibility boundaries and code map

| Responsibility | Current entry point/owner | Goal |
| --- | --- | --- |
| Requests, arrival waves, SLO, topology, snapshot triggers | `tools/event-drive/`, `test/benchmarks/p4-4node/` (OUTER) | Product requirements and actual placement spec |
| Delivery, ordering, mailbox, node lifecycle | `layers/agent/`, `layers/adapters/adapter/`, `entrypoints/agent/src/event_runtime/mod.rs` | Backend-neutral preservation, admission notification and drain |
| runnable/batch selection | `layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs` | Pure, deterministic policy; combined with independent state transitions |
| Request, flight and reservation ledgers | `v2/node/state.rs`, `v2/node/worker/drive.rs`, `release.rs`, `settlement.rs` | Single ownership of publication records and atomic, idempotent settlement |
| Splitting and capsules | `v2/capsule.rs`, `v2/capsule/`, `v2/logical.rs` | Cross-check of row ownership against actual physical membership |
| Completion model | `v2/simulator.rs` | Replaces only fake time/engine; no duplication of state-change logic |
| Native execution | `staged/server/src/runtime/`, `staged/server/src/compat/`, `server/CMakeLists.txt` | Public type/facade boundary; upstream convenience features preserved as an opaque plan |
| llama.cpp and concrete backends | `layers/adapters/llamacpp/upstream/`, `staged/compat/` | Per-pin prepare, CPU and declared production backend audit |

The base root of `v2/` above is `layers/adapters/llamacpp/staged/adapter/src/v2/`.
Keep the boundaries policy/ledger → P4-owned capability → native compat → llama.cpp abstraction layer → ggml/backend (CUDA, CPU, Metal, etc.).
Do not put per-model batch/KV rules into the P4 core. Do not copy `common_params` as a few fields and lose upstream indirect options.
Splitting out a new crate is a packaging change chosen after sharing actually works, not a goal of B1.
Each layer's change authority, public types, allowed CMake/Rust dependencies and pin update process follow the [isolation contract](layer-isolation-contract.md).
In particular, adapter-owned semantic DTOs are not lifted into the generic P4 protocol. Make the isolation contract's interface ledger and
dependency manifest implementation deliverables, and separate L1 state commitment from L5 native/output effect execution.

## 5. Target state transitions — contracts to implement

Do not interpret the current function signatures as already implementing the contracts below.

### Publication and settlement

- `plan`: reads the state snapshot and budget and builds candidates. Building candidates alone does not consume publication volume, credit or KV.
- `reserve`: secures all participating stage KV/shape and edge budget. Partial failure is released/converged safely.
- `issue`: binds the actually accepted publication to the fragment record. If transfer success is uncertain, do not revert it to unpublished; reconcile it as `Uncertain`.
- `validate_return`: checks generation/session/sequence/fragment/execution/phase/position range/row membership/outcome **against the publication record**.
- `commit_settlement`: applies together the requests, flight ledger, reservations, credit and follow-up output intents that the return event touches.
  The default contract is full pre-validation of the event followed by atomic application. Switching to partial acceptance first requires a separate receipt and retry contract.
- `drain/release`: transport ACK, compute completion, KV stop point and sequence release attestation are treated as separate events.

A fragment record needs generation, session/sequence generation, logical dispatch and fragment ID, the physical execution set,
phase, `[start,end)`, membership digest, reservation/credit ticket, and state and settlement evidence.
The exact wire encoding is versioned in the implementation slice and respects existing content-type ownership. Raw counters are not used as a substitute for identifiers.
Same ID with the same completion is a no-op/re-response with the existing receipt; same ID with different content is rejected as a conflict.
Unregistered IDs, earlier generations, other sequences, ranges or phases, and duplicate rows are rejected before any state change.
Advancing to an unpublished position, an earlier return consuming the next flight, or reusing a sequence before settlement is a failure.

The engine and the fake engine both return normalized results. Only the way token values are obtained differs; the semantics of applying generated count,
next input position, stop/length and verify/replay continuation are shared.
If the output publisher is Full, the completion intent is preserved, and a retry does not publish the same logical result twice.

### Batch planning contract

- KV resident capacity, resident sequence limit, this decode width, logical batch limit, physical ubatch limit,
  edge credit and per-device execution slots are different axes. Do not merge them into a single `parallel` or bool.
- Ordinary attention allocates legal decode and chunked prefill within the row budget.
- The equal-width family selects cohorts from the whole eligible demand and guarantees progress of **individual requests** through per-cohort rotation.
- The next decode token is not published without the preceding result. Multiple prefill fragments are allowed after proving KV ordering and row/byte credit.
- The atomicity of verify/replay and speculative windows is not relaxed for throughput.
- Rows that are ready but unpublished are classified by reason, such as `shape/credit/KV/cohort/in_flight/deadline`.
  "0 rows left" is not a goal in itself. Rows that cannot be sent because of dependencies are not inflated into runnable.
- Fairness is checked not only by cohort counts but also by each request's first selection, intermediate gaps, final wait interval and real-time queue age.

## 6. Execution phases and promotion conditions

Status labels: `TODO`, `IN_PROGRESS`, `PASS`, `BLOCKED`. Without the required evidence, it is not PASS.
Each phase is linked to test IDs in the [verification protocol](distributed-batching-verification.md).
Sealing counterexamples in B0 is work that **confirms and preserves pre-fix failures**. It is not reported as a passing implementation.
Passing those counterexamples on normal/negative paths and mutations is B1's exit condition.
Do not skip counterexamples because the existing suite passes, and do not hide expected failures to make B0/B1 PASS at the same time.

| Phase | Current | Deliverables / exit condition |
| --- | --- | --- |
| B0 baseline and counterexample seal | IN_PROGRESS | Confirm baseline HEAD/execution path, formal failing tests for R-A/B/C, document and test inventory, list of target candidates/resources/permissions. T00–T04 |
| B1 publication ledger + atomic settlement | IN_PROGRESS | Implement head authority, candidate settlement, effect intents, follow-up incarnation/control receipt and post-approval fairness commit. Hop idempotence before physical execution, full sharing of normalized transitions and failure convergence remain. T10–T19 |
| B2 actual event worker integration | IN_PROGRESS | Current-run consumption regressions for ordinary actual run 2/4/8-stage, speculative 2/4-stage, busy UNLOAD, head OUTPUT/publication evidence and owned observation, plus separate broker saturation verification. Cancellation, drain, capacity notification, the integrated cyclic network and restart freshness remain. T20–T28 |
| B3 admission, KV reservation, edge credit | TODO | Bounded pending and byte/token budgets, deadlines and explicit rejection, multi-node all-or-none reservation, row/byte credit, leak/over-admit 0. T30–T38 |
| B4 continuous batching policy | TODO | Full runnable reselection, ordinary/equal-width/atomic strategies, per-request fairness, batch/ubatch separation, cross-check of simulator/reference against worker using shared transitions. T40–T47 |
| B5 execution identity, native, multi-host harness | IN_PROGRESS | Start by reinforcing the versioned execution identity needed by B1, product LOAD bind and native guard. Actual layout/model/build/ABI binding, full/relink isolation, declared backends and the multi-host runner remain. I00–I09/T50–T58 |
| B6 very-large-model service integrity | TODO | §0's I0–I4 are the current breakdown of this phase. Pass H0–H4/H6/H7 normal requests, sustained ingress, overload, cancellation/drain, actual distribution and soak before performance changes. `integrity_baseline=GREEN` |
| B7 batching optimization and refutation | BLOCKED(B6) | §0's P0–P2. Seal the I0–I4 baseline source, and run paired A/B+holdout changing only one cause on the same single and sustained workloads. A useful TPS/GPU Pareto candidate that passed H5 |
| B8 final re-acceptance and handoff | BLOCKED(B7) | Re-run I0–I4 and the applicable H0–H7 with the selected candidate, declared backend/upstream regressions, a reproducible evidence bundle, all required gates PASS and disclosure of the remaining non-required scope |

Promotion dependencies: B1 after B0, B2 after B1, B3 after B1/B2, B4 after B2/B3.
To verify B1's actual consumption path, B2's minimal fake-stage connection may be written first. This is not permission for B2 promotion or
for real-hardware optimization to go first, and the remaining tests on both sides are not skipped.
B5's environment discovery and identity design can run in parallel with B1, but real-hardware promotion requires all relevant B1–B5 gates.
B7 comes after B6's `integrity_baseline=GREEN`, and B8 after B7. Small native smokes are allowed for regression diagnosis of existing audited combinations but do not replace B6.
Do not modify the checkout of an arm while waiting for a GPU experiment.
Layer isolation is not a final cleanup for B5 alone. Keep the pure boundaries of I00/I03 from every change in B1–B4,
complete all of I in B5, and repeat it in B8 and every adopted pin afterwards.

### Pinning down the first work of B1

1. Reproduce with `Worker::tail`/`handle` an excess return for a registered open batch, a mixed return of A normal + B wrong, an old execution carried in a new event,
   and a wrong sequence/range without an outcome.
2. Do not stop at simply moving `close_execution` later. Pre-validate the whole request, outcome errors and output effects.
   Remove the tail's early output or block it with an approval receipt, so that output from a return rejected by the head does not go out first.
3. Build a validated settlement that checks the return against the expected range/membership in the publication ledger, and apply it exactly once.
4. Inject the same malformed fragment into Simulation's actual arrival path too. Do not substitute direct RequestState function tests alone.
5. When running the new and old ledgers in parallel, do not hide shadow mismatches. A transition period where two places update state independently is not allowed.

### Exit deliverables of B5 isolation implementation

Link the per-boundary ledger of the isolation contract to code symbols/targets. Growing the existing large `p4_llama_compat.cpp` file is not a goal in itself.

1. Seal the current dependency/include/link/type/codec surface as an inventory, and first build normal and violation fixtures for I00–I02.
2. Move common parsing, options, grammar operations and internal assertions into the compat implementation/dedicated test targets.
   Do not reinvent the grammar or weaken white-box tests by duplicating field getters.
3. Clean up public/internal header paths, `Impl` access and direct/transitive links. Verify both the full build and the imported relink.
4. Settle the actually allowed API, lifetimes and failure states of the native shell and engine bridge, and implement a stable code table or
   opaque codec negotiation for the capsule codec. Existing raw integer fields are not automatically called a neutral ABI.
5. Connect synthetic refutations and actual adopted-pin regressions to engine/common changes and to ggml/backend changes separately.
   Promote to real hardware only after passing product LOAD identity enforcement and declared backend conformance.

Each slice delivers the isolation contract's minimum manifest record together with the I sub-cases of the verification protocol.
In particular, cross-checking the dual Rust/native implementation of execution authority, identity of the actually loaded libraries/plugins, opaque handle lifetimes, and
separation of model-free/model-required tests cannot be waived by later performance figures. Report separately what was absorbed by changes in allowed modules
and what needs a higher-level contract change, and do not count getters or new folders as achievements.

This phase can be prepared in parallel, separately from B1's safety fixes. Even in B1–B4, a slice that adds new state authority leaks or
native dependencies is not approved. Detailed responsibilities and allowed dependencies remain solely owned by the isolation contract.

### Policies to compare in B7 and approaches to forbid

First compare a small, pre-declared candidate set for row width, prefill chunk, decode reservation share, age bound and edge issue budget.
Use a cost model that splits per-host/device queue, tokenize, compute, sample, encode/copy, network and tail wait.
Do not wait for the whole pipeline to empty in order to merge cohorts, parallelize without sampler safety, or
conclude from a single local measurement that transfer or node count is the cause.
Research that changes cuts/placement is a separate arm, and reports KV capacity, model, quality and compute differences together.
1GPU/replica/stock llama-server is a diagnostic control under matching conditions. It is not a substitute product for the distributed capacity goal.

### Concrete starting point for B4 policy implementation

Directly read the pinned upstream's `tools/server/server-context.cpp::update_slots`, `can_batch_with` and prompt add/split paths,
and first write a mapping table between the semantics to reuse and the contracts that differ because of distribution. Do not import the HTTP/slot/server_context
implementation or assume llama.cpp already guarantees distributed settlement/credit.

1. Normalize caller state to `resident/eligible/blocked`. Token dependencies, cache command stop points and
   model/LoRA/shape compatibility are separate from a simple ready count.
2. Derive legal shapes and max rows from **the intersection of capabilities of all participating stages**. Reject batches that fit only at the head.
3. Ordinary attention considers decode and chunked prefill together. equal-width and verify/replay use separate strategies that state those constraints explicitly.
4. State deterministic tie-breaks, per-request rotation and age bounds, and row/byte/KV budgets explicitly.
   Separate planning results from publication commit, so that issue rejection/partial success/Uncertain paths do not wrongly consume the cursor or fairness.
5. Cross-check exhaustively against an independent reference allocator over a small state space. The oracle is a baseline for the explicit objective/constraints at one point in time,
   not a global-optimum oracle that knows future GPU costs. Candidate selection by actual cost happens in B7.
6. Version the capability/measured cost profile as input data. Do not fill semantic gaps by adding environment variable thresholds.

The current experimental knobs (min rows/open batches/issue rows/prefill fragments) stay disabled by default or keep the existing single-fragment behavior
until they are connected to a verified new policy. A knob that stays must have an application path, budget, safety and performance refutation.

## 7. Link to the existing U/P series — kept, but reordered

Concrete storage contracts and defect background remain in the [old plan](adapter-restructure-plan.md). Its old done/not-started wording reflects the state at that time.

| Old item | New owner/handling |
| --- | --- |
| U0 | B5 + B8 per-pin regressions. Compat work as a whole does not block B1's pure correctness work |
| P-1 | B0/B5 identity and reproducibility, record identity of the K storage branch below |
| P0 | K branch: namespace, receipt, bundle, CONTROL |
| P1a/P1b | B1/B2/B3: execution ledger, dynamic cells, slot reuse counterexamples. Observation alone does not complete a fix |
| P2 | K branch fault/restore matrix. Unused persistence features are not tied in as preconditions for batch correctness |
| P2.5 | B7 static batch/ubatch calibration. Storage identity changes go through the K gates |
| P3 | Split into B3 admission/cell reservation and K snapshot policy implementation |
| P4 | B1/B4 mechanism-policy separation. Creating a crate and matching goldens alone do not complete it |
| P4.5 | B3 credit; settlement identity and atomicity are needed from B1 |
| P5 | B2/B4/B6/B7. Proof of safe and useful overlap/service, not whether depth exists |
| P6 | B5/B7: optimize after actual multi-machine transfer and fixed-cost profiling. Do not settle the bottleneck in advance |
| P7 | B5/B8 backend/model promotion and K chunked persist. Undeclared combinations stay rejected |

### K: persistence, snapshots and extension branch

Handled in the order K0 namespace/CONTROL/immutable bundle → K1 Persist/Restore/Discard fault convergence → K2 Checkpoint/Fork/RestoreInto/List,
LCP/TrimTo and per-stage stop points → K3 large state chunks/cross-backend and ABI matrix.
Detailed contracts are owned by the [storage convention](kv-state-store-convention.md). Existing open decisions such as Committing, epoch, read-pin and quota are not discarded.
Enabling automatic TTL eviction, prefix reuse or KV restore makes the relevant K gates mandatory. If the B6 wave fits within a resident-only budget,
the core distributed batching goal can be verified first with those features turned off. The final report does not count disabled features as complete.
If an unaudited model is chosen as the final target, its memory/backend audit is not optional but a blocking gate of B5.

### Index of existing defects and open decisions, to prevent omissions

The following is an **ownership transfer**, not a verdict that a defect currently reproduces or has been resolved.
Re-check after the baseline commit, and keep regression tests for closed past defects too.
Detailed background/source text follows the old plan and domain conventions; execution verdicts follow the T/K/H IDs of the verification protocol.

| Old ID | New owner / gate to check |
| --- | --- |
| D1 | B1–B4/B6: actual issue/settle/credit/overlap; T14/T20/T21/T38/T44, H2/H4. The past depth-1 claim is not copied into the present |
| D2/D3/D6/D13–D18 | K0–K2: namespace, identity, receipts, bundles, session serialization, fault convergence; K00–K07 |
| D4 | B1/B2: separate audit of slot/sequence reuse and delivery loss; T12/T18/T24/T25/T58. The O13 repair does not close all position defects |
| D5/D12 | B3/B5: KV reservation/occupancy and wire telemetry; T31/T32/T37/T57 |
| D7 | B5/B7: actual cut-set/copy/network profile; H4–H6. Re-measure cost on multiple hosts |
| D8/D9 | B3/B5/B7: actual compute/SWA memory accounting and batch/ubatch; T31/T37/T44/T53, H5 |
| D10/D11 | K2/K3: large state/prefix reuse; K07/K09 |
| D19–D22 | B5/B8: pin, patches, include, execution identity, EOL; T50–T53 |
| O1 | B3/B5: telemetry version, timing and ownership of reserved/used/last-access; T31/T57 |
| O2 | B5: audit of actual stage ABI functions/types and upstream semantic changes; T50/T52/T53 |
| O3 | B5/K1: actual existence and replay of per-family small models/golden state assets; T53/K03/K04 |
| O4 | B3/K0: total order of reservation/lease, TTL/Commit races, Prepared accounting; T32/K01/K09 |
| O5/O13 | Keep regressions for previously reported fixes: session key delivery and cancel-safe wire; T00/T24/T54/T58 |
| O6/O8 | K2: exact snapshot verb/ID contract and OUTER ledger recovery; K06 |
| O7/O11 | B3/K3: resident/disk/RAM capability, budget, ENOSPC; T31/K09 |
| O9 | B1/B2/K2: separation of transport credit and quiescence; T25/T35/K05 |
| O10 | K2: read-pin/storage domain/Discard; K08 |
| O12 | B2/B6/B8: repeated admission and connection lifetime on the same agent; T28, H2/H7 |

New defects are registered together with a reproduction path, owning phase and test IDs to run. "Already listed" is only a classification of discovery history;
it does not mean a currently blocking defect may be ignored or promoted.

## 8. Status records and stop rules

Per-phase records accumulate at the end of this file in the format below. Evidence figures/full response text are referenced only by links to evidence files.

```text
Phase / status / date:
Verified source commit + dirty or not:
Contract changes and owning documents:
Lesson IDs reused before starting / past evidence / current automatic block paths:
Execution rounds 1–3 / reason each round was opened / sealed input bundle:
Passed test IDs / run command / exit code / result file:
IDs that failed when the fix was removed or errors were injected:
Real-hardware run IDs / binary+model+workload digests / machine identities:
Lesson IDs of new failures / refuted hypotheses / cause proof / automatic block paths added:
Not run, excluded, failed, BLOCKED, and why:
Counterexample/task the next session runs first:
```

Unpreserved counterexamples, acceptance of malformed returns, credit/RSS overruns, normal response failures, record corruption and
arm identity mismatches immediately stop the relevant promotion. Do not pass by narrowing scope or excluding failures.
A phase without its required tests is incomplete, and "tests to be written" is not recorded as PASS.
A record with an empty pre-start lesson search or an empty post-failure automatic block path is not promoted to the next execution round or phase.

### 2026-09-06 initial handoff record — state before the follow-up records below

- B0 IN_PROGRESS: code audit, document role reorganization and failure counterexamples for R-A/B/C added. Continuing actual handle follow-up,
  the not-yet-reached inputs of the later permutation/identity table, and the full worker/effect path need additional verification.
- B1–B8 TODO. The existing partial implementations in the table above are reused, but the completion tests of each phase are not skipped.
- Layer isolation reinforcement: the authority/API/dependency ledger and per-upstream-change failure contracts were stated, but this is not a full I gate implementation.
  common types/Impl, indirect links, codec isolation and the R-D output approval problem remain.
- The K branch is at the target contract/foundation implementation state and needs re-audit before promotion.
- First next action: re-run the preserved T10–T13/T17 failures, and connect registration of actual published expected membership and
  whole-event validation → ledger/effect intent commit. Do not paper over the suite with partial repairs; extend to the handle and output paths.

### 2026-09-06 follow-up implementation — B1/B2 IN_PROGRESS

- Source: HEAD `a9e1967fc` + uncommitted adapter/test/document changes. No commit/push/native build/deployment/GPU run.
- `node/flight.rs::FlightLedger`: issued invocations/owners, physical sets per logical fragment,
  buffering of partial and reversed returns, bounded terminal receipts, rejection of existing ID reuse. **This is the head's ledger**, not end-to-end exactly-once across hops.
- `node/state.rs::RequestState::issue_fragment` and `settle_fragment`: shared by actual publication/arrival in the worker and Simulation.
  Prepared/AwaitingNative/Uncertain are distinguished, and a lost response is not reverted to unpublished.
- `worker/release.rs::Worker::tail` and `worker/outcome.rs`: full candidate validation → request/flight/follow-up intent commit.
  The tail's early OUTER output was removed and output is published after head approval. Pending KV ack and physical outstanding were separated.
- `worker/effects.rs`: executes approved output/forward/native intents. Full is preserved on the existing wait path;
  Closed/unknown outcomes preserve the remaining intents and fence. **This is in-memory preservation**, not an implementation of durable convergence across restarts.
- `process/core.rs::ServerControl` is injected as a Box. The fake only produces native Frames and does not imitate the selector, ledger or settlement.
  It checks actual handle/drive, native call count, split, the last-return barrier and the post-error fence.
  The LOAD parser, the `Worker::run` receive queue and an actual N-stage broker/transport are outside this minimal seam.
- For error counterexamples, mutations and final full counts, use only the latest section of the [follow-up evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
  Reinforcing the original R-A–D counterexamples does not make all of T10–T28 PASS.

**First action and remaining order for the next session**:

1. Check the current source/tests, and lock down the counterexample of **an old RELEASE/RELEASED/SETTLE arriving after reuse of the same load, session, request ID and slot**
   on the actual fake middle and tail workers. `pending_releases[key]=slot` alone cannot distinguish the earlier request from the new one.
   Bind request incarnation and operation/receipt identity from the physical issue through native effects and acks at every hop.
   Do not work around it by only adding a head nonce or forbidding reuse of the same request ID. An adapter wire version and native conformance are required.
2. Lock down the counterexample where a middle node's re-delivery of the same PHYSICAL touches the native KV/sampler twice.
   Implement accepted/running/completed/uncertain in the edge receive ledger, plus reconnection/previous-generation handling.
   Do not claim that the head's duplicate terminal no-op solves downstream duplicate computation.
3. Connect the actual `Worker::run` to an N-stage fake network. Verify bounded servicing that gives publication opportunities even when the input queue keeps filling,
   capacity wake and cancel/reload/drain. The current unbounded `try_recv` drain is separate from selector fairness.
4. Do not stop sharing at two small counter functions. Extend the reference, which replaces only engine result generation, to use the same publication/settlement semantics and
   outcome/stop/position transitions. Concrete native types are not put into the pure ledger/policy.
   The cohort resume/decode_runs that `Scheduler::plan` changes first are also separated into a candidate fairness delta.
   First lock down the counterexample where publication rejection → replanning in the actual drive does not consume the service turn.
5. Before a high-performance baseline, separate immutable request input/routing from small progress candidates. The current `RequestState::clone` copies long prompts and
   the original Event on every publication and return, and effect clones may re-copy cut-set bytes.
   Build a member index at publication registration and update only touched requests/follow-up fragments. Keep the full cross-check as an independent test.
   Check that CPU time and allocation/copy bytes do not grow in proportion to prompt length or unrelated open batches.
6. After that, complete B3/B4's KV/row/byte credit and continuous batching policy and B5's native/product identity isolation, and
   run B6–B8's **very large model, multiple physical computers, strong waves and normal responses**. Local green does not substitute for this goal.

B0's final model/host/permission/budget inputs and B5–B8 real-hardware evidence are still unsettled.
The current return receipt treats identical results as no-ops only within a bounded memory window, and expired IDs are fail-closed.
The full memory cap for active flights/queues/inter-stage tensors is a separate B3 gate and is not proven by the receipt cap.

### 2026-09-06 follow-up record 2 — execution ownership and policy candidates, B1/B2/B5 IN_PROGRESS

- The counterexample of reusing the same load/session/request key/slot was locked down on the actual head/middle/tail consumption paths.
  It was bound in the adapter and native so that late control/acks do not consume the KV, slot or pending barrier of a new incarnation.
  The sole contract for wire, BindLoad and lifetime/scope is the execution ownership section of the [batching contract](adapter-batching-layers.md).
- Exact SETTLE/RELEASE re-delivery at the middle/tail returns the same receipt without additional native execution.
  A different body/kind with the same ID, or an earlier operation, is rejected without effect. The total budget of multiple controls is also
  checked before the first native effect, and later receipt shrinkage is not pre-counted as earlier headroom.
- `Scheduler::prepare_plan_with_physical_capacity`/`commit_plan` were connected to the actual drive and Simulation.
  Candidate rejection does not change rotation or cohort turns, and only approved publications advance. This reinforcement is not a policy optimization result.
- The native ownership/UTF-8 helpers build without llama/ggml types. Product LOAD binds execution identity, but
  this is not a completed B5 boundary that enforces every build/model/state ABI/actual placement.
- Actual consumers, RED/GREEN, mutations, frozen source and full counts are in the [latest evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
  Not-implemented wording in the earlier progress record is history from that time and is updated only within the completion scope of this record.

**First action for the next session**: start by running the failure counterexample that re-delivers the same PHYSICAL to middle and tail with a new event ID.
The current control receipt and head terminal receipt are not ledgers that prevent physical KV/sampler recomputation.
The order after that is as follows.

1. Implement accepted/running/completed/uncertain for PHYSICAL receive/execution, per-stage prefix ordering and blocking of earlier load/run.
   Distinguish duplicate-return no-ops from duplicate-native-execution no-ops, and complete T18/T24.
2. Check sustained input, output saturation, cancellation, reconnection and shutdown on the actual `Worker::run` + N-stage fake transport.
   Do not substitute direct method-call tests for completion of this step. Starvation from unbounded input drain is also closed here.
3. Bind the reference and production through the same transitions up to outcome/stop/position, and separate large immutable inputs from small candidate deltas.
   Reduce full-history scan/copy cost while keeping the independent full cross-check, rejection counterexamples and mutations.
4. Then complete B3/B4's actual KV and row/byte credit and continuous batching policy, and B5's remaining isolation, execution identity and harness,
   and move to the final multi-computer real-hardware runs of B6–B8. The relevant K branch gates come first only when persistence features are enabled.

Because the execution ownership wire changed, earlier binaries/deployments cannot be used as is. Every stage must negotiate the new contract,
and legacy mutation bypasses or automatic acceptance of mixed versions are forbidden. The actual model choice, multiple physical hosts and
access permissions are still H0-unsettled, and an arbitrary small model/single host does not substitute for the final goal.
Native CTest's exit 0 contains a model-absent SKIP branch, so "13 finished successfully" is not read as a model conformance PASS.
GPU/real-hardware waves and performance non-regression are not part of this slice's completion claim.

### 2026-09-07 documentation reinforcement — layer isolation is a constraint on every phase

- Per the user request, the [isolation contract](layer-isolation-contract.md), which distinguishes P4 common, adapter L0–L5, native shell/engine bridge/common compat,
  the llama.cpp model execution abstraction layer and ggml/backend, was reinforced.
  No new layer was added; the three checks of dependency, authority and semantics were stated, and the required fields of the implementation manifest were defined.
- The common signature, Impl bypass, indirect linking, imported binary identity and handle lifetime paths in existing code/CMake were
  re-read. Remnants found or reconfirmed were linked to the isolation contract and to I/T sub-test requirements. This is not a code repair or a gate PASS.
- Pin changes with the same semantics are absorbed inside the allowed compat module, and normalized inputs/traces and the scope of upper-layer source changes are
  cross-checked independently. Actual semantic, ABI or backend constraint changes are handled by rejection or an explicit contract change.
- **The implementation status is the same as follow-up record 2 immediately above.** This reinforcement does not change B1/B2/B5 to complete.
  The first next action is the counterexample of re-delivering the same PHYSICAL with a new event ID, and the order after that follows the record above and §6.
  Without a declared final model, hosts and permissions, no real-hardware or very-large-model results are claimed.
  The current load highwater is protection within the same Worker lifetime. New load/run freshness and reconnection, including Worker re-creation within the same agent,
  are T18/T24 remainders, and it is not assumed to be preserved just because the process is alive.
- Document verification: `npm run test:docs-lint` 12 passed / 0 failed, `npm run docs-lint` 73 tracked files,
  `node tools/scripts/docs-lint.mjs --all` all 79 files clean, `git diff --check` no errors.
  No full Rust/C++/GPU tests were newly run for this documentation reinforcement. These figures are not I/T/H implementation passes.

### 2026-09-07 follow-up implementation — PHYSICAL re-delivery and confirmed real-hardware resources

Status is **B1/B2/B5 IN_PROGRESS**. HEAD is `a9e1967fc`, and uncommitted changes were verified.
A receive ledger was connected to the actual middle/tail `Worker::handle`→PHYSICAL→native Frame path.
Exact input re-delivery replays the preserved result, and a cached+Fresh mix computes only Fresh. Full pre-rejection,
the fence on unknown native outcomes, prevention of number collisions from another head, and no recomputation of old results after release/slot reuse were checked.
The sole definition of identity, retention caps and expiry follows the PHYSICAL receive section of the [batching contract](adapter-batching-layers.md).
This implementation is an idempotent ledger foundation, not a TPS improvement from batch policy or completion of all of T24.

The use-after-move defect of the opaque plan was also fixed in the test shared preparation. The ownership transfer order of the actual options E2E
and the model-free lifetime test go through the same helper, but **the options E2E with a model loaded
has not been run yet**. Successful exit of the native build and the skipped model path are counted separately.
The latest tests, RED, mutations and source identity are in the last section of the [evidence record](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

The user specified the model path and GPU scope and allowed RAM offloading. Current resources and the expansion order are owned by §1,
and the verdict conditions by verification protocol H0. Only listing/SSH reads were performed; no new binary deployment, GPU model load or
real-hardware wave was run. The existing harness fixes GPU loading and one ingress, so it is not treated as a runner that already executes RAM/multi-host
manifests. That path and its negative tests must be implemented in B5.

**The first next action** is the counterexample that sends a past position, a future gap and a wrong phase of the same incarnation, tagged with a new execution ID,
to the actual middle/tail. Exact old-ID re-delivery and normal continuous prefill/decode are checked together.
The subsequent progression is as follows.

1. Explicitly bind the per-stage KV frontier for ordinary/equal/Verify/Replay and SETTLE/RELEASE.
   The current ownership check only confirms the active owner and does not prove this order. Separately reinforce the freshness of new Worker/native Session
   creation and credit/retry lifetimes. Do not complete this with the existing result cache alone.
2. Verify publication, partial return, sustained input/output saturation, cancellation and drain on the actual `Worker::run`+N-stage transport.
   Do not use the 14 method tests and pure ledger tests as a substitute for this full-loop gate.
3. Reduce B1 candidates' large immutable input/response copies and `O(seen)` full index duplication to touched deltas.
   Keep the independent full ledger cross-check, atomic rejection and mutations. Do not extend the cache byte cap into an active memory/RSS cap.
4. After completing B3/B4's reservation, credit and batch policy and B5's native/placement/runner, enter real-hardware runs in the resource steps of §1.
   Even if absent resources limit the real-hardware scope, do not skip local consistency implementation or close the final goal with a small model.

### 2026-09-07 follow-up implementation — defending against KV position bypass with new IDs

The first action of the previous record was run on the actual middle/tail. 1 normal-path case passed, but the 8 cases of
past position, gap and phase regression with a new ID and pre-rejection of mixed events failed by mutating the native KV/sampler.
Afterwards, a pure stage frontier was connected to head publication, middle/tail PHYSICAL and SETTLE/RELEASE.
The sole contract for position and phase follows the stage KV frontier section of the [batching contract](adapter-batching-layers.md).
Normal full Verify acceptance, partial acceptance and checkpoint Replay were also checked with actual worker methods.
10 pure tests and 28 actual PHYSICAL worker tests passed, and 6 mutations in independent copies were detected.
The full run is 1012 passed / 0 failed / 7 ignored; detailed source seals, raw text and mutation scope follow the last section of the evidence record.

**B1/B2/B5 are still IN_PROGRESS**. Full Worker::run, position self-verification on direct native calls, restart
freshness, credit, and VRAM-only/RAM offloading real-hardware runs are not complete. The currently queried hardware scope and
the RAM expansion order after VRAM-only stay as in §1, and these Rust tests are not a substitute for real-hardware results.

**The first next action is to close the two newly reproduced P1 paths where proposal width is not checked.**
The independent copy in `target/proposal-cap-red-20260907-01` preserves the counterexample where, at `physical_capacity=2`, both PHYSICAL and SETTLE
approve 3 well-formed native proposals. Being within the token budget is different from
being within the physical atomic width. Port this counterexample into the original's actual consumption test, check the cap
before approving the response, and confirm the unknown-outcome fence and 0 follow-up native calls. Do not close it by having the head's `SETTLED` reject it later.

After that, continue with the full-loop, touched-cost and B3/B4/B5 items of the earlier record. Do not rebuild the already implemented receipt and
frontier from scratch. Do not report new counterexamples outside the frozen source as included in the whole-suite GREEN;
keep their raw text and failure counts separately. This time, even though the original suite is GREEN, the two independent P1 cases are RED.

### 2026-09-07 follow-up implementation — continuation width and first verification of the actual loop

The previous two P1 paths and the normal/wrong Fresh mix were ported into the original tests. The native continuation width is
checked before PHYSICAL, SETTLE and head return approval. Follow-up progress at normal widths 1/2 and the post-failure fence were
checked together, and the sole definition of the contract is the stage KV frontier section of the batching contract.

2/4/8 actual `Worker::run` threads were connected to an independent native fake. It checks ordinary wave merging, individual
token/position, release on all stages, recovery from a Full completion queue and the max-open partial return barrier.
A separate actual EventNode/broker test reproduced a two-node
stall where each waits on outbound Full and cannot read inbound. The neutral core's pump preserves one event per direction and keeps processing the opposite direction.
No token/batch/model knowledge was put into the core. Test scope, mutations, source and counts follow the latest evidence record.

**B1/B2/B5 are IN_PROGRESS**. The run-loop fixture above does not go through LOAD/subprocess, an actual model or the network,
and covers only ordinary. Even passing together with the separate broker test, it does not complete credit across the integrated full cyclic network,
starvation under sustained input, speculative SETTLE/Replay, or cancellation/graceful drain.
The original Worker::run's unbounded input drain and drive loop have not been changed yet.

After the freeze, a sustained-ingress counterexample was reproduced in an independent copy. When the Tokenize chain in front of runnable requests is 0/16/256,
the number of Tokenize events before the first Logical is also exactly 0/16/256. Completing normally after the chain ends is different from guaranteeing finite processing
opportunities. This observation probe was not included in the original whole-suite GREEN.

**First next action:** port the counterexample above into an actual run-loop regression in the original, and implement an explicit contract that alternates input processing and publication
in finite opportunities. Also check starvation in both the input and publication directions, and the opportunities of tail/control.
Removing the new call budget must make that regression fail, and the tuning constant is not announced as a TPS optimum.
After that, proceed with speculative control, cancellation and shutdown in the actual run loop, B1 touched-cost, B3/B4 credit/admission/policy, and
B5 native/placement/runner. Hardware resources and the VRAM-only → RAM offloading order stay as in §1.

### 2026-09-07 follow-up implementation — finite input/publication opportunities and termination classification

This section is the implementation state after the earlier "unbounded loop unchanged" record. The actual `Worker::run` processes up to 32 inputs
per turn and then publishes at most one voluntary logical batch from the head. If publication succeeds, it reselects in the next turn even with
no input; on gate/no demand, it waits for input. The number 32 is not a TPS tuning knob
but a cap on actor processing opportunities. The workload and synchronous native call time inside one PHYSICAL/control event are separate.

After an observed stop/input EOF, new native publication is blocked. Before shutdown, the remaining local requests, settlements, flights and KV/effects are
recorded, and normal local empty is distinguished from abandoned/stopped/failure. Completed history is not counted as unfinished work,
but Stopped KV and Uncertain are kept. A successful shutdown is not announced before cleanup, and a cleanup failure is
recorded as a top-level failure. This is **preservation of shutdown evidence, not an implementation of request cancellation notice or network drain.**

A sustained Tokenize chain, additional publication without input, a normal SESSION during publication, and stop/EOF/cleanup failure are locked down as actual
run regressions. Query functions and actual shutdown consumption tests are kept separate, and independent-copy mutations removing the implementation and
final source/run counts are recorded in the dated evidence. Existing GPU measurements are not reused as performance evidence for this change.

**B1/B2/B5 remain IN_PROGRESS**. **The first next action** is to extend the actual run fixture to speculative
SETTLE/Replay with normal full/partial acceptance, through exact follow-up positions, outputs and release on all stages.
Reuse the existing direct method-call tests and the independent token/KV model, but do not duplicate production transitions inside the fake.
After that, verify cancellation, normal drain, repeated runs without restart and capacity notification. B1 touched-cost,
B3's bounded admission/edge row and byte credit, B4's fairness/policy including multiple sessions, and
B5's direct native calls and actual placement/runner also remain. The full exit conditions of each phase stay in §6, and
this slice's success is not turned into overall completion. With current resources, first prove normal strong waves with VRAM-only,
then extend to RAM offloading models. The final multi-physical-computer proof is separate.

The starting file for the next slice is `worker/loop_tests.rs`. Use the node-target routing pump as is, and
add a separate speculative oracle without weakening the ordinary oracle. The current fake's `compute`
allows only Prefill/Decode and does not implement `PhysicalSettle`, and the simple chunk split and the check of 1
append per position cannot be used as is for speculative. First lock down three literal response scripts: **full acceptance (no SETTLE) / direct partial acceptance /
Replay after checkpoint recovery**. Independently cross-check atomic group preservation,
0 new native calls while SETTLED is held, the append→trim/restore→re-append record, exact output tokens, positions,
caps and termination, and RELEASE on all stages. In particular, do not confuse Replay's output flag with actual generation results,
and cross-check the rollback position against the native path. This is a design guideline for the next test and
does not mean that the current 1039 include that speculative full-loop test.

### 2026-09-07 follow-up implementation — speculative full-loop and native Replay boundary

The first action of the previous section was performed on the actual Worker::run. Full acceptance, direct partial acceptance and checkpoint Replay
are each checked on 2/4 stages for settlement chain holding, a separate runnable request, exact output, KV change history and release on all stages.
The existing 5 ordinary tests were kept and 3 were added, and 3 production consumption mutations fail.
The sole verdict conditions of the tests are verification protocol T20/T24; raw text, source and counts follow the latest settlement evidence.

Separately from the fake passing, an error was found where the native batch also used Replay's logical output=false for the logits request
and then read those logits immediately. The native translation of the request mask was separated from the logical wire semantics, and
a model-free consumption test that goes through the actual batch construction body was added. This does not yet prove actual llama sampling, checkpoint
restore or model numerics/performance. Restoring FIRST's server capsule mask is a separate consumption scope.
Also, the unconditional compat include in the no-llama build was made conditional, without widening include/link dependency permissions.

**B1/B2/B5 are IN_PROGRESS**. **The first next action is to close the UNLOAD-during-execution counterexample.**
In an independent-copy actual run, sending the existing UNLOAD while holding the tail return recorded native shutdown 1,
rejection 0, UNLOADED 1, held tail 1, output 0 and snapshot unloaded. This RED is separate from the original full
1042 GREEN. Lock down the target contract below on the actual path so that an UNLOAD outside a stop point does not erase unfinished requests/KV as a success.
The concrete counterexample and the not-run positive control are left in the evidence record.

1. Start by implementing a safety contract that limits UNLOAD to releasing a stopped load. Look not only at pending/requests/flights but also at
   pending SETTLE/RELEASE, the Verify fence, the middle's active owner/frontier, and Uncertain/remaining effects.
   A busy rejection must have no native call, no ledger deletion and no UNLOADED success effect. Do not call this an immediate forced cancel.
2. Port the tail-holding counterexample into the original ordinary run, and run idle UNLOAD success after the original requests complete normally
   as a positive control. Add speculative SETTLED holding and middle-stage active KV as well. Both a wrong guard that checks only requests/flights
   and unconditional UNLOAD rejection must fail.
3. Then design explicit Cancel/Drain state and authority. The current event vocabulary has neither, and the old Agent/deployment
   Cancel is a different path. Distinguish stopping new admission from delivering existing returns, SETTLE, RELEASE and output, and do not
   roll back tokens already sent. Do not promote input EOF/Drop, local remainder 0 or native unload success to a global drain.
4. Bind completion ACK, capacity notification, shutdown/join and repeated runs of the actual EventNode/adapter/transport.
   Separately verify freshness for slot reuse on the same loaded Worker, unload/reload on the same Worker, and new Worker/agent restarts.
   Do not assume the current load highwater is preserved in a new Worker.

Then proceed with B1 touched-cost, B3 bounded admission/edge row and byte credit, B4 multi-session policy, and
B5 direct native call authority, actual placement and the real-hardware runner. Unverified MTP/native model paths are not
opened as ordinary GREEN. The currently approved resources and the order **VRAM-only sufficiency verification → verification of larger RAM offloading
models** stay as in §1 and H0. Single-host 3090×2 results are distinguished from the final multi-physical-computer goal.

### 2026-09-07 follow-up implementation — safe UNLOAD and the native shutdown failure boundary

The UNLOAD RED from the earlier record was ported into the original actual Worker::run. Four regressions — ordinary tail holding with middle KV, and
speculative SETTLED holding with middle Verify KV — pass through completion of existing requests and idle UNLOAD success.
The original ordinary/speculative oracles were kept. Independent mutations where a guard that checks only request counts and unconditional rejection
also fail are kept. The sole semantic contract is the explicit UNLOAD section of the batching contract, and the verdict is owned by T25.

Additionally, an actual run found a counterexample where a new SESSION is ACKed after a native cleanup failure on idle UNLOAD.
After a native failure, the worker is fenced and shuts down keeping the original error. Normal busy rejection and fatal failure
cleanup are distinguished, and this change is not an implementation of Cancel/Drain, actual OS process cleanup or output delivery ACK.
Source, raw text, full counts and mutations are recorded in the latest settlement evidence. HEAD is still `a9e1967fc` + uncommitted changes.

**B1/B2/B5 are IN_PROGRESS**. The next consumption boundary audit found a code mismatch where
`tools/event-drive/src/run/inference_identity.rs::InferenceIdentity::output` accepts only the tail source and so rejects OUTPUT approved by the actual head.
A worker fake pass is not called a pass of the current real-hardware drive.
Feeding the 15 actual producer outputs from an independent copy (5 ordinary, 10 checkpoint Replay) unchanged into the actual consumer
reproduced rejection of all of them. A causal control that changed only the source to tail passed entirely. That change was not
accepted as a repair; these are two consumer REDs outside the original 1047 GREEN. The actual captures, commands and source seals are recorded in
`target/head-output-consumer-red-20260907-01/verification.md` and the latest settlement evidence.
**The first next action is to port this cross-boundary counterexample into the original and bind it to the head-approved output contract.**
Do not relax the rejection check by unconditionally accepting both tail and head. Keep the actual production output fixture together with strict identity
negative tests for load/session/request/route/position. This is not something that changes the P4 neutral transport.

After that, continue with the Cancel/Drain authority, output delivery/ACK, capacity notification, shutdown/join and repeated runs without restart from the previous section.
Cancel acceptance, publication prohibition, settlement of existing native work, release on all stages and OUTER terminal delivery are
different pieces of evidence. Commands absent from the current v2 vocabulary are not replaced with the legacy Agent Cancel. B1 touched-cost,
B3 admission/edge credit, B4 policy, B5 native/placement/runner and the real-hardware order of §1 remain as they were.

The next Cancel/Drain implementation first closes the following code-level constraints with actual consumption tests. This is not a description of the current command implementation.

- `worker/emit.rs::publish_or_wait` makes the worker thread itself wait on a completion Full. Do not claim that adding only a Control class or
  a receive waker lets a later cancel be processed. Design a bounded effect pump/space notification together with
  control processing opportunities and capacity, and prove arrival during saturation in T22/T23/T26.
- Distinguish request-attempt authority before admission from slot/incarnation and issued membership after admission.
  Do not use the durable session_key as a cancel ID, and a late cancel after reuse of the same request name must not
  touch the new work. Do not delete a still-in-flight request from the ledger first.
- Do not use enqueue in `event_runtime/transport.rs::deliver_outer` or write success in `write_loop` as an OUTER consumption ACK.
  Currently OUTPUT does not carry the incarnation, and RELEASED does not carry a per-request completion watermark.
  State this explicitly when defining the cancel-completion/Drain receipt; the P4 core owns only a delivery contract without model semantics.

### 2026-09-07 follow-up implementation — head-approved OUTPUT and the actual OUTER consumption boundary

The output rejection counterexample of the previous section was ported into the original default tests. The 15 ordinary and checkpoint
Replay outputs of the actual Worker::run are preserved as a shared wire fixture, and connected separately to a semantic cross-check of the current producer and to actual InferenceIdentity/
inference::drive consumption. The consumer accepts only the full configured head endpoint, not the tail or
all nodes together. Producer and consumer mutations and the exact run/exclusion scope follow the latest settlement evidence.
The approved output contract is owned solely by the batching contract, and the verdict by verification protocol T20. The P4 transport and native ABI did not
change in this slice. HEAD is still `a9e1967fc` + uncommitted changes.

**B1/B2/B5 are IN_PROGRESS**. An additional audit of the source-fixed actual drive in an independent copy found it still approving the three
rejection counterexamples below. Normal control 1 PASS / rejection rules 3 RED, separate from the original full GREEN.

- The full release count is filled by two fresh-ID RELEASED of the same request. The actual peer's release set contains no other request.
- With submitted max_tokens=1, two OUTPUTs at consecutive positions and a length termination are approved.
- At the boundary [4,7] of different prompts, shifting all output positions of the second request by +1 is still approved.

**The first next action is to port these three counterexamples into default actual consumption-path regressions and close the evidence for the completion verdict.**
The starting files are `tools/event-drive/src/run/inference.rs`, `inference_identity.rs`, `acceptance.rs` and
`worker/release.rs`. Find the independent runs/original probes to reuse in the settlement evidence. Per T20's completion conditions:

1. First lock down positive/negative consumption tests for caps, terminals and per-request prefill boundaries. Do not weaken the existing response/judge.
   Check the relationship between BatchObservation arrival order/loss and the actually approved prefill end position, and
   do not mistake the optional common `expected_prefill_rows` for required per-request evidence.
2. The existing scalar RELEASED cannot express a set of requests. Design, together with actual producer/consumer changes, the membership/operation
   and incarnation evidence needed for an adapter-owned versioned completion receipt. Distinguish duplicate prevention within the current run
   from freshness after restart. Keep the normal path that releases multiple requests in one notice, and
   do not replace the higher-level success verdict with a simple count or correlation de-duplication.
3. Each counterexample must fail on the original → pass after the fix → fail again under independent mutation. Distinguish the claim that the producer actually
   produced this corruption from the proof that the consumer blocks a corrupted peer response.

Next, continue with the Cancel/Drain, bounded effect pump, capacity notification, actual EventNode/broker
saturation, shutdown/join and repeated runs from the previous section. B1 touched-cost, B3 admission/edge credit, B4 policy and
B5 native/placement/runner remain as they are. Source approval alone does not approve GPU real-hardware runs or normal responses as a whole,
and current resources' **VRAM-only sufficiency → RAM offloading expansion** applies §1/H0 unchanged.

### 2026-09-07 follow-up implementation — OUTPUT budget and the per-request fresh-prefill boundary

Of the three consumer REDs from the previous section, the sampled output cap and per-request first position were closed with default actual drive tests.
The budget check runs before applying OUTPUT, and the per-request observation cross-check runs at the overall terminal/release boundary.
The order in which observations arrive after a valid OUTPUT is allowed, but if observations are missing or mismatched by that final boundary, it is rejected.
An empty EOS is also counted in the sampled budget, and an empty response is not approved as normal quality. The detailed semantics are owned by
the OUTER budget section of the batching contract, the tests by T20, and run raw text, independent mutations, seals and counts by the latest settlement evidence.

This is a cross-check of head observations and OUTPUT for fresh position 0 submissions. It is not a proof that independently observed the actual tokenizer/KV, nor
a position contract for Restore/LCP. The 15 shared actual producer OUTPUTs were kept as is, and the prefill total of the actual observations
published by the producer was compared against an independent workload constant. The observations and release notices added to the consumer fixture
are synthetic and are not called actual producer captures. Full Rust **1076/0/7 ignored** and
harness/build wiring **63/0** are attributed only to that source seal. No C++/GPU real-hardware runs were done in this slice.

**B1/B2/B5 are IN_PROGRESS**. **The first next action is to close the actual production/consumption contract of the release set.**
The counterexample that counts A twice with scalar RELEASED and approves it as an A+B release is still unresolved. The boundaries below are handled
together, and no circular verification is built in which the received receipt defines its own expected identity.
A follow-up independent actual run confirmed a RED where, even when A/B of different OUTERs finish normally in one physical capsule, release notices
go 2 to A and 0 to B. A separate broker→actual handle test showed that an ACK from a middle/external Node that is not the tail
also triggers slot return and re-admission of waiting requests. Both results are copy-only counterexamples not included in the original 1076 GREEN,
and are not yet repaired. **First lock down independent SESSION authority for release ACKs**, and port the external completion contract below
together with it. In a 3-stage, next is the middle, so a temporary repair that accepts only next as the source is also forbidden.

1. Settle the expected authority of the request attempt, slot/incarnation and release operation before the release notice. The existing
   OUTPUT/RELEASED content-types do not carry this evidence sufficiently, so change the versioned adapter contract and
   producer/consumer together. Duplicate prevention during the current run and freshness across new OUTER/Worker restarts are
   separate. The Sender's sequence starts from 1 in a new instance, so event_id alone does not prove restart identity.
2. Start by locking down the actual run counterexample where one physical batch holds multiple OUTER owners. Like the per-owner
   ReplySpec of OUTPUT, the request-owned route must be preserved for pending releases. Do not send notices for multiple owners through the base batch or the
   single return_route of the returned ACK. Internal multi-request RELEASE batching is kept.
3. Clarify the relationship between slot return/pending admission and external notices after validation of ACKs from all stages. A notice failure does not
   re-release already settled KV, and the remaining notification intent must be preserved in effects.
   Check on the actual path the atomic rejection of A valid/B invalid, duplicate/missing/stale releases, multiple OUTER routes, and first/follow-up emit failures.
4. Also connect the observations of the same multi-OUTER actual run through to the consumer. Currently, production that sends the full batch requests to every ReplySpec
   and consumption that accepts only its own submissions disagree. Do not call the whole multi-OUTER case complete after repairing only release routing.
   Apply the batching contract's distinction between per-owner observations and overall statistics together with T20's positive/negative tests.
   Replaying actual producer observations, without after-the-fact edits, into the actual InferenceIdentity has already confirmed unknown-request
   rejection on both A and B. This is copy-only **1 PASS/1 RED** and does not count as the full drive or completion of the original default tests.

After that, continue with Cancel/Drain, bounded effect pump, capacity notification, actual EventNode/broker saturation, shutdown/join and
repeated runs without restart. B1 touched-cost, B3 admission/edge credit, B4 policy and B5 native authority/
placement/runner also remain. Model/GPU results are still approved only in the §1/H0 order of **VRAM-only sufficiency → RAM offloading expansion**.
Single-host verification with two GPUs is not turned into multi-physical-computer completion.

### 2026-09-07 follow-up implementation — SESSION authority and the release ACK sender boundary

The ACK source counterexample from the previous section was moved into an original actual broker/worker regression and repaired with an independent topology declaration in SESSION.
The old wire is not converted implicitly, and the actual OUTER production path was ported together. The sole definition of wire/role semantics
is the release authority section of the batching contract, the test constraints are T20/T25, and runs, mutations and seals are owned by the settlement evidence.
This fix is inside the concrete adapter and OUTER, and adds no knowledge to the P4 neutral broker/native/llama/backend.

The actual 3-stage counterexample confirmed on the old code that a middle node's ACK caused the 9th waiting request to be published early.
After the fix, native publication state is preserved, and after resuming on the normal terminal ACK the run completes against the existing output/position/KV oracle.
The existing ordinary 2/4/8 and speculative 2/4 paths are kept. This is the actual worker loop with post-LOAD fake stages,
not a proof of an actual model, quality, GPU or TPS. The code baseline is still the uncommitted tree on the same HEAD.

A further code-only audit confirmed that transport peer/source authentication, control authority of the first configurer, and full-topology attestation in SESSION_READY
are separately not implemented. Layer attribution follows the trust boundary of the isolation contract.
In the B5 product/fleet identity gate, separate trusted-network restriction from actual authentication/consensus proof, and do not report field comparison
as sender authentication or as approval of identical declarations by all nodes. This audit is not a reproduction of a network intrusion.

**B1/B2/B5 are IN_PROGRESS**. The ACK role check is closed, but full release completion is not.
**The first next action is to connect the request-owned provenance of pending releases to the versioned OUTER completion contract.**
The starting files are `worker/release.rs`, `worker/effects.rs`, `node/state.rs`, `commands.rs`, and
`tools/event-drive/src/run/inference.rs`/`inference_identity.rs`. Reuse the already sealed multi-OUTER actual run and
the scalar duplicate-approval RED. Fixing only the owned route does not complete membership evidence or restart freshness.

1. Before deleting a resident request, preserve small request attempt/ReplySpec/slot, incarnation and operation evidence in pending.
   Port the contracts and production/consumption of submission, terminal approval and release notice together, and do not let the received receipt create the expected values.
2. After full ACK candidate validation, commit the state and per-owner notification intents together. In actual Full/Closed/
   event-ID exhaustion counterexamples, the remaining notices must be preserved and native release must not be repeated.
3. Connect per-owner OUTPUT/release/observation of an actual concurrent A/B batch through actual OUTER consumption. The current observations'
   foreign-request rejection and missing B spans are also unresolved. Do not pass by turning overall statistics into request-owned rows.

After that, continue with Cancel/Drain, bounded effect pump, capacity notification, actual EventNode/broker saturation, shutdown/join,
repeated runs, and B1 touched-cost/B3 admission and credit/B4 policy/B5 native, placement and runner.
The real-hardware resource order stays as in §1/H0, and local consistency GREEN does not substitute for VRAM-only/RAM offloading waves.

### 2026-09-07 follow-up implementation — per-request release proof and owner notification

For normal sampled termination, items 1 and 2 of the previous section were implemented, and actual production/consumption was ported together. The owner is the release section of the batching
contract, and the new wire's fields, order and exclusion scope are not redefined in other documents. The P4 neutral envelope/
broker and native/llama/backend are not targets of this change. It is still the uncommitted working tree on HEAD a9e1967fc.

Expected values are fixed first from actual request submission and terminal approval, so that A's duplicate receipt cannot stand in for B's
completion. In actual 2/4-stage mixed terminals, each OUTER's route/correlation/deadline is preserved, and
unpublished notices are preserved on first/follow-up Full, Closed and ID exhaustion. New captures of the actual original PREFILL, OUTPUT and receipts
keep the old-version token/text/position/stop checks unchanged. This is a proof with post-LOAD fake native and in-memory
EventWire, not a proof of model semantics/tokenization, an actual network or GPU performance.

For the source379 seal, full Rust **1122/0/7 ignored**, extended JS **75/0**, independent production/consumption mutations, and per-run
scope and first failing test, see the [per-request release section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
The earlier JS63 was the result of 57 selected harness tests + 6 build wiring tests. While widening the scope, a configuration test referencing an old repository path
was found, and only its import was fixed to the actual module. Missing tests were not turned green with a runtime skip.

**B1/B2/B5 are IN_PROGRESS**. Normal termination in the current run does not mean freshness across new OUTER/Worker restarts, Cancel/failure without output,
durable outbox/reconnect convergence, or success of multiple OUTERs as a whole. Multiple concurrent OUTERs with the same request_id
are also not supported yet. Notices fenced after a failure are not reported as automatically republished/recovered.

**The first next action is to align observation production and consumption for the same actual multi-OUTER batch.** The starting points are
`worker/observe.rs`, `commands.rs::BatchObservation`/`StageSpan`, OUTER's `inference_identity.rs`,
`inference_evidence.rs` and report aggregation. Reuse the earlier sealed unknown-request RED and the current per-owner actual run.

1. Lock down a versioned contract that separates the original physical width and whole-execution cost from each OUTER's authorized request rows.
   Do not pass by copying all requests to every route or by turning off unknown-request rejection. State the span recipients explicitly
   and do not count the same computation once per request. Also audit the attempt scope of observations that carry only request_id.
2. Connect the actually produced OUTPUT/receipts/observations to each actual OUTER consumer. Include negatives for exposure of other requests, missing B observations,
   deletion of all observations and confusion between overall statistics and owned rows, together with a positive case of multiple requests on the same OUTER.
   A run that passes by correcting the actual producer's wrong routing with synthetic observations is not proof of this step.
3. Then continue with Cancel/Drain, bounded effect pump, capacity notification, actual EventNode/broker saturation, shutdown/join and
   repeated runs without restart. B1 touched-cost, B3 admission/edge credit, B4 policy and B5 native authority/
   actual placement/runner remain. All of the past U/P series is not made a serial precondition again.

The §1/H0 order of **VRAM-only sufficiency → larger RAM offloading models** does not change. Resource proof on the current single physical
host with 3090×2 is separated from the final multi-physical-computer proof, and local green figures are not raised to completion of the final goal.

### 2026-09-07 follow-up implementation — observation completeness audit and separation of report metrics

The observation path of the previous section was audited further in the actual code. Current production copies whole-request observations to each route,
sends spans only to the first owner, and consumption ends immediately after the terminal/receipt. Requiring coverage only for received executions
cannot detect the case where an entire bundle of observations and spans is lost. The target contract for this is locked down in
[the observation completeness section of the batching contract](adapter-batching-layers.md), and the counterexamples in verification protocol T20/T25/T57/T58.
This is **a read-only code audit and contract reinforcement**, not a new producer/consumer wire implementation or GREEN.

A report formula error that could be handled in parallel was fixed. A metric version was added to the actual `run.mjs::buildReport` consumption path, and
computed rows were separated from approved output tokens. Detailed fields, migration of earlier figures and denominators follow the current implementation in the
[harness README](../test/benchmarks/p4-4node/README.md). Run raw text including the 11 new report tests and independent
mutations is preserved in the settlement evidence. The existing Rust379 seal is unchanged; this full Rust re-run gave
**1122/0/7 ignored**, and extended JS **86/0**. Physical span aggregation, Rust per-request row rate and H4 final useful TPS were
not completed by this formula fix. No actual model/VRAM-only/RAM offloading/multi-computer real-hardware runs were done.

The user's resource expansion order stays as in §1. H0 clarified the classification so that even intended CPU computation is not approved as VRAM-only
when actual model computation/weights/KV depend on the host. General control/tokenize/CPU sampler/staging and
non-owned layers that do not compute are distinguished. The existing runner's GPU-fixed plan is not evidence of RAM offload support.

**B1/B2/B5 are IN_PROGRESS, and the first next action is still binding actual multi-OUTER observation production and consumption.**
This report fix is not permission to skip that prerequisite contract and re-tune GPU thresholds.

1. First lock down the batching contract's canonical publication evidence input as an independent literal vector, and connect it to `accept_prepared_issue`
   candidate validation/commit and to the new OUTPUT version for normal terminals. Forbid partial commits of the ledger and requests and clones of past
   history, and do not approve a different publication of the same volume from raw counts alone.
2. With that contract, connect per-owner head observations, fresh stage spans and effect fan-out, and implement the actual drive's
   Missing/Invalid/Complete verdict. Preserve the existing actual A/B captures and the old-version output oracle.
   Check late observations, loss of a whole bundle, a different attempt and confusion of physical whole/owned part on the actual path.
3. Next, continue with Cancel/Drain, bounded effect pump, capacity notification, integrated EventNode/broker saturation and repeated
   runs. B1 touched-cost/B3 admission and credit/B4 policy/B5 placement and runner also remain.

The current Rust/JS green proves only the defined local code scope. It is not promoted to a claim that all new target tests were run, or to
VRAM-only sufficiency or later success of larger RAM offloading models.

### 2026-09-07 follow-up implementation — internal publication evidence and the submission entrance

Of the previous first action, **only the internal publication evidence** was connected to the actual L1 approval path. Canonical input, dependent positions and atomicity
follow [the internal issued-work v1 section of the batching contract](adapter-batching-layers.md). The primitive, the actual L1 API and
2/4/8-stage actual Worker::run are cross-checked against independent bytes/digests and the existing output/KV/release oracles.
Missing approval records, early commit and missing execution input were detected with recompile mutations in an independent copy.

An additional code audit found that the P4 envelope and the lower-level approval identity allow NUL in different ranges. A RED was recorded where, on the actual worker,
6 inputs led to Uncertain and shutdown after computation/KV changes. This was fixed by rejecting them at the PREFILL entrance with the same identity
check. Normal Unicode, a separate correlation and normal resubmission on the same worker after rejection are kept.
This repair does not change backend-specific code or the P4 common string rules. Scope and final source/count/mutation raw text follow
[the internal witness section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**B1/B2/B5 are IN_PROGRESS**. The OUTPUT v4/observation wire/actual OUTER completion conditions have not been ported yet.
The current witness exists only in the internal RequestState and is removed at normal terminal, so it is not reported as externally verifiable
evidence or as detection of missing whole observations. The existing gaps in full row information, owned projection, late observation and effect
fan-out also remain. A bypass where received observations build their own expected set and approve themselves is forbidden.

**The first next action is to actually lock down the remaining row-string boundary at the submission entrance.** The code audit
confirmed that input whose serialized ReplySpec exceeds the logical/capsule 4096-byte limit can still be rejected only after tokenization/recording.
This is a code-only open surface and is not claimed as reproduced by this NUL RED.

1. Compare `worker.rs::prefill` against the existing string caps of logical/capsule. Distinguish the original JSON length from the post-escape
   ReplySpec byte length, and first build a counterexample with actual prompt/tokens input. Keep positives for normal boundary values,
   Unicode and independent correlation, and reject before tokenization, request/session key, slot, witness or effects on other requests.
   Do not raise the existing wire cap or report a late worker shutdown as a normal request rejection.
2. Then copy the already locked witness into **a new OUTPUT version for normal terminals**, and port producer/consumer
   together. Keep the existing v4 captures/output oracle. Separately from publication primitive/actual L1/worker mutations,
   add a counterexample where the actual OUTER cross-checks the final count/digest.
3. Connect per-owner head observations, fresh stage spans, effect fan-out and the actual drive's Missing/Invalid/Complete.
   Check last-observation delay, loss of a whole intermediate observation bundle, a different execution/position of the same volume, leakage to other owners,
   and span duplication/confusion of whole and partial statistics, together with a normal multi-OUTER batch.
4. Then continue with Cancel/Drain, bounded effect pump, capacity notification, integrated EventNode/broker saturation and repeated runs, and
   B1 touched-cost/B3 admission and edge credit/B4 policy/B5 native, actual placement and runner.
   Do not assume SHA recomputation/current row sorting and the existing prompt clone cost are free.

Do not add policy knobs or skip the correctness boundaries above with GPU utilization figures. Current resources and the real-hardware order
keep §1/H0's **VRAM-only sufficiency → larger RAM offloading models**. No model loading, real-hardware waves or
remote deployment was done this time, and the two 3090s in one host are distinguished from the final multi-computer proof.

### 2026-09-07 follow-up implementation — submission string boundary and preparation for observation porting

The serialized ReplySpec/options size boundary from the previous first action was locked down as RED on the actual Worker::run.
Inputs at the normal limit are kept, and a shared wire check was placed so that oversized inputs are rejected before session key recording, Tokenize and request admission.
The exact limit/ownership follows the [batching contract](adapter-batching-layers.md), and counterexamples/mutations follow the verification protocol.
Independent boundary regressions were also added to the existing Rust and model-free C++ codecs. Production C++, limits and CMake were not changed.
Run source, RED/GREEN, mutations and full counts are preserved in
[the submission string section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
The atomicity of other admission failures, such as existing Tokenize failure or context overflow, is not marked complete by this check.

**B1/B2/B5 are IN_PROGRESS**. The internal witness is not yet in OUTPUT, and missing whole observations cannot be judged either.
**The first next action is porting the actual production and consumption of the new OUTPUT and per-owner observation DTOs.** It is not completed by exposing only the internal hash
and leaving the current consumer's immediate exit on terminal/receipt.

1. Reusing the batching contract's canonical witness, build the adapter-owned types and strict validation for OUTPUT v5/BATCH_OBSERVATION v4/STAGE_SPAN v4
   together. Only the terminal holds the final evidence, and the existing wire is not silently
   extended. If request_issue_index is used, bind it to the count at head approval time, not to an expected total built by the receiver.
   Tests that incrementally handle gaps, reversed order and exact re-delivery of logical ordinals without a request come first.
2. Preserve approval evidence before RequestState removal in `release.rs`, and build head observations from the approved split + original submission.
   Separate owned rows per full OuterEndpoint from physical whole statistics, and keep only fresh spans. Pre-check all recipients
   and fix the post-forward time only once. Do not change the time on each retry or re-run native.
3. The actual drive cross-checks its own send authority, terminal witness, release set and all-stage coverage of the relevant execution.
   Missing waits within the existing overall deadline, and Invalid fails. Check early completion, deletion of all observations, replacement with a different
   execution/intermediate position, multi-OUTER leakage and lower-priority Full/Closed/ID exhaustion with the actual producer/consumer.
   The existing raw captures/token/KV oracles are preserved, and the new version is freshly captured from an actual run.
4. Even if waiting for observations takes longer, do not silently change the denominator of the existing release-boundary metrics. Latch the release time
   separately and separate the observation completion time, or explicitly migrate the summary version. Do not add owner-visible work into fleet-wide
   cost. Do not add native/backend types to the policy or ledger.

Remaining after this are Cancel/Drain, bounded effect pump, capacity notification, EventNode/broker integration, touched-cost,
admission/credit, batch policy and native placement/runner. No model or GPU/remote waves were run
this time. RAM offloading expansion after VRAM-only sufficiency on current resources and the final multi-physical-computer proof stay as in §1/H0.

### 2026-09-07 follow-up implementation — porting publication evidence into OUTPUT and observation completeness

The internal publication evidence from the previous first action was ported to the actual production/consumption boundary. OUTPUT v5 preserves the actually approved witness
only at normal terminal, and head OBS v4 and all stage SPAN v4 separate per-full-OUTER owned breakdowns from
physical whole statistics. Sole ownership of versions, fields and exact semantics lies with the [batching contract](adapter-batching-layers.md).
No such semantic types or dependencies were added to the neutral P4 envelope/core, native stage wire or llama/backend.

The actual drive derives expected volumes from actual send authority and terminal evidence, and does not succeed until the observations' issue chain/release/declared stage
coverage is complete. Late observations after output and release are allowed, but the original deadline is not
extended. Separating the release completion time from the observation completion time preserves the existing throughput denominator. The existing v3/v4 raw text and
token/text/position/stop, KV and release checks are kept, and the new wire was captured separately from the actual worker.

During review, a counterexample was reproduced on the actual drive where adding an unfounded empty-owner execution to a normal span still succeeded.
Now a **received** global execution stays Missing, even with an empty owned breakdown, until it is confirmed by the head physical size. This is not a rule
that requires a B-only stage span that never arrived at A. Normal span-before-head is also kept.
The exact run source, pre-fix failure, independent-copy mutations, full counts and limits are preserved in
[the OUTPUT and observation porting section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**B1/B2/B5 are IN_PROGRESS**. This check covers ownership and observation completeness within the current run, not a proof of actual llama tokenizer/quality,
GPU wavefront, cross-host clocks, independent device instrumentation of overall statistics, or complete distributed drain.
The actual worker using fixed native responses is distinguished from the actual drive replaying captures. Offline acceptance does not
independently re-run the online ledger checks. The local seal in target is not a persistent deployment bundle either.

**The first next action is to lock down the counterexample where an actual normal settlement ACK stalls during completion Full.**
`worker/effects.rs::flush_effects` → `worker/emit.rs::publish_or_wait` waits on the worker thread, so
even the existing input/issue quantum cannot read control events. Simply replacing the sleep with a waker does not create
control consumption opportunities for the same actor. Proceed in the order below.

1. On the actual worker, hold A's RELEASED, saturate the queue with B's completions, then send A's exact ACK.
   Use test-only read observations to distinguish actual input admission, saturation and non-processing. After reproducing the cause, reopen output space and
   also run the positive case that keeps the original OUTPUT/receipt/observation bytes, order, exactly-once delivery and all-stage settlement.
   Do not declare saturation/starvation from elapsed time alone without source changes.
2. Design a bounded effect pump that preserves immutable committed intents, and capacity notification. Full keeps
   unpublished work; Closed/ID exhaustion is an explicit failure, not a native re-run. Give control consumption opportunities while respecting effect dependency order and settlement
   authority. Unbounded side queues, bypass slot returns and substituting ACK=KV completion are forbidden.
   A shared mailbox change is checked together with the neutral contract of mock/other adapters, lost-wakeup, close and actual bidirectional saturation.
3. Connect the actual EventNode/broker's input end with late completions already admitted. Do not call the current local-close Ok
   a graceful drain. Split into separate contracts/versions the adapter-owned Cancel/Drain authority, acceptance ACK, publication prohibition, settlement of existing native work,
   all-stage release and final OUTER delivery, and also check cancellation without output and duplicates/restarts before and after.
   Do not substitute legacy service commands for event commands that are not in the code.
4. Continue B1 touched-work/immutable input clone cost, B3 bounded admission/KV reservation/row and byte credit, B4 policy, and
   B5 actual placement/ABI, isolation and runner through their respective gates. A drive ledger that keeps all observations in memory
   does not count as a bounded-RSS proof. Saving a partial artifact on error is also still separate work.

This slice's convenience `SubmissionLedger::approve_output` is used only by tests, not by the actual drive, and an
unused warning remains. The source frozen for evidence was not quietly cleaned up at the end; follow-up code edits will
narrow the scope and re-verify. Model loading, GPU/remote deployment and VRAM-only/RAM offloading were not done in this slice.
The current fleet's order of **VRAM-only sufficiency → larger RAM offloading models** and the final multi-computer proof stay as in §1/H0.

### 2026-09-07 follow-up implementation — the actual completion Full counterexample and space notification

The previous first action was reproduced on the actual Worker::run. With A's exact ACK held after A's native release, and completion filled with B's
OUTPUT, A's ACK was admitted to input but not processed. After space recovered, the existing
outputs, releases and observations all completed. The new required test is not hidden and its expected value is not lowered; **it stays RED**.
The original source, actual recompile, executables and recovery positive are preserved in
[the completion Full section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

Space notification in the neutral mailbox and the reader-close wake are prerequisite work for this actor problem. The implementation and verification results of that primitive
follow the same evidence section. Adding them does not remove the current blocking of `publish_or_wait` or
EventNode's input retry/graceful drain. **B1/B2/B5 are IN_PROGRESS**, and while this
RED remains, the earlier full green counts are not cited as current counts or promoted to a performance phase.

**The first next action is to connect the bounded effect pump to the actual worker and make this RED pass.**
The exact target semantics are owned by [the output saturation section of the batching contract](adapter-batching-layers.md), and the tests by verification protocol
T22–T26. The already reproduced ACK starvation is not investigated again as a new finding or bypassed with environment variables.

1. First make the apply/send stages of head control intents explicit and add an early-ACK rejection counterexample. Keep the existing normal
   RELEASED, SETTLED with a terminal proposal and checkpoint replay together.
2. Integrate direct LOAD/SESSION/UNLOAD/error responses under the same fixed outbound payload and effect reservation boundary. Count/byte
   reservations are secured after full candidate validation and before commit, and the space to keep native results is secured before the call.
   Arbitrarily shrinking existing wire limits or moving to another unbounded queue is not fulfillment.
3. Connect an actor loop that waits on input, capacity and shutdown together. Normal ACKs progress, but backlog is not grown with new native
   publications. Even on Pending/Closed/ID exhaustion or shutdown after partial native success of an effect,
   keep the original Event/order, exactly-once execution and uncertainty/abandonment accounting. The existence of a capacity API alone does not complete this.
4. After verifying actual EventNode/broker saturation and explicit Cancel/Drain in turn, proceed with the remaining B1 touched-cost,
   B3 admission/reservation/edge credit, B4 policy and B5 placement/ABI/isolation/runner.

This slice does not perform model loading, GPU/remote deployment or real-hardware waves. The user-specified resources of §1/H0 and
the RAM offloading expansion order after VRAM-only stay as they are. Two GPUs in one physical host are not counted as multiple computers.

### 2026-09-07 follow-up implementation — apply/send authority of head control

The head control stage, the first sub-task of the previous action, was implemented. The counterexample where an early ACK returns a slot or
resumes Verify/Replay on pending registration alone was preserved as pre-fix RED on the actual codec→handle. Local native success and
acceptance of the send to the next stage were bound as separate states, and the existing tail proposal/Replay and whole-event rejection were kept.
The actual effect consumption tests pass native Frame responses, receipt/frontier and send acceptance separately. Consumption tests that inject initial state
are distinguished from evidence of actual run-loop progress. Semantics and ticket lifetime are owned by the [batching contract](adapter-batching-layers.md),
test obligations by verification protocol T23, and seals, runs and mutations by
[the head control section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**B1/B2/B5 are IN_PROGRESS**. The actual completion Full ACK starvation test is still a required RED, and
blocking publish, the unconnected capacity API, the full outbox budget and Cancel/Drain were not fixed this time.
The existence of a phase field does not mean the actor yields, nor does send credit prove KV settlement.

**The first next action is to connect the fixed outbound payload and full effect reservation to the actual consumption boundary.**

The starting counterexample was locked down as `SESSION` response ID exhaustion in an independent copy. `control.rs::Worker::session`
performs sessions.insert before the ID check in `emit.rs::Worker::emit_bytes`, so the route is
installed even after rejection. The two actual paths session()/handle() are RED, and the normal response wire positive passed. This copy's tests are not added to
the original full 1225 count. First port this counterexample into the default regressions to close **no state change before SESSION response preparation
and reservation**. The ID/outbound payload preparation tested at this small consumption boundary is not called a native result budget or
completion of full outbox reservation, and the work continues along the full path below.

1. Audit retention cost and reservation lifetime including every direct send in `worker/effects.rs`, `emit.rs`, `control.rs` and `drive.rs`.
   Capacity for the OUTPUT/ACK receipts that already-published work produces later must be secured before
   publication. Reserving for the first time when the ACK arrives lets other OUTPUTs fill the budget, and the same starvation recurs.
2. Reserve count/retained-byte/ID for the full candidate before state commit, and resubmit the once-built Event as is
   on Full. Do not exclude the current native frame cap, result and observation fan-out, or base/payload copies.
   Do not discard results with an arbitrary small cap after receiving them, and do not claim an RSS cap from fixed-size queues alone.
3. In the asynchronous effect pump that consumes that reservation, wait on input, capacity and shutdown together. Tickets for synchronous sections
   are not reused after yielding; they are re-validated. Yielding inside a native command first requires a separate group reservation.
   Keep both the actual Full ACK RED and the normal delivery positive, and make them pass.
4. The order after that follows the EventNode/broker, Cancel/Drain and remaining B1/B3/B4/B5 items of the previous progress record.

The source is uncommitted changes on the baseline HEAD. Model/GPU, C++/remote runs, deployment and commit/push are not outcomes of
this slice. §1/H0's **RAM offloading model expansion after VRAM-only sufficiency verification** and the final multi-computer proof are kept.

### 2026-09-07 follow-up implementation — the consumption boundary of SESSION response preparation

The previous first counterexample was ported into the default SESSION regressions. Response serialization, ID candidates and the feasibility of an actual Event round trip
are checked before any state change, and only on success does it proceed to session install → ID commit → the existing synchronous send.
An additional counterexample, where the input is wire-valid but the original ID is duplicated into the response ID/causation and grows the response envelope, was also
reproduced in an independent copy. Plain Event::validate or encode success does not guarantee decoder
acceptance. The existing normal wire and Unicode metadata/body were kept. The exact tests, seals, counts and mutations are owned by
[the SESSION response preparation section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**B1/B2/B5 are IN_PROGRESS**. This private preparation is only for the same worker's synchronous section, and is not a queue count/
retained-byte/future native result reservation. The overall wire limit was not reduced and the neutral protocol was not changed,
and ordinary ERROR/LOAD/UNLOAD responses do not yet have this preparation boundary. The ERROR fallback for a large original ID may also be
wire-invalid. On preparation failure, preservation of SESSION authority is distinguished from delivery of a normal error response.
The current completion Full ACK starvation remains a required RED, and session rollback after Closed is not in this scope either.

**The first next action is to bind the retained representation and budget of effects to actual publication/settlement obligations.**

1. Start by making explicit and reducing the cost of cloning the full TAIL base Event for each OUTPUT in `effects.rs` and cloning the front in flush.
   Where only the envelope is needed, avoid keeping duplicate full bodies, but first lock down a consumption regression where the actual
   OUTPUT/observation/control Events have identical identity, body and order.
2. Reserve the full TAIL effects in `release.rs` together with **the owner receipts that future RELEASED will generate**,
   and reserve the result/forward/observation obligation caps before the native call in `drive.rs`. Planned reservation and
   the actual publication witness are separated. A structure that first requests the general budget when the ACK arrives is not allowed.
3. Instrument not only counts but also retained bytes, nested telemetry fan-out, parsing/Vec capacity and temporary copies.
   Do not use the current frame's 2GiB limit as the RSS limit. Pre-allocation by the declared count in `capsule/decode.rs::read_capsule`
   must also be checked against the budget/valid input length, and a refutation that exhausts the development host with an actual large allocation
   is forbidden. Use a negotiated cap or safe isolation and instrumentation.
4. After all direct responses and future obligations are inside the same retention boundary, turn on the capacity/input/shutdown actor pump
   to make the existing Full ACK RED pass. Then continue with EventNode/broker, Cancel/Drain and the rest of B1/B3/B4/B5.
   Do not forget that ordinary ERROR has not been ported and declare saturation solved from the normal token path alone.

The real-hardware expansion order stays as in §1/H0. The model file listing this time was a read-only path/stat survey; no load, GPU or
VRAM-only/RAM offloading waves were run. The source is uncommitted changes, and there is no automatic push/deployment.

### 2026-09-07 follow-up implementation — retained effect representation and pre-allocation checks

The effect representation, the previous first task, was ported on the actual production/consumption paths. Not only OUTPUT but also Forward/observation/
release notice provenance own only the Envelope, and flush moves ownership instead of cloning the whole effect.
On failure, the body and nested observations are restored to the original intent. The path where a capsule's wrong outcome/generated declaration
pre-allocated a Vec even without a body was blocked with a minimum wire size check. The exact code seal, tests and mutations are owned by
[the retained effect representation section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**B1/B2/B5 are IN_PROGRESS**. The actual completion Full ACK starvation is still a required RED. Reduced copying in the retained representation
is not a proof of a full RSS cap, fixed Event resumption, throughput improvement or a completed asynchronous pump.
Active input/parse objects, temporary serialization, in-flight effects, and future result and notification obligations must be accounted for separately.

**The first next action is to connect the Worker-lifetime ResourceBudget to actual response/publication/return consumption.**
The detailed reservation, ID and lifetime contract is owned by [the output saturation section of the batching contract](adapter-batching-layers.md), and the failure counterexamples by
verification protocol T22–T26. The already completed representation/check work is only re-audited, not repeated.

1. Pass resource declarations through the composition root → adapter construction settings, and connect a Worker-owned reservation ledger.
   Sum general retained objects, inputs, native scratch space, future returns and failure diagnostics. Do not substitute queue counts or the native frame
   limit for RSS. Include consumption tests where a reservation failure for an actual response/native candidate preserves state, IDs and effects.
   This step is not closed by only creating an unused pure budget class.
2. Attribute future owner receipts and ID issuance count reservations to the pending release of a TAIL candidate. Check authority per original submission/
   operation, and let a normal ACK settle with its own reservation even when the general budget is full.
   Verify wrong ACKs, full group overruns, ID headroom boundaries, duplicate transitions and native uncertain states with an actual consumer.
3. Complete the same retention boundary for direct LOAD/UNLOAD/SESSION/ERROR and native result/forward/observation.
   Then let the actual actor handle input, capacity and shutdown together, and make the existing Full ACK RED pass.
   Distinguish saturation at the front of a single FIFO from progress of already admitted ACKs, and do not yet yield inside a native group.
4. Continue with actual EventNode/broker saturation, Cancel/Drain and the B1/B3/B4/B5 remainder. Do not approve progress across the full cyclic network
   without a reserved return input path/edge credit. After safety approval, move to the B6/B7 real-hardware waves.

§1/H0's **RAM offloading model expansion after VRAM-only sufficiency verification** is kept. This slice has no
model loading, GPU waves, remote deployment, C++ runs or commit/push. It is not yet the final multi-computer proof either.

### 2026-09-07 follow-up implementation — re-review of the ACK progress design and a full checkpoint

The user pointed out excessive time and tokens, repeated local repairs and a long stretch without commits. Re-checking the overall causes showed that
the previous record's **decision to make the full ResourceBudget a serial prerequisite of the current ACK stall fix
went too far.** Full RSS, native scratch space and EventNode credit are needed, but they are not preconditions of this local counterexample.

In the actual counterexample, what occupied the mailbox was the already sent B OUTPUT, and what the worker waits on is B's RELEASE forward
after it. A is already ForwardAccepted, and its normal ACK is in the input. The minimal unit of the fix is preserving the following transitions together:
keep the same active Event → pure prepare/commit of the reachable ACK → preserve receipts/diagnostics behind the existing FIFO → re-validate head authority
right before each offer → success callback.
Preemption inside a native group, recursive handle/flush and additional native issues are not allowed.

This bounded service and a count check of queued/active-suffix/future-receipt IDs were integrated into the current working tree, and
all cumulative source, tests and documents are committed together as a **WIP checkpoint**. The exact source seal, full counts and the regressions
not yet passing are owned by the last checkpoint record of the [settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
The earlier full 1244/1/7 is not reused as a result of the new fix.

**B1/B2/B5 are IN_PROGRESS**. A normal ACK can create new receipts only up to the number of pending authorities, and
additional general retention is limited to 1 FIFO input and 1 diagnostic. This is a structural limit on the number of additional objects, not
a full byte/RSS reservation or a cap on native response retention. The native frame limit is not used as a RAM budget.
ACK progress after a non-ACK blocks the FIFO front, or after a second wrong ACK is retained, is outside this local guarantee.
The 1ms wait, integrated capacity/input wake, a complete fixed outbox, all-path byte budgets and Cancel/Drain are also not complete.

The first next action is **to prove the single bundle of fixes below**. Do not first add new performance knobs or a general resource model.
Do not weaken existing normal/rejection expectations to fit failures.

1. Make the existing actual Worker Full ACK counterexample, early ACK/full group rejection and normal recovery pass unchanged.
2. On the same actual harness, confirm a normal ACK after 1 wrong ACK, non-ACK retention/FIFO recovery, and SETTLED's direct
   proposal/Replay. Also assert that normal ACK settlement does not trigger new native execution.
3. Verify same-payload retry, a fresh head ticket every time, future ID obligations/queued suffix, overflow/exhaustion and leftovers at shutdown
   with the actual consumption path and independent-copy mutations. A compile error or a test not run is not mutation detection.
4. Record the sealed final full tests, mutations, run scope and remaining limits, and commit all changes again.
   Then proceed with capacity wake/return admission path, byte budget, B3/B4/B5 and the approved VRAM-only→RAM real-hardware runs.

No global guarantee is made that there are no exceptions under any input or fault. Each guarantee states its state, failure model, entry path and
out-of-bounds cases together. After an intermediate commit, 0 non-ignored changes are confirmed, and GPU, deployment and push are not part of
the verification or permissions of this checkpoint.

### 2026-09-07 follow-up implementation — verification of bounded ACK progress and the second full checkpoint

The 196 cumulative changed files were first committed as the `2e9451a5c` WIP, and 0 non-ignored leftovers were confirmed. Then the checkpoint's
9 regressions were fixed and 8 actual consumption-path tests were added. Pre-commit obligation checks and post-commit delivery faults were
separated, and the expected values of the existing tests for the latter were kept. ID rejection for native publication also happens **before** installing the prepared issue.
Over the full 399 Rust input seal: **1253 passed/0 failed/7 ignored**, 57 summaries, cargo0.
Removing each of the 5 fixes in an independent copy makes an executed test fail, and restoring it passes again.
The exact inputs, test names, commands, mutations and hashes are owned by
[the second checkpoint section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**The counterexample closed this time** is the local starvation where a normal RELEASED/SETTLED reachable in the FIFO was not settled
because of completion Full on the same worker. On the actual worker, a normal ACK after 1 erroneous ACK, original retention and
order recovery for non-ACK, settlement of Direct/Checkpoint SETTLED before space recovery, and 0 native re-entries were checked. A separate actual
flush test re-validates and rejects a control retransmission whose authority retired because of an ACK during Full, and preserves the original intent.
This last case is not absorbed as a success and is left in a fenced state. Transparent reclaim of completed retransmissions is not implemented.

**B1/B2/B5 are still IN_PROGRESS**. This change does not complete byte/RSS reservation, progress under all inputs, a complete outbox,
credit across the transport/node cyclic network, graceful Cancel/Drain or performance improvement. ACKs after a second error or
a non-ACK are not overtaken, and the 1ms wait also remains. These boundaries are not hidden behind a claim of "saturation solved".

The first task of the next session is **to verify the return admission path, not to rebuild the currently correct ledger/settlement**.

1. Lock down the ownership, capacity and wake flow of actual EventNode→adapter input→completion→broker in one table.
   First build a minimal cyclic-wait counterexample where a non-ACK front and output Full overlap. Do not infer and confirm a not-yet-observed
   global deadlock from this local counterexample. Decide an ACK-only admission/reservation path together with the FIFO contract.
2. Connect the integrated input/capacity/shutdown wake and the return path to that counterexample. Do not solve it by adding arbitrary sleeps/thresholds,
   native re-entry for ACKs, or unbounded drain of ordinary requests. Check rejection preservation, duplicates, reconnection, shutdown and
   completion of normal mixed requests in the same run, and lock them down with independent mutations.
3. Connect a byte budget covering active Events, held inputs, effects, future returns/receipts and native scratch space to the actual path.
   Do not merely rename the current ID count check to a byte reservation. After passing the B3/B4/B5 remainder per the verification protocol,
   move on to B6/B7's approved **VRAM-only→RAM offloading strong waves**.
4. Record each cohesive change and its verification results as a full intermediate commit. Confirm 0 non-ignored leftovers, and mark failed
   checkpoints as failed. Build/model/raw run outputs are ignored; source, tests and documents are not omitted.

This second checkpoint also did not run C++, an actual model, GPU, remote deployment or push. The final real-hardware goal of normal
full prompts/responses of a very large model with useful TPS and GPU utilization remains as in §1/H0/H6.

### 2026-09-07 follow-up review — cross-check against the external review and re-examination of work scope

The external review's 11:43–11:45 snapshot and **1236/9/7** are results from the time of the first WIP. The later full checkpoint
`96c90f99e` is **1253/0/7**. The 9 regressions are not hidden by relaxing expected values or recorded as ResourceBudget
completion. The current state is local progress of reachable ACKs and an ID count obligation check, and B1/B2/B5 are
IN_PROGRESS. The exact runs and the results of this small API cleanup are owned by the
[settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

This scope review **withdrew** the inclusion in Git of the generated raw data bundle and of an archiving tool specific to one commit.
Adding a repeated hash list of already committed source does not make runs reproducible on another machine.
They are not deleted but kept in an ignored path, and the long-term evidence retention/re-run conditions remain unmet.
The batching work is not expanded into archiving tool development. A full commit means including all non-ignored source, tests and documents
without omission, not indiscriminately including generated outputs.

**The first next action is the same return admission path verification as in the previous record.** Start by locking down a minimal counterexample connecting the actual EventNode, broker and Worker,
and a normal progress oracle. The code-level candidate is a cyclic wait where normal inference and allowed SESSION
re-delivery fill each input/output queue together. It is not yet an executed RED, and it is not confirmed to be a defect of pure PREFILL
waves alone. The adapter in the existing duplex test always accepts input, or
the test gives headroom from outside, so it does not prove this candidate.

Before the test, record each held Event's owner, queue cap, wake, target and finite reachability order. Do not implement a new ACK lane/reservation policy/threshold
before reproduction. Even after reproduction, keep the policy/ledger and llama/backend boundaries,
and first define the acceptance conditions for normal, saturated and shutdown cases. At phase transitions, apply the
[trial-and-error check of the verification protocol](distributed-batching-verification.md). Do not repeatedly rewrite the current local fix
or search for this consistency design with GPU experiments.

### 2026-09-07 follow-up verification — the actual actor cycle counterexample and the fix boundary

The baseline is `f13e2560b`. Without operational changes, two tests of the actual EventBroker→EventNode→LlamaNodeAdapter→Worker::run
were added. Only native computation/finite delay and post-LOAD initial setup are fake; normal SESSION,
inference, capsule and release state are produced on the actual path. The runs of the earlier review were on the pre-reinforcement source, so they are not reused as results
of the current source. The exact runs, failures and holding-owner table are owned by
[the actor cycle section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).
Run 13 over the sealed 400 inputs gave 1254/1/7 ignored, cargo101; only the final cap1 progress assertion failed, and cap8
passed. This is a pre-fix RED checkpoint without operational changes. The earlier 1253/0/7 is not read as the current state.

This verdict concerns B2/B3's **admission path and the follow-up space guarantee of the causing work**. It is not work to widen the bounded ack_service,
forbid SESSION, enlarge queues/thresholds, or look for the stall cause again with GPU A/B.
A capacity wake only announces freed space; it cannot create space in an already closed wait cycle. The order of the same source/
correlation and the space for required results must be kept together, so a single ACK-priority lane cannot complete this.
The semantic contract is owned by [the output saturation section of the batching contract](adapter-batching-layers.md), and the neutral boundary by the isolation contract.

**B1/B2/B5 are IN_PROGRESS, and B3 end-to-end reservation is incomplete**. The first next action is to leave the sealed counterexample as a separate
checkpoint, then settle the next implementation scope as a closed state-transition table.

1. Before approving the causing work, secure in advance the count/retained bytes for required results, returns and earlier same-order outbound payloads.
   The adapter is responsible for flight/KV semantics, and the neutral delivery layer for space/ownership. Keep ordinary input from consuming the space of reserved returns,
   and make an implementation that rejects all work also fail against the same normal-input control.
2. Connect the responsibility transfer of completion→EventNode→broker→worker and the Full/Closed/duplicate/cancel/unknown-outcome transitions
   to the actual consumption path together. Also secure the required effects after ACK processing, and do not misread ID headroom as a byte budget.
   Connect capacity notification together with lost-wake verification of register → re-check condition → wait.
3. Handle the Full→connection close of remote `transport.rs::serve` and the cross-destination wait of the shared outbound pump at the same
   boundary. Do not promote local actor GREEN to proof of remote progress. A design that carries grants in the envelope
   needs a wire version, producer/consumer, rejection of old peers and a neutral responder that knows nothing about llama.
4. If the fix prevents saturation from forming at all, put a normal-progress branch in the test setup. Keep the 14 submissions, 6 results,
   native KV/release and order oracles, and do not distort the implementation to force a wrong saturated state.

The three-round limit is not reset under a new slice name. After the request, the earlier record's additional run 12 is the first
round, and this fixed counterexample/normal control plus full regression bundle 13 is recorded as the second. The last round is not
spent exploring candidates whose design is not yet settled. If the premise that the existing local fix closes the whole cyclic problem
does not hold, report that a redesign is needed; completion of all of B1–B8 within three rounds is not guaranteed.

### 2026-09-07 follow-up implementation — local store reservation foundation and a pre-run checkpoint

The pre-fix RED was preserved separately in `393a6c23e`. This time, instead of choosing the implementation after seeing run results,
the reserve→publish→owned dequeue→transfer/retire transitions of the original Event/space claim were fixed first.
The actual completion store was connected to the same ledger, and retention cost is counted as the capacity of every owned String/Vec.
Existing ordinary publication also uses the new store. **This is unverified WIP that changes operational store code**, not
product activation of the reservation path or a repair of the end-to-end deadlock. Detailed API constraints are owned solely by the batching contract.

A static code review ruled out treating a permanently oversized single Event as a Full retry, and re-issuing a RELEASE preview ID
later. The former can never be accepted no matter how long it waits, and the latter conflicts with ID consumption by earlier effects.
Switching only the producer to the reservation API, or reserving all of multiple results in a cap1 slot, also blocks normal progress.
No additional test runs were done to confirm this verdict.

**The last execution round has not been used yet.** New cost/store/actual publication rejection/RELEASE oracles
were written, but compilation, tests and mutations were not run. The earlier 1254/1/7 remains only as a result of the pre-fix source.
The existing mailbox tests and actor cap1/cap8 inputs/oracles were not changed. For the detailed change and not-run list, see
[the local store section of the settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**B1/B2/B5 IN_PROGRESS, B3 end-to-end incomplete**. The first next action is not to switch on the new API unconditionally, but
to connect the reservation/responsibility transfer between the causing work's follow-up effect retention and the receiver's actual space.

1. Create the Event once in the actual FIFO outbox and convert the ID obligation into an actual number. Port reserved store
   production and owned consumption together with EventNode/broker. Distinguish the known response representation of RELEASE from the variable native
   result cap. Do not substitute an arbitrary-multiple budget for the current HELLO, which has no cap on the full native result.
2. Use the existing cap1/14 inputs/6 results unchanged as the approval criteria. Only if preventive progress makes setting up saturation unnecessary,
   add a branch with an actual progress witness. An implementation that does not submit normal inputs or rejects them forever
   because of the new budget is a failure. Remote, the same ordering domain and cancellation/shutdown are not dropped from the existing verification scope.
3. After sealing the candidate, counterexamples, normal controls and mutation list, run the last verification bundle. Do not spend
   the last round testing only the current partial API, or reset the rounds under a new name. Unverified changes are also preserved as a full WIP
   checkpoint, but not reported as complete or as performance improvements.

This checkpoint has no model/GPU/C++/remote runs or push. All non-ignored changes are included, and generated outputs stay in
the existing ignore paths. The final goal of VRAM-only→RAM offloading strong waves does not change.

### 2026-09-07 follow-up implementation — fixed outbound payload and static review before connecting reservations

The earlier store WIP was preserved in `7f402aba5`. This change is the step that turns a committed effect into an Event only once, at the actual FIFO head,
and preserves that Event and its follow-up obligations even on final publication failure.
The existing Full path already retried the same Event internally. This fix is not the first implementation of that fact.
To make broker Full also return the original allocation, securing the actual destination slot is placed before copying.
The exact state distinctions of the contract are owned by the [batching contract](adapter-batching-layers.md), and tests/static review by the
[settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**This is WIP with compilation, tests and mutations not run.** The 8 tests written and the port of the existing failure-representation tests are not counted
as passes. Two independent static reviews confirmed ID obligation preservation and head authority re-validation, and the missing full
telemetry/Envelope cross-check in the test port was reinforced before running. The inputs/oracles of the pre-fix actor RED are unchanged.
This is not advance space reservation or a yielding implementation for the synchronous worker, so no resolution of the actor deadlock is claimed.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end incomplete; the last verification round is unused**.
The first next action is not to test a partial API but to actually connect the admission/responsibility transfer design below.

1. Confirm which store returns and ordinary inputs each occupy in the current synchronous FIFO, and separate the retention space for all required effects of the causing work
   from delivery slots. Do not put clonable RAII authority into the pure candidate ledger.
   Connect opaque obligation IDs to worker-owned linear space authority.
2. PHYSICAL of the same `(source, correlation)` and the observations that follow it keep their order even when destinations differ.
   Do not close the design with only a special case that pulls SESSION responses out first or unconditional per-destination bypasses. Define ownership/rejection transitions
   through the actual receiver-side space, required effects after ACK, remote Full and the variable native result cap.
3. If the required space is not available before approving the cause, hold normally, while reserved returns of existing work must progress.
   An implementation that blocks all input must also fail against the existing 14 inputs/6 results control. The implementation separates the general neutral delivery API from
   the adapter's flight/KV semantics, and connects through actual EventNode/worker consumption.
4. After sealing the completed candidate and the bundle of fixed counterexamples, normal controls and mutations, run the one remaining round. Do not repeatedly use compiler or
   test output as the basis for the next design choice. Record unexpected failures as they are, and
   do not claim to have guaranteed all of B1–B8 or exception-free global completion within three rounds.

This time too, all non-ignored changes are included in the WIP checkpoint, and generated outputs stay in the existing ignore paths.
GPU/model/remote/C++/push were not run. The final outcome promotion conditions and approved resource scope stay as in §1.

### 2026-09-07 follow-up implementation — separating delivery slots from required result retention space

The earlier fixed outbound payload WIP was preserved in `bcbadf101`. This time, the atomic reservation that holds known fan-out results was separated from
the small delivery queue. The option of requiring slots for three results at once in a single queue cell was ruled out because even normal work could not
start. This is not tuning that raised a cap after a test failure but a separation of responsibilities between retention space and delivery slots.
The cost of the group metadata itself, contention re-checks, and the boundaries of cancellation and wake are owned by the [batching contract](adapter-batching-layers.md).

**This is WIP with compilation, test runs and mutations not run.** Regression oracles for 10 group reservation cases and 6 queue/retained separation cases were
written and only statically cross-checked. Ordinary publication also changed, so this is not called a no-op refactor.
The existing actor counterexample's inputs, cap1/cap8, results and native oracles are unchanged, and no end-to-end GREEN is claimed.
The boundary where raw consumers cannot read the reserved front if only the producer enables reservations also still remains.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end incomplete; the last verification round is unused**.
The first next action is not to build more separate reservation APIs but **to connect production → owned consumption → receiver-side
responsibility transfer of one causing work on the actual EventNode/worker path**. First close the three connection conditions below at once.

1. Bind the obligation secured in advance with the cause ID/response slot once, in the FIFO, to the final Event ID/sequence.
   Do not put RAII claims into pure candidate/ledger copies; the worker owns linear claims. Also preserve earlier outbound payloads
   in the same ordering domain, and count the broker's exact duplicate ledger retention cost independently.
2. Distinguish PublicationBlocked/EffectsRunnable/Idle/NativeInProgress/Fenced/Closing, and
   wait on input+capacity+shutdown together. Turning Full into a plain Ok and dropping down to the existing `receiver.recv()`
   can wait forever even after space returns. Do not couple ACK settlement with securing resources for new decode publication.
3. Separate known SESSION/control responses from variable native results. The current HELLO's row/sequence limits and
   the receive frame cap are not a prior byte cap on cut tensor count/shape/dtype/alias. Do not attach an arbitrary-multiple budget to the path that
   allocates with `output_desc.nbytes` after native execution. Without an actual result bound and a
   remote grant/Full, cancellation and shutdown acceptance contract, that part stays incomplete.

Do not spend the last round on partial API tests before finishing this connection. Seal the new tests, normal controls and removal mutations on
the same candidate source, and judge progress on the existing 14 inputs/6 results/external dequeue0. Do not go back to continually changing the source during preparation
and choosing the design from GPU figures or test output. Keep only the required source, tests and contracts as a full
WIP checkpoint, and ignore generated outputs. The performance and multi-computer real-hardware goals are not yet complete.

### 2026-09-07 follow-up implementation — PREFILL rejection atomicity before connecting admission

The earlier store separation WIP was preserved in `d8fff7d27`. While statically tracing the actual production→consumption connection, a premise was found where PREFILL
records the session key before context/incarnation/admission validation. Connecting that as is to input processing during saturation
would leave traces even for rejected requests, so instead of adding a new reservation API, **the first write boundary of the actual admission function
was moved later**. Reservations are not connected yet. The bounded guarantee and the changed error priority are owned solely by
[the L2 section of the batching contract](adapter-batching-layers.md#l2-acceptance-and-occupancy-admission).

7 actual codec→handle/direct PREFILL oracles were written in `prefill_admission_tests.rs`. They check together state preservation on rejection,
1 diagnostic, resubmission after fixing the cause, and normal FIFO with existing pending first.
**This is WIP with compilation, tests and mutations not run**, and it does not claim that actual failures were executed. The existing actor cap1/cap8
verdict of 14 inputs/6 results/external dequeue0 was not changed. The last run result is still the pre-fix 1254/1/7.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end incomplete; 2 verification rounds used, the last 1 unused**. The first next action is,
on top of this premise fix, to connect **the actual responsibility transfer producer→owned EventNode→broker→receiver** as one bundle.
Do not break the accounting with a raw compatibility bridge, or tie source claims to the exact dedupe ledger.
The existing incomplete surfaces — the blocked worker's input/capacity/shutdown wait, per-cause required result and return space, the native result bound and
remote acceptance — remain as they are. The local PREFILL fix is not claimed to close the whole deadlock.

First finish the static cross-check of code paths, owners and error kinds, then seal the completed candidate/counterexamples/normal controls/removal mutations before
using the last round. Progress conditions are not changed by new knobs, enlarged queues or shrinking failing inputs. Only the source,
tests and owned documents to keep are included in the full unverified checkpoint, and generated outputs are ignored. GPU/remote/model/C++/push
were not run in this work, and the final strong-wave outcome is incomplete.

### 2026-09-07 follow-up implementation — original ownership on actual delivery rejection

The earlier PREFILL WIP was preserved in `2b1d1d539`. This connection audit traced the raw/owned boundary through all actual callers.
An opt-in bridge that copies the Event midway and drops the claim, and the option of tying source claims to the lifetime of the exact dedupe
ledger, were ruled out. Owned porting of the whole success path has not been done yet.

What was actually changed first is **the canonical rejection return and its consumers**. The broker returns the original Event on every rejection,
adapter input Closed also returns the original, and EventNode terminal returns items held in both directions. The product node
task was connected to preserve that result. The exact lifetime/incomplete scope is owned by the
[neutral event contract](event-protocol-v2.md#local-refusal-ownership--limited-implementation-boundary).
This is raw Event preservation, not space claim transfer or actor deadlock GREEN.

4 broker, 3 actual node loop and 2 llama try_offer regressions were written, and the existing API tests were adapted to the new return value
while keeping the cause/raw-text cross-check. The original actor 14 inputs/6 results/cap1 and cap8/timeout/oracle are unchanged.
**WIP with compilation, tests and mutations not run; B1/B2/B5 IN_PROGRESS, B3 end-to-end incomplete**.
The last actual result, 1254/1/7, proves only the pre-fix sealed source; 2 verification rounds used, the last 1 unused.

The first next action is to port the canonical owned success path at this actual boundary, which now returns failed originals.
Connect producer/held input and output/destination/WorkerInput/long-lived request originals together, and keep the independent cost of exact
duplicate copies and notification outside the lock. A raw fallback or `handle(event.clone()); retire()`
leaves a long-lived copy unaccounted for and is not approved. Not as a later step but **as a promotion condition of the same candidate**,
the causing work's required result/return space and the input/capacity/shutdown pump must be connected before judging cap1 progress.
The prior native result bound, remote grant/acceptance and normal prompt waves remain incomplete.

Only the operational changes, required regressions and owned documents to keep are included in the full unverified checkpoint, and generated outputs are ignored.
No remote/GPU/native/C++/push runs this time, and no policy was added that introduces new limits or reduces normal inputs.

### 2026-09-07 follow-up implementation — direct response FIFO and the notification boundary

The earlier rejection ownership WIP is `658c9cded`. This time, the original and duplicate copies of successful delivery were separated, notification after the actual completion
enqueue was separated, and LOAD/SESSION/UNLOAD/error responses were also connected to the existing committed FIFO.
**This does not port the whole canonical owned success delivery.** The actual broker still uses raw queues.

The static cross-check examined together a narrow shutdown diagnostic boundary that does not lose the existing publication of 1 native error, and
preservation of the remaining diagnostics after the first Closed in a batch. Pre-validation of the maximum future ID width is a contract change that conservatively
narrows the acceptance range of boundary inputs. A test that counted an existing malformed ERROR publication as success was changed to require, with the same input, no publication,
no authority approval and diagnostic preservation. The contract/required connection scope is owned by the [batching contract](adapter-batching-layers.md),
and oracles and limits by the [settlement evidence](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md).

**B1/B2/B5 IN_PROGRESS, B3 end-to-end incomplete; WIP with compilation, tests and mutations not run**.
2 verification rounds used, the last 1 unused, and the existing 1254/1/7 is a pre-fix result. actor 14 inputs/6 results/cap1 and cap8 were
not changed. No actor GREEN or performance improvement is claimed from local API or direct response preservation.

The first next action is **to close the batching contract's required connection table with actual owned consumption**. Do not repeat the step of only adding
new auxiliary APIs. Trace `EventSender/Receiver` and product store creation, EventNode/NodeAdapter/WorkerInput,
long-lived request/release provenance, and control/connection terminal consumption as the same change scope. Separate source claims from
independent dedupe cost, and complete notification outside the lock, advance reservation of required results/returns and the input/capacity/shutdown pump together
before judging progress on the fixed actor counterexample. The variable native result bound and remote acceptance are separately incomplete, and
are not hidden with a raw fallback or an arbitrary row multiple. Do not spend the last round on partial compilation/tests.

This source, regressions and owned documents are preserved as a full WIP checkpoint, and generated outputs are ignored. Remote/GPU/C++/push were
not run. The final goals of strong waves, normal responses and RAM offloading promotion after VRAM-only are unchanged.

### 2026-09-07 follow-up implementation — immutable sharing of actual request input

The earlier direct response WIP is `f5aa09675`. While statically tracing the canonical owned connection, paths were found where the handler and
candidate/batch error reporting copy the raw input again. This change is **limited to removing this repeated copying**.
The actual handler borrows the original Event, and RequestState candidates and the drive's observation/error owners share immutable input.
The output authority of the original request and the preservation of progress state on rejection are unchanged. The contract is owned by
[the input sharing section of the batching contract](adapter-batching-layers.md#immutable-accepted-input-and-mutable-progress-candidates).

1 copy of the original Event in PREFILL admission remains. This is not claim transfer, a parsing cost budget or a full memory cap
implementation, and the **whole owned success connection** that the earlier record called for **is again incomplete**. This mismatch is not hidden.
No new ResourceBudget numbers/defaults were created from queue counts. The two count fields in the current product declaration alone cannot
prove a byte limit, and the boundary between known responses and variable native results continues to be kept distinct.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end incomplete; WIP with compilation, tests and mutations not run**. 3 oracles for actual prepare/accept
rejection, the normal path and shared input lifetime were written. The values, assertions and input counts of the existing fixtures were kept.
The last run, 1254/1/7, is a pre-fix result; 2 verification rounds used, the last 1 unused. The actor 14 inputs/6 results
cap1/cap8/external dequeue0 verdict is also unchanged. Seeing no blocking point in static review is different from passing a run.

**The scope of the first next action is not widened further.** Actually port the canonical
EventSender/Receiver→EventNode→NodeAdapter/WorkerInput→long-lived owner already listed in the batching contract's required connection table. First
settle the product configuration's declarations of delivery slots/retention count/byte/independent receipt limits and the policy for undeclared ones, without
arbitrarily adjusting numbers. Keep advance reservation of required effects and the input/capacity/shutdown pump as promotion conditions of the same candidate.
Do not declare completion with a partial success path that hides unresolved bounds or remote acceptance.

At this step, do not attach further preceding refactors/knobs. Use the last verification only when sealing a completed candidate that passes the existing counterexamples and per-cause
removal mutations. Only source, required regressions and owned documents are preserved as a full WIP checkpoint,
and generated outputs are ignored. No remote/GPU/model/C++/push runs and no final outcome promotion.
