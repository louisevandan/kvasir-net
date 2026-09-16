# llama.cpp adapter restructure plan

> Document status (2026-09-06): **historical / superseded plan**. It preserves the plans and observations of the time. Do not use it for current status, execution order, or promotion criteria.
> Current goals, status, and order follow the [execution roadmap](distributed-batching-roadmap.md); document authority and reading paths follow the [document map](document-map.md).

**2026-09-06 status change:** This document is the historical backlog of U/P defects, contracts, and experiments.
The [distributed batching roadmap](distributed-batching-roadmap.md) is the sole owner of current status and implementation order,
and new ownership of the old phases is cross-checked there as well. "Current/done/not started" below is as of each entry's recorded date.
The required persistence and compatibility safety contracts are not discarded, but not every persistence feature is a serial prerequisite for the current batching-correctness implementation.

Revised 2026-08-31 (first edition 2026-08-30, with same-day review feedback applied). This is the full plan for the complete restructure of the llama.cpp
adapter — batch layering, KV persistence, session ledger, admission/eviction
policy, transport efficiency, and model diversity gates. The supporting measurements came from
the 2026-08-30 4-node gemma-4-E2B run.

Two documents own the design details:

- Batch layer contract: [adapter-batching-layers.md](adapter-batching-layers.md)
- Persistent storage convention: [kv-state-store-convention.md](kv-state-store-convention.md)

## Principles

1. **No change to the P4 core; adapter + OUTER changes are allowed.** The protocol, agents, and services
   know nothing about batching, KV, or model concepts. Code changes happen in `layers/adapters/llamacpp/` and
   the OUTER implementations (drive, planner, harness) — new wire fields such as `session_key`
   grow inside the adapter-owned content-type and are filled in by OUTER.
   No llama-specific knowledge is added to the P4 core, **but backend-neutral 2PC correctness
   fixes are allowed** — the current coordinator (`layers/service/src/cache.rs::recover` @ df5b9ce7;
   the test `recovered_partial_restore_is_failed_closed` pins this current
   behavior) fails a partial Restore immediately after restart and folds a
   committed receipt seen during Aborting into failure, so as-is it cannot pass P2's all-resident/all-persisted
   convergence. The storage convention document owns the convergence direction table.
2. **Tolerance of llama.cpp layer separation.** Policy code (strategy, ledger, admission) does not link against
   llama.cpp and sees only HELLO-negotiated values, GGUF-derived values, and calibration constants —
   "only the values change after a pull" holds for this layer alone. Updating the native compat layer
   is a semantic rebase on every pin (d7a207411→d7bd3bfc dry-run: 5/24
   conflicts — reproduced independently in round 6), and that cost is owned by U0's per-pin compatibility gate and the
   3-way split of the patch queue.
3. **Stay fail-closed.** Unaudited memory families (msa/dsa/dsv4/hybrid_iswa) are
   refused at load. Model expansion requires passing a 3-axis audit (stage retention ∧ unified sequence separation
   ∧ backend conformance), and persistent-identity relaxations are lowered only for items that pass the P2 verification
   matrix.
4. **Mechanism before policy, measurement before optimization.** And **do not turn on automatic
   policy above a failure path before that path is verified** — no automatic TTL eviction in P3 before the P2 fault gate
   passes.

## Current status — past observations (reproducibility caveat)

The following are local observations from 2026-08-30. The checkout at the time of the 5th review could not
reproduce them — compat 0022~0024 and the manifest were untracked, and the upstream
checkout had drifted off the pin (d7a207411) to d7bd3bfc (dirty tree), so the official prepare
failed with `no compatibility manifest`. This round put the patch queue and manifest under
tracking; restoring the upstream pin and pristine verification are U0 acceptance criteria.

- 4 nodes (3090×2 + 4080×2) ran gemma-4-E2B to completion, 40/40, over the event-v2 path.
  iSWA stage-retention opt-in + passthrough tensor identity fix (compat 0022~0024).
- Measured: parallel 20 → 121.43 tok/s, parallel 40 → 189.26 tok/s,
  TTFT p50 1.4 s under concurrent admission.
- The Persist/Restore file format, atomic publish, restore-position verification, the 2PC
  coordinator (`service/cache.rs`), and on-disk receipts (TransactionStore) exist.

## Measured: layer tolerance to upstream movement (2026-08-31)

To verify Principle 2 ("only the policy layer is value-independent; native compat is rebased on every pin"),
we pulled the latest llama.cpp master and replayed the patch queue. `bump-pipeline-upstream.mjs`
creates a clean worktree and applies the queue there, so the measurement does not touch the working tree.

| Item | Value |
| --- | --- |
| pin | d7a207411 (2026-08-27) |
| target | 557614e02 (2026-08-31) |
| upstream commits in between | **69** |
| conflicts | **6 of 24** (0009, 0010, 0013, 0016, 0017, 0018) |
| P4 policy layer (Rust) changes | **0** |

Broken down by layer, the result matches exactly what the contract predicted.

| Class | Patches | Conflicts | Interpretation |
| --- | --- | --- | --- |
| `upstream_fix` (ggml layer) | 2 | **0** | Over 69 commits, the ggml abstraction layer moved independently of these patches |
| `model_feature` | 4 | 1 (0018 mtp-tail) | Model feature ports are mostly independent |
| `stage_hook` (intrudes into llama core) | 18 | 5 | Update cost concentrates here |
| P4 policy (strategy, ledger, scheduler, session_key) | — | 0 | Structurally 0, because it does not link against llama.cpp |

**The cost is not linear.** The pin from one day earlier (d7bd3bfc, 1 commit apart) already had 5
conflicts, and pulling 69 more commits gives 6 — 68 commits added only 1 new conflict.
Update cost is determined not by the volume of upstream change but by **whether the files we intrude into were touched**.
This is the measured basis for U0 ④ (3-way patch queue split) and ③ (stage ABI isolation):
if `upstream_fix` is kept as a separate bundle, it does not conflict along with the whole port when upstream
absorbs it, and the narrower the intrusion surface of `stage_hook`, the smaller the number 5
becomes.

Limitation: this measurement looks only at **applicability**. Semantic equivalence after application (state format,
numerical equivalence) is judged separately by U0 ①'s clean-pin prepare and the per-pin state compatibility gate.

## Measured: tracking cost (2026-09-01, 18-commit drift)

llama.cpp does not ship once a day; it ships **17.5 commits a day on average** (367 commits in the last 21 days,
range 2~26). Tags also come several a day (b10718~b10731 over two days). And the 27
files we patch **moved on 20 of those 21 days**, with cumulative change over the period of +2438/-506 across 20 files.
So "can we keep up" is not a performance question for this project; it is a survival question.

