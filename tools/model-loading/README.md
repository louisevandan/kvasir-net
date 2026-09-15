# P4 model loading

P4 내부의 OUTER 모듈이다. 별도 저장소·배포 단위가 아니며 P4의 루트 테스트 명령으로 검증한다.
llama.cpp/GGUF 기반 계획 구현과 입력 타입, 참조 판단, 회귀 시험 및 로컬 검증 산출물의 소유 경로는 이 폴더다.
Python HF 계획기는 [HF 모델별 모듈](../../layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md#automatic-loading-planner)이
구현·시험·실측·검증 자료를 함께 소유한다. HF 모델 의미를 이 TS 계획기나 P4 공통 core에 넣지 않는다.
실행 순서와 실기 수용은 [로드맵](../../docs/distributed-batching-roadmap.md)과
[검증 규약](../../docs/distributed-batching-verification.md)을 따른다.

| Purpose | Path |
| --- | --- |
| Public function and types | [index.ts](index.ts) |
| Planner, memory policy, partition solver, inventory and GGUF reader | [src/](src/) |
| Inventory collection, coarse audit and placement CLI | [cli/](cli/) |
| Regression tests and actual evaluation CLI test | [tests/](tests/) |
| Independent reference, literal judgments, scenarios and comparison CLI | [validation/](validation/) |
| Local inputs, frozen datasets, mutations and logs (ignored) | `target/` |

P4 TypeScript consumers import `planModelLoading` and its types from
`tools/model-loading/index.ts`. The implementation never imports its tests or reference solver.
The existing model-catalog benchmark reuses this module's GGUF reader.
`npm run test:model-loading` runs both llama.cpp TypeScript and HF Python tests; `test:placement-policy` remains an alias.
Use `test:model-loading:llamacpp` or `test:model-loading:hf` for a scoped suite.

`cli/collect-inventory.ts` accepts `--agents <agents.json>` and `--probe <event-probe.py>`;
its default output is this module's `target/fleet-inventory/`. The probe is the P4 runtime's
external INSPECT client. No fleet discovery or native deployment is implicit in a policy evaluation.

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

`validateNativeDeployment` is the fail-closed bridge from an approved placement to adapter-native
PLAN and post-LOAD allocation evidence. Every stage binds its native device/host entries to one named
physical pool. Host-shared entries are added once in that pool, all cuts must be contiguous and
adapter-approved, and every stage must have the same upstream commit and patch set. PLAN-only input may
authorize LOAD but returns `actualAllocationConformant: false`; runtime acceptance requires matching
`MEMORY_ACTUAL` evidence for every stage. The CLI uses the same public function:

```powershell
node tools/model-loading/cli/validate-native-deployment.ts INPUT.json RESULT.json
```

The evaluation writes reference data before importing a policy. Its independent Pareto search consumes
the original typed input and calls none of the production normalisation/admission/partition functions.
Ten literal analyst judgments anchor that solver; the large matrix is algorithmically expanded from
the analyst's explicit objective, not thousands of separate LLM judgments or measured speed predictions.
The scorer reconstructs selected memory/time/ordering from input, detects false admission/refusal,
compares all four objectives and allows equivalent cuts. Missing profiles and unexpected policy errors
remain separate failures. Immutable study/reference hashes prevent replacing expectations after scoring.

```powershell
node tools/model-loading/validation/evaluate-loading.ts prepare --model-root S:\models --out tools/model-loading/target/loading-study/new-run
node tools/model-loading/validation/evaluate-loading.ts reference --out tools/model-loading/target/loading-study/new-run
node tools/model-loading/validation/evaluate-loading.ts compare --out tools/model-loading/target/loading-study/new-run --label candidate
# Optional baseline: use an independent checkout with the previous policy.
node tools/model-loading/validation/evaluate-loading.ts compare --out tools/model-loading/target/loading-study/new-run --label baseline --policy-root F:\dev\p4-baseline --policy-dir tools/cluster-inference
npm run test:model-loading
node tools/model-loading/validation/verify-mutations.mjs
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

Local evidence: `F:/dev/p4/tools/model-loading/target/loading-evaluation-20260913/` (study, references, both policy datasets,
comparison summary, tests and mutations). Study SHA256
`985e77217841ed1ede6a2be5d9d2f65a455961a9d9855bb8c1ad5392a90f1601`; reference SHA256
`3f78a2f089264f2eaae0cfd2d4311a1b8e1f319869a688de8a24f56a576ad0b8`.
This is a local policy-validation checkpoint. No fleet deployment, native PLAN/LOAD, real throughput
acceptance or remote publication was performed. The remaining runtime step is stated above.

```powershell
node tools/model-loading/cli/audit-model-loading.ts --model-root S:\models --fleet target/fleet-inventory-current/final-v2.json --out tools/model-loading/target/model-loading-audit.json --exclude-devices central:gpu:1 --without-machines mi250-a,mi250-b --reserve-gib gddr=2,mac_unified=10,gb10_unified=20,ddr_offload=16
```

Historical bundles retain their original source paths, hashes and commit identity after relocation.
They are evidence for the recorded checkpoint, not a fresh execution of the relocated source.
Always prepare a fresh study for new source hashes; do not rewrite a frozen seal.

2026-09-13 relocation validation: the planner, tier policy, partition solver and GGUF parser
retain identical code after line-ending normalization. A fresh scan of all 42 models and the
53×3 matrix again agrees on 6,678/6,678 cases; literal judgments agree on 10/10. All 6,688
reference inputs/verdicts/plans equal the historical run, excluding execution time. Module tests
pass 34/34, composer tests 4/4, all five mutations are detected and docs-lint checks 98 tracked
pages. This relocation does not rerun the Rust workspace or approve native loading.
Evidence is in `target/layout-validation-20260913/`; historical file movement is hash-checked in
`target/evidence-relocation.json`. Next runtime acceptance still requires adapter PLAN/calibration
and the roadmap's multiple-machine execution gates.
