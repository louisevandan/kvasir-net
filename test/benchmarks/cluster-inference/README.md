Compose distributed inference experiments from model, placement, policy, workload and immutable runtime evidence on main.

| Purpose | File |
| --- | --- |
| Current work and promotion gates | [Roadmap](../../../docs/distributed-batching-roadmap.md#v11-plan) |
| Runtime acceptance | [Verification](../../../docs/distributed-batching-verification.md#v11-gates) |
| Composer and CLI | [compose.mjs](compose.mjs) |
| Fleet/model loading planner | [model-loading-planner.ts](../../../tools/cluster-inference/model-loading-planner.ts), [model-loading-policy.ts](../../../tools/cluster-inference/model-loading-policy.ts), [audit-model-loading.ts](../../../tools/cluster-inference/audit-model-loading.ts) |
| Loading reference/policy evaluation | [evaluate-loading.ts](evaluate-loading.ts), [analyst judgments](loading-judgments.ts), [independent reference](loading-reference.ts), [scenario matrix](loading-scenarios.ts) |
| Model templates | [Hy3](models/hy3-no-think.json), [Step3.7](models/step37-no-think.json) |
| Experimental policies | [decode2/open8](policies/decode2-open8.json), [decode4/min4/CPU4](policies/decode4-min4-cpu4.json), [pipeline/open8](policies/pipeline-open8.json) |
| Long Korean workloads | [8 requests](workloads/long8.json), [16 requests](workloads/long16.json) |

Long-lived source and release work use `main`. Model names select data, not Git branches.
Temporary comparison source commits stay reachable through integration history and evidence bundles.

An experiment JSON names `model`, `policy`, `workload`, `cluster`, `runtime`, optional `tokenCounts`,
plus a fresh `runId` and integer `generation`. File references are relative to that JSON.
`cluster` is a saved event-drive config: nodes, cuts, backend selection, KV placement, RAM offload,
resident/context and native endpoints. Keep machine addresses, credentials and local paths in ignored
`target/cluster-inference/` experiment inputs. `runtime` contains full `sources` commit IDs and
`binding_file_sha256` for the actual agent/driver/native/library manifest; one source is not inferred
from a model name. Mixed historical binaries remain explicit until a common main build is deployed.

```text
test/benchmarks/cluster-inference/
  compose.mjs            common OUTER config generation
  models/                saved model-specific chat templates
  policies/              scheduler environment and optional native CPU budget
  workloads/             meaningful messages, output contract and arrival waves
target/cluster-inference/
  clusters/              local topology and saved memory/placement plans
  runtimes/              platform binaries and source/library/model hashes
  experiments/           combinations of the above inputs
  runs/<fresh-id>/        config, manifest, artifact, raw telemetry and responses
```

```powershell
node test/benchmarks/cluster-inference/compose.mjs target/cluster-inference/experiments/arm.json target/cluster-inference/runs/fresh-id
node --test test/benchmarks/cluster-inference/compose.test.mjs
```

The composer refuses existing output directories, mismatched waves, missing runtime identity,
duplicate native thread settings and measured prompt-plus-output context overflow. Token counts
must bind each exact prompt SHA256; absent counts produce `tokenizer_verified=false`, never an
actual-input claim. Template profiles preserve the measured restricted system/user/no-think forms;
other models or tool conversations need their own GGUF-template check.

This module prepares inputs. Agent startup/cleanup, host availability, actual binary hash verification,
absolute wall-time enforcement and execution remain the cluster lifecycle runner's responsibility.
Apply `manifest.agent_environment` when starting agents; it is not a per-request native option.
The driver accepts `config.json` and writes `artifact.json`. Preserve first errors and partial artifacts,
including separate cleanup failures. An inference timeout alone is not a whole-run deadline.

Use `tokenCounts` entries `{ "prompt_sha256": "...", "prompt_tokens": 123 }` only after exact
tokenization. Per-request EOS, length, full prose, arithmetic and UTF-8 are separate acceptance facts.
The built-in workloads require at least 1,024 generated tokens, allow 4,096, and require EOS plus
record IDs and a Korean conclusion marker. Substring checks do not certify numerical correctness.
No profile increases fragment/resident windows or approves B2/B3, GPU saturation or H5 performance.

The coarse loading audit groups split GGUF files, excludes `mmproj`, converts persisted agent inventory into
GDDR, Apple unified, GB10 unified and host DDR pools, and compares its tier decision with an independent
cumulative-capacity judgment. Even with explicit KV/runtime bytes this aggregate audit does not prove
legal layer cuts, repeated stage costs or native loading. Zero values only audit weight-tier boundaries.
Operator-disabled devices remain in evidence but are not
admitted.

`planModelLoading` is the typed black-box entry point. It accepts arbitrary CPU, RAM and accelerator
specifications, an exact layer model profile, target context/concurrency and per-device calibration.
It returns evidence for hardware normalization, layer demand, memory-tier admission and contiguous-cut
optimization. Missing calibration is an error; a device name is never substituted for measured service.
Placement uses the smaller of total and available memory, minus the reserve. Unified GPU/RAM shares
one capacity and the larger reserve; multiple unified devices require a future explicit shared topology.
`model.legalCuts` contains allowed interior boundaries (`[]` means unsplit only); an absent value assumes
all boundaries and is not native topology approval. An oversized optional device is skipped.
The exact ordered-device objective is minimum tier prefix, maximum stage time, total stage time, then
stage count. Two passes preserve the secondary optimum when a later stage dominates the maximum.
`admission` remains a coarse, nominal-capacity diagnostic; `placement` is the plan for current capacity.
DDR pools represent whole-layer CPU stages. GPU expert offload and shared-device execution contention
need richer profiles. Hardware link fields are validated metadata; hop cost comes from calibration.

The evaluation writes reference data before importing a policy. Its independent Pareto search consumes
the original typed input and calls none of the production normalisation/admission/partition functions.
Ten literal analyst judgments anchor that solver; the large matrix is algorithmically expanded from
the analyst's explicit objective, not thousands of separate LLM judgments or measured speed predictions.
The scorer reconstructs selected memory/time/ordering from input, detects false admission/refusal,
compares all four objectives and allows equivalent cuts. Missing profiles and unexpected policy errors
remain separate failures. Immutable study/reference hashes prevent replacing expectations after scoring.

```powershell
node test/benchmarks/cluster-inference/evaluate-loading.ts prepare --model-root S:\models --out target/loading-study/new-run
node test/benchmarks/cluster-inference/evaluate-loading.ts reference --out target/loading-study/new-run
node test/benchmarks/cluster-inference/evaluate-loading.ts compare --out target/loading-study/new-run --label candidate
# Optional baseline: use an independent checkout with the previous policy.
node test/benchmarks/cluster-inference/evaluate-loading.ts compare --out target/loading-study/new-run --label baseline --policy-root F:\dev\p4-baseline
node --test test/benchmarks/cluster-inference/*.test.ts test/benchmarks/cluster-inference/*.test.mjs
```

`study.json` preserves all model file paths/stat identities/header hashes, per-layer stored extents,
hardware definitions, workload assumptions and literal judgments. `reference-data.jsonl` holds every
reference plan/refusal; `<label>-policy-data.jsonl` holds policy plans, errors, comparison and timing;
`<label>-summary.json` groups scores by model/scenario/workload. Existing output files are never replaced.
The 53 configurations cover 1/2/3/4/8/9 available hosts, dedicated 12/24/64-GiB devices, dual/multi-GPU
hosts, 64-GiB Mac/128-GiB GB10 unified classes, CPU DDR, pressure and disabled devices. All service
times are hypothetical. Workloads are stored-weight geometry, assumed 4k×1 and assumed 32k×8
(512 KV bytes/token/sequence/layer and 4 MiB runtime/layer in the latter two).
GGUF headers/padding and all non-block tensors repeat per stage; all stored blocks including auxiliary
blocks are represented. Metadata-only and metadata-first split shards are supported. This conservative
storage model is not native resident bytes, model capability/alias-cut validation or a payload digest.
Embedding models remain storage cases; `mmproj` is excluded. None of these scores approves LOAD,
normal responses, actual TPS or hardware performance. The next runtime step is adapter-proven model
topology and workload/backend-specific PLAN plus calibration before any deployment decision.

2026-09-13 evaluation checkpoint: implementation `532deee47`, baseline `484b856ee`. All 42 catalog
models yielded storage geometry; 53 hardware scenarios × 3 assumed workloads produced 6,678 cases.
The ten literal judgments are separate from that denominator.

| Matrix verdict | Baseline | Candidate |
| --- | ---: | ---: |
| Full feasibility/objective agreement | 5,915 / 6,678 (88.57%) | 6,678 / 6,678 (100%) |
| Optimal among 4,872 reference-feasible cases | 4,321 | 4,872 |
| Correct infeasibility | 1,594 | 1,806 |
| False admission | 179 | 0 |
| Invalid selected plan | 407 | 0 |
| Worse total service at the same bottleneck | 111 | 0 |
| Policy exception instead of a plan/capacity verdict | 66 | 0 |

Literal judgments improved 5/10 to 10/10. Baseline failures reproduce unavailable VRAM, shared RAM
pressure, forbidden cuts and the later-bottleneck counterexample. In the stored Nemotron 550B
`mixed-4-nominal` case both policies predict a 1,724-ms maximum, but total assumed service improves
3,402→3,399 ms. Different optimal cuts are accepted. These are modelled numbers, not inference measurements.

Validation: Node 38/38 (34 TypeScript tests plus 4 composer tests), including 240 seeded oracle cases,
real GGUF/shard parsing, the actual prepare→reference→compare CLI, input preservation and seal tampering.
Five independent source-copy mutations (availability, unified host availability, cut forwarding,
secondary objective, disabled device) each produce assertion failures after syntax validation in a
fresh Node process; source/runtime/log hashes are retained. No build cache supplies mutated execution.
`cargo test --workspace --no-fail-fast --locked` on unchanged Rust source completed with exit 0,
1,449 passed / 0 failed / 7 ignored, 58 summaries. The previously reported phase-pacing failure did
not recur in this run; this TypeScript change does not claim to repair it. `npm run docs-lint`: 97 clean.

Local evidence: `F:/dev/p4/target/loading-evaluation-20260913/` (study, references, both policy datasets,
comparison summary, tests and mutations). Study SHA256
`985e77217841ed1ede6a2be5d9d2f65a455961a9d9855bb8c1ad5392a90f1601`; reference SHA256
`3f78a2f089264f2eaae0cfd2d4311a1b8e1f319869a688de8a24f56a576ad0b8`.
This is a local policy-validation checkpoint. No fleet deployment, native PLAN/LOAD, real throughput
acceptance or remote publication was performed. The remaining runtime step is stated above.

```powershell
node tools/cluster-inference/audit-model-loading.ts --model-root S:\models --fleet target/fleet-inventory-current/final-v2.json --out target/model-loading-audit.json --exclude-devices central:gpu:1 --without-machines mi250-a,mi250-b --reserve-gib gddr=2,mac_unified=10,gb10_unified=20,ddr_offload=16
```
The pipeline profile derives independent phase cohorts from admitted ready/inflight populations
and the fixed flight window; simultaneous returns do not merge all generation work.
Final prefill returns preserve initial cohort width without activating the mixed-work limit.
it requires a finite open window, fragment limit one and positive mixed prefill quantum.
Its 128-row mixed quantum is an experimental work limit, not a measured latency guarantee.
Optional `P4_STAGED_MIXED_BATCH_ROWS` bounds decode plus prefill rows while decode is active.
It requires pipeline mode and a positive integer. Select its value from a separately recorded
profile before sealing measurements; decode consumes the budget first, then prefill uses the
remainder. It cannot be combined with the experimental online service-time controller.