The actual cost of moving 557614e02 → 0eadefebd (18 commits) was:

| Stage | Result |
| --- | --- |
| Possible conflict points | 3 of our files changed upstream — `git diff --numstat 557614e02..0eadefebd`: `common/speculative.cpp` +6/-46, `src/llama-context.cpp` +19/-6, `src/llama-kv-cache.cpp` +26/-38 |
| Queue rebase (24 patches) | **24/24 clean** — 0 escalations, 0 manual interventions |
| pristine replay verification | 24/24 clean |
| Patch classification gate | valid — hook 18 / fix 2 / model 4 (no classification change) |
| Official prepare | passed — compatibility_id `0eadefebd3.3cfc636181e4…` |
| CPU build + CTest | 11/11 (4 real-model subtests SKIP) |
| CUDA build + CTest | 11/11, sm_86/sm_89 |
| 4-node real-hardware run (3090x2) | smoke 1/1 · mixed 40/40 · service 40/40, all passed semantic judgement |
| C++/policy source changes needed for the compat port | **0** — the Rust and harness changes in the same commit are evidence-channel fixes unrelated to the port |

The patch queue size went 13 (08-03) → 21 (08-05) → 24 (08-27) → 24 (08-31) → 24 (09-01),
**holding at 24 for the last three consecutive pins**. That the queue does not accumulate is the practical
evidence of layer separation — even though upstream touches the neighborhood of our patches almost every day.

**It is, however, a single observation.** 18 commits is a short drift, and the earlier 69-commit drift needed fuzz 3 and
3-way 2. We do not claim that conflict-free replay is a general property.
We record the reproduction commands. The commit count is `git log --since="21 days ago" --format=%H origin/master`
(21×24h rolling window, as of 2026-09-01 08:00 UTC), the change volume is
`git diff --shortstat <base>..origin/master -- <the 27 patched files>` against the base commit from 21 days earlier, and daily aggregation uses the KST
calendar. Without stating the base and end SHAs and the time zone, the same figures do not reproduce.


## Measured: adopting the new pin (2026-08-31, U0 ①)

Following the tolerance measurement above, we actually moved the pin. The result is the strongest
evidence for the layer separation claim.

| Stage | Result |
| --- | --- |
| Queue rebase (24 patches) | clean 19, fuzz 3, 3-way 2 — manual intervention on 5 hunks |
| pristine replay verification | **24/24 clean** (`rebase-pipeline-upstream.mjs` phase 2) |
| Official prepare | **passed** — compatibility_id `557614e029.00e66c6b…` |
| CUDA build | **succeeded**. All 11 native test executables exited with code 0, but **4 real-model subtests were SKIP** (`P4_STAGED_LLAMA_MODEL` and `P4_STAGED_MTP_MODEL` unset) — only the model-free suite was proven |
| 4-node acceptance | **passed** — a meaningful Korean answer to the same prompt, structure 1/1, semantics 1/1 |
| P4 policy layer (Rust) changes | **0** |

The rebase found one real defect. Upstream added memory creation sites and two families
(`llama_kv_cache_dsa_iswa`, `llama_memory_hybrid_idx`), and
these were bypassing the retention gate. The bypass silently reopens partial stages for unaudited
families, which breaks fail-closed. We changed the gate to apply
to all `llama_kv_cache*`/`llama_memory*` creation by pattern match instead of a list of family names, and
added **0 bypasses** as an assertion in replay verification — that
assertion is what exposed this hole. Both new families landed as default-deny, as intended.

We also record a build-architecture trap. The script default was `75;89`, so this batch's
sm_86 (3090) had no native code, and a local run with that sm_75 build went
19.9 → 6.15 tok/s. **This figure is an observation of that past build, not an
explanation of current runs** — the currently deployed `ggml-cuda.dll` contains both sm_86 and sm_89 cubins
(direct inspection: sm_86 15, sm_89 22, sm_75 0), and the remote hash is identical.
Because the acceptance verdict was about **the meaning of the answer**, not throughput, this trap surfaced
even while the test passed — this is where keeping the two verdicts separate paid off.

The remote smoke's 6.4 tok/s is not a performance baseline: 1 request, mostly 1-row decode,
`mixed_batches=0`, so it does not represent batching or pipeline throughput. The baseline will be
re-measured with the same commit, build, and model after the evidence system (E0/R0) is closed.

## Historical defect register — not a current status table

| # | Defect | Evidence |
| --- | --- | --- |
| D1 | Pipeline depth 1: a single `in_flight: bool`; downstream runs all physical UBATCHes of a CapsuleSet sequentially, then returns them all at once | GPU 18~25%, step time independent of row count, 0~2 mixed batches |
| D2 | No `--kv-root` in the plan → `kv=0` | HELLO log |
| D3 | No state storage organization: flat `<root>/<key>.lkv`, unconditional overwrite, no timestamp | state_store.cpp |
| D4 | Position discontinuity on slot reuse with 40 requests. **Root cause unknown** — the order local seq_rm → settle all stages → return slot is already implemented (release.rs), so an observation ledger alone will not fix it | Stress C (evidence not preserved; must be re-acquired) |
| D5 | No cell accounting: admission counts only slots | scheduler call site |
| D6 | Over-pinned identity: requires an exact match of n_batch/n_ubatch/n_seq_max and the build | manifest comparison code |
| D7 | Fixed cost of 81 individual tensor transfers per hop | cut-set 31/27/23 |
| D8 | Compute buffer over-allocation: 1.4 GB at ubatch 512, 3.5% actually used | 512↔128 measured 3.66× |
| D9 | SWA V over-allocation (256→512 with v_trans) | KV buffer log |
| D10 | Persistence cap of 128 MB/node | state_store size check |
| D11 | No prefix reuse | adapter audit |
| D12 | No dynamic occupancy telemetry. `MEMORY_ACTUAL` reports the full allocation at load, not runtime cell occupancy | llama_stage_runtime.cpp |
| D13 | Receipt collision: flat `<kv_root>/.p4-transactions/<op>.receipt` layout with no stage scope in the path → on a shared volume, 4 stages conflict on the same operation_id | transaction_store.cpp:185 |
| D14 | 2PC commit hole: native Commit runs seq_rm immediately after publish, so if a failure occurs during the commit wave, the coordinator's Abort cannot revive stages that are already committed. The convergence rule for partially committed state is unverified | kv runtime + cache.rs |
| D15 | No persistent SessionKey or LCP evidence: `InferenceCommand` carries only session_id/request_id, and stored meta has no token history or prefix digest → no basis for judging "the same conversation" after restart | `v2/commands.rs::InferenceCommand` @ df5b9ce7 |
| D16 | No per-session serialization: locks are per operation_id, so Persist/Restore/Discard/GC on the same session can run concurrently | `transaction_store.cpp::TransactionStore::Lease` @ df5b9ce7 |
| D17 | Non-atomic record bundle: partial combinations of state/tokens/meta are possible, and the binding between LCP evidence and KV position cannot be proven | convention document defect table |
| D18 | `Committing` undefined: commit is Committing durable record → side effect → Committed, but adapter Reconcile collapses Committing into Inconsistent; Prepare writes only a receipt, with no staged copy | `protocol.hpp::KvReceiptState` definition, `server.cpp::Session::handle` KvCommit branch, `transaction_store.cpp::TransactionStore::prepare` @ df5b9ce7 |
| D19 | Fragile native compat updates: 24 patches and 27 upstream files; replaying d7a207411→d7bd3bfc gave 5 conflicts (0010, 0013, 0016, 0017, 0018 — reproduced independently with a clean-worktree replay via `bump-pipeline-upstream.mjs --from d7a207411`). The queue does not separate stage hook/upstream fix/model feature, and it patches down into `ggml-backend.cpp` and RPC, intruding below the llama layer | compat/d7a207411 queue |
| D20 | Stage server coupled to llama private headers: CMake exposes upstream `src/` as a PRIVATE include, and `stage_memory_plan.cpp` directly includes `llama-cpp.h` and `llama-ext.h` → breaks on every internal refactor regardless of the public ABI | `server/CMakeLists.txt` (P4_STAGED_LLAMA_SOURCE_DIR/src), `runtime/stage_memory_plan.cpp` top-of-file includes @ 87ec1317 |
| D21 | HELLO cannot identify the executable: only the upstream commit is exposed — builds with the same commit and a different patch-set look identical, and stage_abi, the active backend/device, trim_support, and state_abi are absent | `server/server_hello.cpp` (capabilities string) @ 87ec1317 |
| D22 | Compat manifest depends on checkout EOL: 0001~0021 were recorded as CRLF byte hashes and 0022~0024 as LF hashes, so a clean checkout under any autocrlf setting fails verification; fixture self-test 14/15 | Fixed in round 6 — `.gitattributes` (`*.patch text eol=lf`) + full LF rehash, validate passes, self-test 6/6. Re-verification of patch_set_sha256 and patched_tree is done by U0 ①'s clean-pin prepare. The 7th review externally verified that the current aggregate values match by applying the queue directly to a clean pin |

## Target structure (summary)

```
L0 queue → L1 ledger → L2 admission/occupancy → L3 composition strategy → L4 proof → L5 transport
```

Record identity = (model×cut×format)×(session_key×position), not the load ID.
Extending pipeline depth requires the **fragment credit
contract** before `in_flight: bool` is unlocked — `(generation, edge, sequence, stream_epoch, fragment)` identity,
per-edge row and byte credit, idempotent return on ACK/duplicate/timeout, ordering and cancellation
rules, and queue and RSS caps. The P4.5 row carries all of the normative content; [plan.md](plan.md) §3 remains only as a historical
rationale link.

## Old phase definitions — see the new roadmap for current order and promotion criteria

| Phase | Content | Resolves | Acceptance criteria |
| --- | --- | --- | --- |
| U0 | llama compatibility boundary: ① restore a clean upstream pin and verify pristine (official prepare passes) ② bind stage_abi, `build_id` (including patch_set_sha256), the active backend/device, `trim_support`, `state_abi_id`, and backend layout into HELLO ③ remove the server's dependency on llama private headers (public `llama.h` + a P4-owned versioned stage ABI only; internal access moves inside the compat implementation) ④ 3-way split of the patch queue (stage hook / upstream fix / model feature) ⑤ register the 3-axis backend conformance audit ⑥ the backend gate is based on the release manifest's `required_backend_set` declaration — the set always includes the CPU baseline, plus the production backends the deployment claims (current declaration = {CPU baseline, CUDA production}). Promotion requires every declared backend to pass, together with plugin/version, device capability, and actual buffer placement evidence ⑦ cross-backend Persist/Restore stays fail-closed until the matrix passes | D19, D20, D21, D22 | ① official prepare passes on the pin — including clean-checkout fixtures under both autocrlf settings, patch_set_sha256 and patched_tree re-verified and re-recorded, and the dirty port attempt preserved as a branch rather than discarded ② a test that distinguishes two builds with the same upstream and different patch-sets via HELLO + negotiated values (trim_support, state_abi_id, backend layout) match measured capability ③ 0 private headers, enforced by a server include build gate ④ every patch classified as `stage_hook\|upstream_fix\|model_feature` — 0 unclassified, an independent application test per bundle, automatic detection and removal of fixes absorbed upstream, a specification of the allowed file and symbol scope for stage hooks ⑤ the 3 audit axes and the numerical equivalence criteria documented ⑥ the `required_backend_set` declaration and verification procedure defined — of the current declaration {CPU baseline, CUDA production}, the CPU axis passes on the current pin; passing the CUDA axis is a production promotion condition, not a U0 completion condition ⑦ cross-backend restore refusal confirmed by a negative test |
| P-1 | Fixed groundwork: finalize the `base_model_id × kv_variant_id` contract (the convention is the sole owner of the definition; implementation starts after contract approval), document the 5 contract elements of `session_key` and prefix evidence (normalization, uniqueness scope, version, comparison rule, negative tests), the `session_key` wire field (adapter content-type + OUTER delivery), move the harness into versioned `test/benchmarks/`, evidence convention (commit, compat manifest, full spec, environment, summarizer version, artifact checksum) | Part of D15 | The harness can be re-run from the repository; D4 failure evidence re-acquired and preserved; base_model_id and kv_variant_id bound into the load/restore paths, and the 1-byte base tamper test **passes on both the cache-absent path and the tampered-sidecar path**; tampering with LoRA scale, mmproj, or control-vector is refused + entry-order-independence test passes; session_key round-trips with the same value OUTER→adapter→storage→post-restart Restore; reusing the same session_key with a different request_id succeeds; aliasing the same request_id with a different session_key is refused |
| P0 | State **and receipt** namespace: `v2/<model_id>/<cut_id>/{sessions,receipts,tmp}`, identity check on overwrite, meta.json (binding saved_at, session_key, and digest) + a separate advisory ACCESS file + CONTROL epoch CAS | D3, D13, D17 | Concurrent Prepare/Commit/Reconcile across 4 cuts with the same `operation_id` succeeds; conflicting saves are refused; in crash tests at each step of gen-N bundle + MANIFEST CAS publish, only complete bundles are ever exposed; atomic session lease acquisition and epoch fencing work |
| P1a | Behavior-free ledger: **observe** mapping, residency, and slot lifetime and detect invariant violations + new per-sequence backend position and used-cell telemetry | D12; for D5, only the measurement prerequisite (resolved in P3) | Per-node KV prediction before load = actual allocation ±1%; ledger and telemetry agree on every event |
| P1b | Fix D4: stable reproduction → root cause → fix of the actual lifetime/native state | D4 | Stress C (40 requests, slot reuse) passes repeatedly |
| P2 | `kv=1` round trip + **fault gate**. Prerequisite implementation: a durable staged bundle in PreparePersist, evidence-based judgement of `Committing` (replacing adapter Reconcile's collapse into Inconsistent), backend-neutral core fixes for the convergence table — the table is owned by [kv-state-store-convention.md](kv-state-store-convention.md). Fault gate: failure before, during, and after each stage's commit; exit after side effects complete but before finalize (= Committing recovery); reconciliation of partially committed + partially prepared state; explicit convergence after coordinator restart; 4 kinds of session contention (Persist↔Restore, Persist↔Discard, Restore↔GC, double Persist). Identity relaxation **matrix**: {n_batch, n_ubatch, n_seq_max, n_ctx_seq, kv_unified} × {K/V format, flash/v_trans} × {rebuild with the same compat, different revision} × {source backend × target backend} — only passing items are downgraded; the rest stay fail-closed | D2, D6, D14, D16, D18 | All fault gates pass (including each Committing point); 0 cross-corruption in session contention tests; restore succeeds after reload; matrix results documented |
| P2.5 | Static n_ubatch calibration: fix the reference values for P5 measurement (automatic optimization is P7) | Part of D8 | Reference workload re-measured and recorded with the calibrated value |
| P3 | L2 admission/occupancy: cell-aware admission + execution of snapshot commands (Persist / new `Checkpoint` / new `SnapshotList` / extended RestoreInto / Fork / Discard — the convention owns the vocabulary and semantics; all trigger policy belongs to OUTER) + Restore on re-request + LCP (including `TrimTo` 2PC) + separation of the three concurrency axes (max_resident/decode_parallelism — the axis contract is owned by [adapter-batching-layers.md](adapter-batching-layers.md)). **Requires the P2 fault gate to pass** | D5, D11 | Predicted worst-case cells are reserved in advance — multi-node uses reservation 2PC (owned by the convention: includes prepared accounting, local monotonic TTL reclaim, idempotent release); wait/reject on shortage; 0 over-admit (including prepared); 0 prefills started before all stages attest TrimTo on a branched prompt; 0 innocent-session failures under a heavy+short mix; improved TTFT distribution |
| P4 | Extract the L3 strategy crate + golden replay of recorded traces | — | Replaying existing traces gives identical results |
| P4.5 | Implement the fragment credit contract: `(generation, edge, sequence, stream_epoch, fragment)` identity, per-edge row and byte credit — `U_edge = min(producer, consumer)` negotiation, `B_edge` agreed per model and cut, load refused on mismatch — idempotent return, ordering and cancellation rules, queue and RSS caps | Prerequisite for D1 | Duplicate/timeout/cancellation fault tests pass; 0 credit leaks; credit exhaustion test passes; measured compliance with the long-prompt boundary memory cap |
| P5 | Pipeline depth >1 (starting with continuous submission of prefill chunks). The snapshot consistency fence narrows from a full pipeline stop to **draining the target sequence** (the settlement evidence is not credit but a per-stage `SequenceQuiesced` attest — O9) | D1 | GPU util rises, mixed batches occur, ITL does not get worse, **boundary memory and queue depth caps are respected**; other sequences keep stepping while one sequence drains |
| P6 | Merge the cut-set into a contiguous buffer, skip retransmission of passthrough tensors | D7 | Measured reduction in fixed per-step cost |
| P7 | Chunked persist (D10), SWA V (D9), automatic n_ubatch optimization, 3-axis audit of DENIED families | D8~D10 | Per-family audit document + successful load |

The old serial order in the table above is no longer an execution directive. Current dependencies and the U/P→new phase mapping are
owned solely by the [distributed batching roadmap](distributed-batching-roadmap.md).
A rise in GPU util alone does not approve P5 completion, a transport observation on one particular host alone does not confirm a P6 bottleneck,
and creating a new crate alone does not approve P4 completion.

## Contract correction round (2026-08-31, 3rd review)

This is the record of how the 7 blocking defects from the 3rd review were handled. "Done" refers only to what can be
verified in this repository; code implementation is the pass condition of the relevant phase, not a completion
claim. **The next review decides whether P-1 may start.**

| Blocking defect | Contract (document) | Implementation (code) |
| --- | --- | --- |
| Committing convergence undefined | Done — added operation×Committing rows and evidence-based judgement to the convention's 2PC table | P2 |
| No per-session serialization | Done — convention "session shard serialization" (lease/epoch fencing/generation CAS) | P0 (lease), P2 (contention tests) |
| Non-atomic record bundle | Done — convention "record bundles and atomicity" (gen-N + MANIFEST CAS, receipt binding) | P0 |
| No LCP trim barrier | Done — convention "LCP Trim barrier" (TrimTo 2PC, attest from all stages) | P3 |
| model_id sidecar bypass | Done — full recomputation on every load by default, substitution allowed only from a verified manifest, negative tests on both paths | P-1 |
| Incomplete session namespace/version | Done — mandatory `sk1:<owner>/<conversation>` format, `sk-v1` path component, list of negative tests | P-1 |
| lint was ineffective | Done — recursive, covers README, checks ownership claims, unindexed files are errors, fixture self-test, `package.json` entry point, tracked in the repository | — |

### 4th review changes (2026-08-31)

| Blocking defect | Handling |
| --- | --- |
| all-Prepared rollback does not hold after restart (resident state is volatile) | Revised the convention's 2PC table — Abort only when there are ×N resident attests with the same epoch; attest failure + valid staged → roll-forward; if neither exists, Inconsistent |
| No authoritative epoch store | Convention — new durable `CONTROL {epoch, generation}`; every publish binds (expected_epoch, expected_generation) |
| TrimTo partial commit undefined | TrimTo row in the convention's 2PC table — a partial truncation is rolled forward to the same position (idempotent); since TrimTo is issued only after the suffix discard is final, only forward progress is safe |
| Immutable bundle vs last_access conflict | Removed from meta and split out into an advisory `ACCESS` file; orphan gens must not be exposed without proof (quarantine/GC only) |
| 16-hex collision in the model path | Paths also use the full 64 hex; the Windows long-path prerequisite is stated |
| Contradictory session key grammar | 512 bytes = the entire raw value including the prefix, split at the first `/`, no Unicode normalization (byte identity), whitespace-only forbidden, negative tests aligned |

Gate claim strength corrected: docs-lint is a string canary, enforced by wiring it into cargo test,
but detecting semantic restatements is the review's job. We corrected 2 anchors that violated R2 themselves (`server.cpp`
without a symbol, the location of the KvReceiptState definition), and lint now rejects
symbol-less anchors. The inclusion of `p4-256-optimization.md` in the previous commit d33c2671f is the intended result of the README
indexing requirement (unindexed = error). We added the three-axis concurrency contract (external feedback) to the
batching document. **The next review still decides whether P-1 may start.**

### 5th review changes (2026-08-31)

We bound into the plan the separation between llama.cpp's abstraction layer (llama/ggml interfaces) and the concrete backends (CPU/CUDA/Metal).

| Blocking defect | Handling |
| --- | --- |
| "Only the values change" overclaim | Split Principle 2 into two layers — only the policy layer is value-independent; native compat is rebased on every pin (5/24 conflicts observed). The per-pin gate and the 3-way queue split are owned by U0 |
| Stage server coupled to private headers | Registered D20; U0 ③ (move behind a P4-owned versioned stage ABI) |
| HELLO lacks identifying power | Registered D21; finalized the U0 ② field list |
| No backend axis in identity | Added state_format, backend_family, and compatibility_id to the convention's layout identity; source×target backend axis in the P2 matrix |
| Misstated concurrency ownership | batching document — 2-layer split, requested value (coordinator) / physical cap (per-stage min); stated that `n_seq_max` is an echo of the configured value |
| Model gate 2 axes → 3 axes | Added the backend conformance axis (CPU mandatory on every pin) |
| CONTROL rename ≠ CAS | Convention — replaced with a `control.lock` exclusive-create critical section + crash recovery contract |
| TrimTo family capability | Convention — `trim_support = arbitrary\|bounded\|none` attest; when unsupported, downgrade to a full re-prefill (recurrent real-code anchor) |
| Shard-local risk in quota GC | Convention — exposed records are deleted only through coordinator Discard 2PC; local GC is limited to orphans |
| Reproducibility | Relabeled the "what works" section as past observations, committed the tracking of the compat queue and manifest; restoring the upstream pin is a U0 acceptance criterion |
| lint polluted by untracked files | Switched the official mode to files tracked by `git ls-files`; `--all` is separate |

**A P-1 completion verdict and native measurements are impossible before U0 passes.** This is subject to the next review's
verdict.

### 6th review changes + self-audit (2026-08-31)

From round 6 on, each round's deliverables include, alongside the fixes for review findings, a **self-audit of the logical soundness of the
whole plan**.

5 review findings:

| Blocking defect | Handling |
| --- | --- |
| Compat manifest EOL dependency | **Fixed** — `.gitattributes` + LF rehash (21 items), validate passes, fixture 6/6. The clean-checkout dual-autocrlf fixture and re-recording of patch_set/patched_tree are U0 ① acceptance criteria |
| Race from separated CONTROL compare and publish | Convention — compare and publish in the same critical section; lock owner `{host_instance_id, boot_id, pid, operation_id}`; added a P0 stale-break late-writer test |
| U0 content vs criteria mismatch | Expanded U0 acceptance criteria to cover all 7 items (enforced classification, 0 unclassified, absorbed-fix detection, allowed scope, promotion criteria, cross-backend negative test) |
| Over-pinned compatibility_id reintroduced | 4-way identity split — build_id (reference) / state_abi_id (path) / backend_layout_id (meta check) / restore matrix. cut_id excludes build provenance |
| "deterministic logits" cannot be judged | Replaced with numerical equivalence — NMSE, error bounds, greedy token sequence criteria; separate criteria for same-backend round trips and cross-backend |

Correction: "the dry-run cannot be reproduced independently because upstream is dirty" was wrong — the bump
script creates a clean worktree, and in round 6 we independently reproduced the 5/24 conflicts (0010, 0013,
0016, 0017, 0018).

Self-audit findings (holes the review did not point out):

| # | Hole | Handling |
| --- | --- | --- |
| a | Lease acquisition order for multi-shard operations undefined → possible deadlock | Convention — total order by ascending stage_index; release everything on failure |
| b | `epoch` term collision (store fencing vs P4.5 fragment) | Renamed separately as store_epoch/stream_epoch |
| c | U0 ② fields inconsistent with the 4-way identity split (single state_format value) | Updated the U0 field list to build_id/state_abi_id/backend layout |
| d | model_id assumes a single GGUF — mmproj and LoRA not included | Extended to an artifact-set digest; a different LoRA set = a different record |
| e | After TrimTo, the persisted record can be ahead of the resident state → risk that restore revives a dead suffix | Stated explicitly that Restore enforces the order `restore→LCP check→TrimTo` |
| f | The coordinator cannot read node-local ACCESS files | Split the roles: victim selection input comes from wire telemetry; ACCESS is for local persistence |
| g | Build provenance may leak into cut_id derivation | Stated that cut_id is derived from layout identity only |
| h | Restore's cell pre-acquisition depends on asynchronous release on each of the 4 nodes | Stated P3 cell reservation as all-or-release across all nodes (P3 item below) |
| i | Recurrent anchor used the repository commit format | Corrected to the upstream pin format; added a rule to R2 |

### 8th review changes + storage tier direction (2026-08-31)

8th verdict: approval withheld — 3 correctness blockers, 2 contract mismatches. O1~O8 were not counted
twice (the registration system worked as intended).

| Verdict | Handling |
| --- | --- |
| Checkpoint argument is valid, but avoid duplicate verbs | Accepted as a single shape, `Snapshot{after_commit: KeepResident\|ReleaseResident}` |
| Zero-copy branching is conditional | Stated the 3 conditions (same storage domain, compatible cut, detached after all stages complete) + read-pin (O10) |
| fragment credit is not quiescence-point evidence | Confirmed — plan.md §3 defines credit return as "the peer has taken it over". Replaced with a `SequenceQuiesced` attest; registered O9 |
| SnapshotList key intersection is insufficient (O8 registration confirmed) | Corrected the intersection unit to the logical snapshot tuple |
| Contradictory backend set wording (judged not closed) | Unified both cells of U0 ⑥ — {CPU baseline, CUDA production}; passing CUDA is a promotion condition |
| State ABI gate and reservation 2PC conditionally closed | Registered the conditions by extending the wording of O3 (fixtures exist) and O4 (reservation lifetime + TTL race) |

Additional direction (storage tier): the persistence destination is not only disk —
we added `tier = durable | ram | resident` to the convention. The ram tier is a volatile tier for fair swapping under
overload (offload part of the KV to RAM, serve other requests, then reload); its crash convergence is Absent (not corruption), it carries no cross-pin ABI burden, and
host bytes form a separate admission accounting axis (O11). All swap policy is issued as OUTER commands.

### Direction set: snapshot command model (2026-08-31)

We corrected the earlier narrowing of the persistence trigger to TTL. Branching workloads (persisting and copying existing KV
to branch the tree into a new session) are routine operations, so the adapter owns only the execution of the command
vocabulary (Persist/Checkpoint/SnapshotList/RestoreInto/Fork/Discard/Unload), and **all trigger policy is owned by
OUTER**. In the process we found one real conflict with an existing contract: the "one verb" argument for
`CacheAction::Persist` did not account for the use case of taking a footprint while keeping the resident state (Checkpoint).
`Fork {into}` is where the contract had already anticipated branching. The convention's
"snapshot command model" section owns the details.

### 7th review changes + self-audit (2026-08-31)

The EOL repair was approved, and the review externally verified, by applying the queue directly to a clean pin, that the aggregate
(patch_set_sha256, patched_tree) matches.

6 review findings:

| Blocking defect | Handling |
| --- | --- |
| state_abi_id does not see upstream state format changes | Convention — owned by the manifest, no manual entry, a per-pin state compatibility gate (per-family N-1→N restore, comparison of bytes, positions, and logits; a mandatory increment on failure) |
| No authority for stale judgement | Convention — compare lock_token and release token; topology decides the authority (new kv_root topology section); no automatic break when uncertain |
| Incomplete model_id + plan-convention R1 mismatch | Split into base_model_id × kv_variant_id (canonical encoding of role, scale, range), aligned P-1, added definition-ownership needles to lint |
| Reversed Restore→LCP order (direction error in self-audit item e) | Replaced with a decision ladder — LCP on tokens.bin before import; skip Restore when trim is impossible. The reversed wording is added to the discarded-phrase list |
| all-or-release is not distributed | New reservation 2PC (prepared accounting, local monotonic TTL, idempotency, Reconcile, failure tests) |
| production backend pinned to CUDA | Generalized to the required_backend_set declaration; stated that the current target is CUDA only |

We accept the self-audit re-verdict: done 2 (b, c) / partial 6 (a, d, f, g, h, i) / direction
error 1 (e — corrected in this round). Of the partial items, d and h were promoted to the review items above
and handled there.

New self-audit (not pointed out by the review):

| # | Hole | Handling |
| --- | --- | --- |
| α | Running the state compatibility gate on production service models makes the per-pin cost explode | Specified small fixture models + per-family golden states as repository test assets |
| β | Defining reservation TTL in absolute time reintroduces the clock skew problem | Defined on a local monotonic clock, measured from the time of receipt |
| γ | The root cause of the reversed-e conflict is cross-document duplication of the execution flow | Removed the Persist/Restore flow block from the batching document (link only) — eliminates the duplication class itself |
| δ | Layering error of demanding completeness from locks | Stated the principle that separates liveness (locks) from safety (store_epoch); a deployment where mutual exclusion does not hold is refused at load |
| ε | If prepared reservations are invisible to accounting, over-admit recurs | Added a reserved_cells telemetry field |
| ζ | Undecided whether kv_root is shared or node-local — the entire lock contract was conditional | New topology axis: default = node-dedicated local (eliminates the authority problem); a shared volume requires a storage capability gate + membership authority |

**Parts** of U0 ② and ③ came to hold on 2026-09-01. They must not be read as done; the exact
breakdown is as follows.

| Piece | Status | Basis |
| --- | --- | --- |
| ③a `src/llama-ext.h` isolation | Holds | Only `p4_llama_compat` has llama `src/` on its include path, and a second intrusion fails the build with C1083 (confirmed by injection) |
| ③b llama.cpp `common/` isolation | **Intermediate gate passed, not done** | **0 headers** (2026-09-02) — planning, checkpoints, sampler, speculative, seq-rm, and MTP bring-up all moved behind P4-owned handles and operations, and the gate **fails** if a header includes `common/` (confirmed by injection). **5 implementation files remain** (measured 2026-09-03) — 2 for request option parsing, 3 tests. The 4 plan-parsing items are closed: once the parser call moved into `LlamaPlan::parse_arguments()`, `server/plan.cpp` was fully decoupled from `common/`. Because upstream's `llama-common` exports its own directory as PUBLIC, the include path comes along for as long as a file that calls that library links it: **include and link end together when the last call moves to the facade.** The runtime target still links `llama-common`, so the boundary is not closed |
| ② Build identity | **Partial** | The three values `upstream_commit`, `patch_set`, and `backend_inventory` round-trip as real values from prepare's tree stamp and the ggml runtime registry through HELLO → adapter telemetry → OUTER (the 3090×2 run reports the full `CPU[CPU]|CUDA[CUDA0]`). `agree()` lives in the adapter crate and rejects `unknown` fail-closed, but **its only real caller is event-drive, so the product load path does not enforce the rule** — this repository has no coordinator API that assembles the whole pipeline. `stage_abi_id`, `state_abi_id`, and `trim_support` are also absent, so no composite `build_id` was created either |

**Correction (2026-09-02)**: This document previously explained that the run reported only `CUDA[CUDA0]`
"because each stage sees only one device via `CUDA_VISIBLE_DEVICES`."
**That was wrong.** The stage server sent `CUDA[CUDA0];CPU[CPU]`, and the `;` inside the value
collided with the field separator of the capability string, so the adapter cut the value at the first `;` — the CPU
registry became an unnamed field and disappeared. We made up the explanation from expectations without reading the artifact,
and that explanation hid a real defect.

Fix: values are separated by `|`, all names are percent-escaped, and registries are
sorted (so that a different plugin load order does not yield a different value). We also added **4 round-trip tests that pass the actual HELLO
string through the parser** — the existing negative tests built `BuildIdentity`
by hand and compared it, so they never exercised this path at all.
With the old encoding injected, the test catches the value truncated to `CUDA[CUDA0]` (confirmed).

**The name is corrected too: `backend_inventory`.** This function only enumerates the backends and
devices registered in the process; it does not look at where model tensors, KV, and compute buffers were actually placed.
A run placed entirely on CUDA and a run that fell back broadly to host buffers
produce the same value. The value bound to persistent state compatibility must be **a separate
`execution_layout_id` taken from placement**, and it does not exist yet.

It is true that each stage sees only one device, but that is a separate limitation: it cannot distinguish
physical GPUs — the execution evidence records placement and GPU UUIDs separately.


**Not yet enforced**: `agree()` being in the adapter is not the same as the product calling it.
Right now the only non-test caller of this rule is `tools/event-drive`, and the adapter's LOAD
receive path only puts its own stage's identity into telemetry; it does not assemble the pipeline.
The product OUTER compiles and runs without calling this API, so the contract is not structurally
enforced. Closing this requires either **allowing only types that passed identification to proceed to the inference phase**, or having LOAD
carry the expected identity declared by OUTER so that each stage checks its own —
both are contract decisions and must be settled before implementation.

### 2026-09-10 physical wire compatibility contract for the fleet

This is a separate implementation candidate, following the user's directive to integrate CUDA and Metal machines. The existing `agree()` and the
`exact-build` default still refuse when backend/device inventories differ.
Only an OUTER that explicitly specifies `physical-wire-v4` checks all of the following and then proceeds to SESSION/inference.

- Upstream and patch-set must be identical and identified across all stages.
- native puts the `p4pb4le64` codec source SHA256 and the actual ggml type/block/byte sizes into HELLO.
  The native source fingerprint is independent of checkout path and CRLF, and covers the native production sources and CMake.
  Anything other than a little-endian, 64-bit, IEEE754 float32, 32-bit token/position/sequence representation is `unknown`.
- The adapter forwards those values unchanged in LOADED. OUTER checks that format, source, and representation match,
  and refuses unknown/missing values or a different pin, patch, codec, or representation even when the backend is the same. The option that allows unidentified builds cannot bypass this condition either.
- The backend inventory is neither hidden nor forged into a merged string. The `stage_builds` artifact preserves
  agent/node/generation and each identity in topology order. The existing `build` is the head's compatibility field.

This contract is limited to tensor transfer in physical capsule v4. It does not change the ledger, control approval, settlement, or KV ownership, and
it does not guarantee KV snapshot portability across heterogeneous backends, numerical bit-identity, legitimacy of arbitrary plugins, or source authentication.
Raw ggml type values are bound to the same pin/patch and the runtime type table. CUDA/Metal interoperation is
approved for a given combination only when the actual model consumption path, negative tests, independent mutation, and execution evidence all pass.
The current enforcement point for the set comparison is the event-drive OUTER. We do not claim LOAD enforcement for arbitrary external product callers, nor
that an authenticated fleet coordinator has been implemented. For the latest verification status, follow roadmap §0 and the fleet evidence.

**Actual classification of the 9 remaining ③b items** (2026-09-02, rechecked file by file): This document previously lumped all nine
together as "all CLI/option grammar, so a contract decision comes first." **That was wrong.** In reality:

| File | What it uses | Does moving it need a contract? |
| --- | --- | --- |
| `server/plan.cpp` | `common_params_parse`, `common_args`, `common_speculative_type` | Yes — the llama.cpp CLI grammar itself |
| `runtime/request_options.cpp` | `common_sampler_types_from_*`, option fields | Yes — request option grammar |
| `runtime/request_options_grammar.cpp` | `common_grammar_trigger` | Yes — grammar trigger representation |
| `runtime/request_stops.cpp` | `string_find_partial_stop` | **No** — a simple helper; wrapping it is enough |
| 5 tests | Check parser results directly | No — switching them to facade-observed values is enough |

So only **3 operational items** depend on a contract, and the other 6 can be moved now. And since
`plan.cpp` still reads `common_speculative_type` and `has_dft()` directly, **"sampler/
speculative API changes stop at compat.cpp alone" is not yet true** — it is true for the runtime
and false for plan parsing.

**③c Result of actually executing that table** (2026-09-03): Of the 6 items the table above counted as "can be moved now",
**only 4 were actually moved** — `request_stops.cpp`, `capability_test.cpp`,
`plan_invariants_test.cpp`, and `server/plan.cpp`, which the table had classified as "needs a contract".
The table's predictions were wrong in both directions, so we record them as they are.

* `plan.cpp` did not need a contract. What had to move was not the parser *grammar* but the parser *call site*,
  and once `LlamaPlan::parse_arguments()` took over that call, only four predicates
  remained. The grammar is still llama.cpp's — what changed is who calls it.
  The last place that read `common_speculative_type` directly is gone, so now **"sampler/
  speculative API changes stop at compat.cpp alone" is true for plan parsing as well.**
* 3 of the 5 tests cannot honestly be moved. `request_options_test.cpp` checks 27 sampling
  fields, and moving it would mean duplicating those 27 fields in the facade — a mirror, not a
  boundary. `compile_test.cpp` and `mtp_ownership_test.cpp` do not link `p4_staged_server_core`, which holds the
  parser; they link only `p4_staged_llama_runtime`. Adding that link just to make the tests
  pass would build the layering upside down.

The measured remaining debt is 0 headers / 5 sources, and of those **the only operational files are the 2 request option grammar files**
(`request_options.cpp`, `request_options_grammar.cpp`). The other 3 are the tests above.

The boundary is also not a replica of the whole llama.cpp CLI. The minimal tracking-cost shape is a P4-owned
typed plan/request contract, with `common_params_parse` and the sampler/grammar conversions kept inside the compat
target, and tests that verify facade-observed values instead of raw `common_params`.


Because the handles took over not only ownership but also operations, sampler and speculative API changes
now stop at `p4_llama_compat.cpp` alone. Breaking draft batch semantics once and then
reverting shows the real risk of this move — upstream drafts all configured sequences
in one batch, and switching to per-sequence `draft()` computes something different. The facade
splits `configure_draft` from `run_draft` so that the distinction stays in the types.

The investigation itself was also disproved once. Counting only the `common_*` prefix missed symbols such as
`string_find_partial_stop`, and removing the include made the compiler point them out. The surface of a convenience
library is not defined by a prefix.

| ③c P4-owned versioned stage ABI | Not started | — |
| ② `patch_set` round trip | Holds | prepare stamp → CMake → HELLO → loaded telemetry → OUTER, recorded in run artifacts |
| ② Refusal on stage mismatch | Holds | Refuses differing patch-sets + refuses stages that cannot report their build (fail-closed; bypass only via an explicit environment variable) |
| ② `stage_abi_id`, `state_abi_id`, backend layout, `trim_support` | Not started | So no composite `build_id` was created either — a composite identifier built before its inputs exist would claim to distinguish what it cannot distinguish |

CPU and CUDA builds from the same pin currently have the same `patch_set`, so **they are treated as the same build**.
That is because the backend, plugin, device, and buffer layout axes are not in HELLO, and this hole remains open until the rest of
② is closed.


**We still do not claim U0 or P-1 completion.** The scope that can proceed in parallel is the session_key
wire and the harness migration; the base×variant implementation comes after review approval of this contract.

The currently demonstrated scope of the session_key wire is **OUTER→adapter, one way**. In a 3090×2 remote
4-node run, we used the adapter's own trace to check that the key issued by the harness and the key the adapter held at admission time
were the same, and admission refuses when the same request_id reappears with a different key.
**The storage and post-restart Restore leg does not exist yet** —
that round trip can hold only after the state namespace (P0) and snapshot commands (P3) exist, and
the other half of the P-1 acceptance criteria stays unmet until then.

## known open surface

These are the unresolved surfaces we ourselves expect the next review to point out. A finding on an item registered
here is judged as "registration confirmed", not "new discovery".

| # | Surface | Planned owner |
| --- | --- | --- |
| O1 | No contract for the telemetry channel itself — which events carry reserved_cells, last_access, and session occupancy, and with what period and ordering guarantees | P1a |
| O2 | U0 ③'s stage ABI only says "remove it" — the actual surface (functions, types) of the P4-owned versioned ABI is undefined | U0 design deliverable |
| O3 | Family coverage of the state gate fixtures — unconfirmed whether small golden models actually exist for each of kv_cache/iswa/hybrid/recurrent | P2 preparation |
| O4 | Relationship between reservation 2PC and the session lease — whether a reservation presupposes a lease, the acquisition order of the two coordination layers, and the winner rule when the reservation lifetime `Prepared→Committed→Consumed/Released` races TTL expiry and Commit | P0 |
| O5 | ~~Whether the session_key wire extension really stays inside the adapter content-type~~ — resolved: the field lives only inside the adapter's `InferenceCommand` and the P4 protocol is unchanged; a 4-node remote run proved that the value issued by OUTER and the value held by the adapter are identical | P-1 |
| O6 | Contract wording for `Snapshot{after_commit}`, `SnapshotList`, and RestoreInto — the direction is set (snapshot command model), but the exact p4-adapter verb signatures, the 2PC coupling, and **the immutable-ID/mutable-ref choice for Fork and snapshot keys** are not written | P-1 contract, P3 implementation |
| O7 | Capability negotiation for resident-tier checkpoints (tier) | P7 |
| O8 | Recovery of the coordinator's snapshot ledger — the intersection unit is the logical snapshot tuple, not the key (convention correction done); the reconstruction procedure after an OUTER restart is itself not written | P3 |
| O9 | Sequence quiescence — a per-stage `SequenceQuiesced` attest contract separate from credit (credit return is evidence of takeover, not of compute/KV completion; plan.md §3) | P4.5 |
| O10 | Snapshot storage domain and read pin — source and target leases, blocking concurrent Discard, node moves and cross-domain copy paths | P3 |
| O11 | durable/ram-byte admission — per-node disk and host RAM reservation, ENOSPC partial-prepare convergence, OUTER available-capacity telemetry | P3 |
| O12 | 1 observation of a remote agent that had finished one run failing to accept a second run — no stage came up, the agent log showed no trace of receipt, and the drive waited until timeout_ms (2026-09-01; the same scenario was normal after a restart). Cause undetermined: not yet separated whether it is adapter state across the load generation transition or tunnel/connection lifetime. The current harness works around it by restarting the agent for every run | Unassigned |
| O13 | ~~OUTER delivery loss~~ — root cause found: the drive's `EventWire::receive` wrapped `read_u32_le`/`read_exact` in `tokio::time::timeout`, and those calls are not cancel-safe, so on cancellation the bytes they had already consumed were lost and the stream fell out of alignment. The drive **intentionally** times out on every arriving wave, so every wave boundary was a chance to misalign, and a garbage length prefix swallowed an arbitrary span before the stream silently resynchronized — this is what the "position discontinuity" really was. Fixed with a reader whose buffer outlives cancellation, and pinned by 2 tests that fail on the old reader and pass on the new one. The adapter's discard counter and dead-path eviction are kept as they are (as a means of observing follow-on symptoms) | Resolved |

## Review convergence rules

These rules keep repeated reviews from finding the same kind of defect again.

- R1 **Single ownership of claims**: a given contract, order, or defect attribution is owned by exactly one document, and
  other documents link to it. Stale duplicate descriptions caused this round's conflicts.
- R2 **Mandatory code anchors**: a sentence that describes code behavior carries a `path::symbol @
  short-commit` anchor or, if not yet verified, is explicitly marked as a "target contract".
  Line-number-only anchors rot as code moves, so they are not used. Anchors to upstream
  files use the **upstream pin commit**, not the repository commit.
  lint checks only the `@` form, so detecting new claims without anchors is the review's job.
- R3 **A name needs a contract**: a new identifier, key, or digest cannot be introduced by name alone until its 5 elements are defined:
  normalization, uniqueness scope, version, comparison rule, and negative tests.
- R4 **Phase verb discipline**: a phase-to-defect link uses exactly one of the verbs
  measure/reproduce/fix/verify. An "observe" phase cannot claim "resolve".
- R5 **Machine checks**: `npm run docs-lint` recursively checks all project Markdown except vendored/build
  files — it rejects as errors mixed EOL within a file, discarded phrases (README included), restated
  ownership claims, unindexed docs/ files (an actual link form in the README is required), and code anchors without a
  symbol, and `cargo test --workspace` runs this check
  (entrypoints/agent/tests/docs_lint.rs). The limits are part of the contract too:
  it is a **string canary**, so it does not catch restatements that change the meaning; detecting those is
  the review's job. The official mode checks only Markdown tracked per `git ls-files`, so
  unrelated untracked drafts do not pollute the gate or the commit scope;
  `--all` checks the whole filesystem. There is no CI/pre-commit wiring yet.
- R6 **Feedback conversion rule**: a review finding is closed only by converting it into one of (a) a fix to an anchored claim, (b) an executable
  test or gate, or (c) an open decision with an owning phase.
  A text edit alone does not close it.

## Measurement harness

The current `target/gemma4-4node/` is git-ignored, so it cannot serve as a baseline. In P-1 it moves under
versioned `test/benchmarks/`, and the evidence for every acceptance verdict records the
commit, compat manifest, full spec, environment, summarizer version, and source artifact checksums.
The currently preserved stress artifacts are the 2/2 successful runs, not the D4 40-request failure
evidence — that evidence is to be re-acquired in P-1.

## Document map

| Document | Owns |
| --- | --- |
| This document | Overall restructure plan, defect register, phases |
| [adapter-batching-layers.md](adapter-batching-layers.md) | L0~L5 contracts, invariants, strategy modules |
| [kv-state-store-convention.md](kv-state-store-convention.md) | Record identity, directories, lifetime |
| [llamacpp-stage-memory.md](llamacpp-stage-memory.md) | Stage memory ownership, legal cuts |
| [event-protocol-v2.md](event-protocol-v2.md) | Event contract, gate proof order |
| [plan.md](plan.md) | 2026-08-21 plan (historical document; the §3 Edge credit normative content has moved to the P4.5 row) |
