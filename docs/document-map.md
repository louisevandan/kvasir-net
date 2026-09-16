# Full document map and authority

Organized 2026-09-06. This file manages the **classification, reading paths and contract ownership** of all repository-owned Markdown.
Migration does not delete existing records. The status labels below also do not mean that the code claims in every past body text were newly certified.
The current code audit scope and open defects are stated in the roadmap and in the evidence for the relevant date.

## 1. What to read in a new session

1. [AGENTS.md](../AGENTS.md): repository work, verification and reporting rules.
2. [Current status in the distributed batching roadmap](distributed-batching-roadmap.md#current-status): verified progress / unverified work / stop and resume conditions. Do not execute from an old "next" item in the chronological record.
3. [Distributed batching verification protocol](distributed-batching-verification.md): deterministic counterexamples and real-hardware approval conditions.
4. [Layer isolation contract](layer-isolation-contract.md): P4/adapter/native/llama/backend responsibilities and absorbing upstream impact.
5. For new release development, read the handoff in the [single development plan](external-analysis-improvement-plan.md#fresh-session), then the [HF acceptance contract](distributed-batching-verification.md#hf-integration-contract) first, and after that the [A acceptance contract](distributed-batching-verification.md#release-a-contract).
6. The domain owner documents below and the actual implementation paths: read only as much as the contract you are fixing now requires.

Very large models, multiple physical computers, strong request waves and complete correct response bodies are the conditions for the final outcome.
Do not promote small-model or single-machine measurements, simulator completion, or old U/P numbers into new completion criteria.

## 2. Contract ownership and conflict resolution

| Topic | Sole owner / usage rule |
| --- | --- |
| Current goal, status, development order, stage promotion | [Roadmap](distributed-batching-roadmap.md). Other documents link to it instead of duplicating the order. |
| Test inputs, mutation, verdicts, real-hardware waves, reported metrics | [Verification protocol](distributed-batching-verification.md). Distinguish "a run command exists" from "currently passes". |
| Per-layer roles, allowed dependencies, public types, update adaptation boundary | [Layer isolation contract](layer-isolation-contract.md). Do not read the existing include-debt figures as whole-structure isolation. |
| Document status and lookup paths | This document. When you create a new document, also register it in the full list below. |
| Studio observability requirements and cost-bounded candidates | [Observability acceptance proposal](inference-observability-proposal.md). A design proposal; not an implementation, a default, a real-hardware approval or a change to the existing execution order. |
| Backend-neutral boundary of the event wire/forwarding | [event-protocol-v2](event-protocol-v2.md); the proof order in its body, written at the time, does not replace the current roadmap. |
| Adapter batching layers, mechanism/policy ownership | [adapter-batching-layers](adapter-batching-layers.md); includes unimplemented target contracts. |
| Persistent identity, namespace, CONTROL, 2PC, snapshot/LCP | [kv-state-store-convention](kv-state-store-convention.md); do not enable before each feature's K-branch implementation and failure gates pass. |
| Stage memory, legal cuts | [llamacpp-stage-memory](llamacpp-stage-memory.md); real conformance is required for each model/backend combination. |
| MTP/OUTER/deployment boundaries | The respective domain documents. Report only currently enabled features as complete; disabled, unaudited features stay fail-closed. |
| How to run | [testing](testing.md), component READMEs. Recheck the current event entry point and the actual command arguments against the code. |
| Past causes, alternatives, U/P defect ledger | Historical documents such as [adapter-restructure-plan](adapter-restructure-plan.md). The requirements moved to the new roadmap, but the old order was discarded. |
| Past test and real-hardware measurements | [runtime-evidence](runtime-evidence.md) and dated evidence. Do not generalize beyond their source/binary/model/workload/topology scope. |

User instructions take top priority. When documents contradict each other, do not choose by latest date alone; first check the owner in this table and
any explicit migration in the roadmap. When code and contract differ, the code is not automatically correct, and the document is not implementation evidence.
Record the difference as a counterexample or an incomplete item and handle it in the relevant implementation stage.

When changing a domain contract, change the owner document and update consuming documents only in their links and short scope notes.
Do not delete past evidence figures or failure history. The old "then stop", fixed 4 nodes, splitting only into as many parts as there are GPUs,
and the conclusion that hops are always cheap are date-scoped hypotheses, not current product rules.

## 3. Full list

There are currently 79 registered entries. The 73 previously tracked documents were classified, and 6 handoff documents/evidence files from this round were added.
Read the detailed body of each file below only within the scope of its role. The figures are a convenience summary; the docs gate checks whether anything is actually missing.

| Document | Status |
| --- | --- |
| [AGENTS.md](../AGENTS.md) | New-session rules |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-fleet-latest-integration.md) | Fleet/NAS measurements and latest upstream integration: progress and unapproved gates |
| [docs/adapter-batching-layers.md](adapter-batching-layers.md) | Domain contract; distinct from implementation |
| [docs/adapter-boundary.md](adapter-boundary.md) | Domain contract; distinct from implementation |
| [docs/adapter-restructure-plan.md](adapter-restructure-plan.md) | Historical / old plan |
| [docs/api.md](api.md) | Path-specific reference; needs re-audit |
| [docs/architecture.md](architecture.md) | Path-specific reference; needs re-audit |
| [docs/node-load-lifecycle-plan.md](node-load-lifecycle-plan.md) | Completed implementation plan: M0–M4 accepted with real models on both adapters and in docs; overall load coordination is OUTER's responsibility |
| [docs/deterministic-execution-register.md](deterministic-execution-register.md) | Active contract: reuses root-cause evidence of repeated failures as automatic up-front blocks and as tests for the next stage |
| [docs/batching-code-review.md](batching-code-review.md) | 2026-09-13 code review of the batching selector, worker and native hand-off boundary; differences from the proposal, improvement candidates, local reproduction; not an execution order or performance promotion |
| [docs/constraints.md](constraints.md) | Path-specific reference; needs re-audit |
| [docs/continuous-inference-refactor-handoff.md](continuous-inference-refactor-handoff.md) | Historical / old plan |
| [docs/deployment-adapter-contract.md](deployment-adapter-contract.md) | Domain contract; distinct from implementation |
| [docs/distributed-batching-roadmap.md](distributed-batching-roadmap.md) | Owner of current goal, status and order |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md) | MI250 and Hy3 diagnosis, bounded-selection implementation, concurrent real-hardware screening; H5 performance/service approval incomplete |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-dense-load.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-dense-load.md) | Windows CUDA 4-stage load and inference of an old MiniMax M3 GGUF without MSA, and MSA re-audit |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-13-minimax-m3-msa-distributed-rejection.md) | CUDA and Metal distributed plan/LOAD rejection of a MiniMax M3 GGUF with correct MSA, and withdrawal of the incomplete opt-in |
| [docs/distributed-batching-verification.md](distributed-batching-verification.md) | Owner of test and real-hardware verdicts |
| [docs/distributed-mock-test-plan.md](distributed-mock-test-plan.md) | Partial test plan |
| [docs/document-map.md](document-map.md) | Owner of document classification and authority |
| [docs/event-protocol-v2.md](event-protocol-v2.md) | Domain contract; distinct from implementation |
| [docs/external-analysis-improvement-plan.md](external-analysis-improvement-plan.md) | Single development plan, external HF acceptance priority, Release A implementation scope / new-session handoff, investment rationale for batching G1–G6, sparse state and DFlash/DSpark; execution order is owned by the roadmap, verdicts by the verification protocol |
| [docs/implementation.md](implementation.md) | Path-specific reference; needs re-audit |
| [docs/inference-observability-proposal.md](inference-observability-proposal.md) | Proposal for always-on aggregation, per-request summaries, optional diagnostics and a traffic/performance budget for Studio observability requirements; not implemented, not measured |
| [docs/internals.md](internals.md) | Path-specific reference; needs re-audit |
| [docs/kv-state-store-convention.md](kv-state-store-convention.md) | Domain contract; distinct from implementation |
| [docs/layer-isolation-contract.md](layer-isolation-contract.md) | Owner of per-layer responsibilities and upstream isolation |
| [docs/llamacpp-stage-memory.md](llamacpp-stage-memory.md) | Domain contract; distinct from implementation |
| [docs/outer-acceptance-test-plan.md](outer-acceptance-test-plan.md) | Partial test plan |
| [docs/overview.md](overview.md) | Path-specific reference; needs re-audit |
| [docs/p4-256-optimization.md](p4-256-optimization.md) | Historical / old plan |
| [docs/plan.md](plan.md) | Historical / old plan |
| [docs/presentation/p4-structure.md](presentation/p4-structure.md) | Structure explainer — Markdown version of the deck for general developers |
| [docs/protocol-mtp.md](protocol-mtp.md) | Domain contract; distinct from implementation |
| [docs/protocol-outer.md](protocol-outer.md) | Domain contract; distinct from implementation |
| [docs/protocol.md](protocol.md) | Path-specific reference; needs re-audit |
| [docs/runtime-evidence.md](runtime-evidence.md) | Evidence index |
| [docs/testing.md](testing.md) | Gate execution guide |
| [docs/usage.md](usage.md) | Path-specific reference; needs re-audit |
| [entrypoints/README.md](../entrypoints/README.md) | Component guide |
| [layers/adapters/adapter/README.md](../layers/adapters/adapter/README.md) | Component guide |
| [layers/adapters/llamacpp/README.md](../layers/adapters/llamacpp/README.md) | Component guide |
| [layers/adapters/llamacpp/staged/compat/1269cb1ff/README.md](../layers/adapters/llamacpp/staged/compat/1269cb1ff/README.md) | Per-pin compatibility record |
| [layers/adapters/llamacpp/staged/compat/3e3a7a416/README.md](../layers/adapters/llamacpp/staged/compat/3e3a7a416/README.md) | Per-pin compatibility record |
| [layers/adapters/llamacpp/staged/compat/4308a4f03/README.md](../layers/adapters/llamacpp/staged/compat/4308a4f03/README.md) | Per-pin compatibility record |
| [layers/adapters/llamacpp/staged/compat/d7a207411/README.md](../layers/adapters/llamacpp/staged/compat/d7a207411/README.md) | Per-pin compatibility record |
| [layers/adapters/llamacpp/staged/compat/ef6876693/README.md](../layers/adapters/llamacpp/staged/compat/ef6876693/README.md) | Per-pin compatibility record |
| [layers/adapters/llamacpp/staged/compat/fe2adf0e7/README.md](../layers/adapters/llamacpp/staged/compat/fe2adf0e7/README.md) | Per-pin compatibility record |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-5000-token-prefill-generation.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-5000-token-prefill-generation.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-cuda-toolchain.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-cuda-toolchain.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-current-nonmtp-options-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-current-nonmtp-options-four-node.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-four-node.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-live-audit.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-final-live-audit.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-four-node-lap-fix.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-four-node-lap-fix.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-gate5-pass-through.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-gate5-pass-through.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-independent-nonmtp-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-independent-nonmtp-four-node.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-manifest-p1.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-manifest-p1.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-unified-forwarding.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-kv-unified-forwarding.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-latest-cuda-four-node-regression.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-latest-cuda-four-node-regression.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-logits.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-logits.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-minimax-multishard-partial-stage.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-minimax-multishard-partial-stage.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-model-vram.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-model-vram.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-auxiliary-ownership-probe.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-auxiliary-ownership-probe.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-speculative-minimum-slice-audit.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp-speculative-minimum-slice-audit.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-mtp.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-multi-stage-kv-e2e.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-multi-stage-kv-e2e.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-native-context-checkpoint.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-native-context-checkpoint.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-options-wire-four-node-regression.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-options-wire-four-node-regression.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-remote-four-node.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-remote-four-node.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-options-semantics.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-options-semantics.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-sampling-options.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-request-sampling-options.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-s-model-qwen2.5-1.5b.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-s-model-qwen2.5-1.5b.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-state-store-local.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-18-state-store-local.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-batched-decode-throughput.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-batched-decode-throughput.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-decode-hop-cost-decomposition.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-decode-hop-cost-decomposition.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-four-node-35b-service-reference.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-08-20-four-node-35b-service-reference.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-03-load-and-batching.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-03-load-and-batching.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-04-35b-and-the-width-collapse.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-04-35b-and-the-width-collapse.md) | Date- and environment-scoped evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md) | Baseline commit review evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-07-head-verification-and-3090x2-ladder.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-07-head-verification-and-3090x2-ladder.md) | Compile, test and mutation verdicts for HEAD `2ed9b71d4`, and 3090×2 real-hardware ladder evidence |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md) | Evidence for the defect where the `no_alloc` memory plan under-reported compute, and the `0025` fix |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-model-load-catalog.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-model-load-catalog.md) | Report on real-hardware loads of 33 models and per-context memory measurements |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-measurement-trust-recovery.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-measurement-trust-recovery.md) | Correction of the retracted width correlation, and evidence for the fixes to three defects that obscured measurement |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-control-receipt-budget.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-control-receipt-budget.md) | Root cause, fix and mutation verification for the control-response budget that blocked release and settlement |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-pressure-measured-baseline.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-pressure-measured-baseline.md) | Measured baseline for responses, TPS, batch saturation and GPU from the first completed `pressure` run |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-saturation-and-utilisation.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-saturation-and-utilisation.md) | 10 saturation/utilization experiments varying model size, split, resident, publishing policy and arrival pattern |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-partial-result-preservation.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-09-partial-result-preservation.md) | Root cause, fix and mutation verification for the defect where a failed run lost its partial results |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-noalloc-recurrent-residency.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-noalloc-recurrent-residency.md) | Confirmed root cause of the upstream defect where plan mode actually allocated recurrent state, and the compat patch |
| [layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-10-release-gate-v0.9.0.md) | Binding of the v0.9.0 release artifacts and verdicts for the five 3090×2 real-hardware gates |
| [docs/release/v0.9.0.md](release/v0.9.0.md) | v0.9.0 release notes — included changes, what was verified, what was not verified, known constraints, supported configurations |
| [layers/adapters/llamacpp/staged/scripts/validation/README.md](../layers/adapters/llamacpp/staged/scripts/validation/README.md) | Component guide |
| [layers/adapters/llamacpp/staged/server/tests/contract/README.md](../layers/adapters/llamacpp/staged/server/tests/contract/README.md) | Component guide |
| [layers/adapters/README.md](../layers/adapters/README.md) | Component guide |
| [layers/protocol/README.md](../layers/protocol/README.md) | Component guide |
| [layers/README.md](../layers/README.md) | Component guide |
| [llamaAdaper.md](../llamaAdaper.md) | Historical / old plan |
| [P4_REVISION_PLAN.md](../P4_REVISION_PLAN.md) | Historical / old plan |
| [README.md](../README.md) | Entry point |
| [test/benchmarks/p4-4node/README.md](../test/benchmarks/p4-4node/README.md) | Development harness guide |
| [test/benchmarks/model-catalog/README.md](../test/benchmarks/model-catalog/README.md) | Catalog of OUTER model load parameters and measured memory |
| [test/benchmarks/cluster-inference/README.md](../test/benchmarks/cluster-inference/README.md) | Common experiment composer on main; model/cluster/runtime/policy/workload separation |
| [tools/model-loading/README.md](../tools/model-loading/README.md) | P4 model loading functions, reference/policy evaluation, tests, local evidence |
| [tools/README.md](../tools/README.md) | Component guide |

## 4. Maintenance rules

- When you add, move or delete project-owned Markdown, update this list and the docs page index in the root README together.
- Mark each document's status at the top: current contract / goal / path-specific reference / historical evidence.
- docs-lint checks only for files missing from this list, broken targets, mixed EOL within a file, and some discarded phrases/anchors.
  It does not automatically certify natural-language semantic conflicts, whether the source was verified, or unimplemented T/H tests.
- The default gate checks only Git-tracked files. Check a new document separately with `--all`, even before staging.
  Do not force unrelated untracked drafts into the official list or into commits.
- Build, vendor and temporary evidence are not part of the exhaustive list. Keep long-term evidence as dated documents and links to recoverable artifacts.
- Record current status updates only in the roadmap, and measurement details only in evidence. Do not spread duplicated status tables across several READMEs.

Internal HF build and run, and acceptance on both adapters: [HF integration guide](hf-integration.md).

Release A: [staged test plan](../tests/plans/release-a-20260914.md) · [A-RED/trace audit](../tests/reports/release-a/20260914_040306.md).
Corpus and spec validation: [preparation results](../tests/reports/release-a/20260914_041920.md).
Native preparation and both adapters: [regression results](../tests/reports/release-a/20260914_043652.md).
Native cost observation: [actual CPU path and removal mutation](../tests/reports/release-a/20260914_050400.md).
Current pin and deployment candidate: [token, CUDA/Metal, both adapters, cost attribution](../tests/reports/release-a/20260914_054100.md).
Fleet preflight: [7-host connectivity, 6 native PLANs, model access blocked](../tests/reports/release-a/20260914_062025.md).
App runtime environment recheck: [model access, native PLAN 8/8](../tests/reports/release-a/20260914_102600.md).
Follow-up real-hardware run preparation: [reclaiming existing fleet nodes](../tests/reports/release-a/20260915_005456.md).
Current fleet resumption: [rebuild, both adapters, 550B arm](../tests/reports/release-a/20260915_011200.md).
Taking over concurrent HF work: [loading planner reproduction plan](../layers/adapters/hf/tests/plans/loading-planner-20260915.md) · [verification](../layers/adapters/hf/tests/reports/loading-planner/20260915_013600.md).

## HF adapter documents

| Document | Status |
| --- | --- |
| [layers/adapters/hf/adapter/docs/api.md](../layers/adapters/hf/adapter/docs/api.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/architecture.md](../layers/adapters/hf/adapter/docs/architecture.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/constraints.md](../layers/adapters/hf/adapter/docs/constraints.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/internals.md](../layers/adapters/hf/adapter/docs/internals.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/overview.md](../layers/adapters/hf/adapter/docs/overview.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/testing.md](../layers/adapters/hf/adapter/docs/testing.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/docs/usage.md](../layers/adapters/hf/adapter/docs/usage.md) | HF configuration, contract, verification |
| [layers/adapters/hf/adapter/README.md](../layers/adapters/hf/adapter/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/AGENTS.md](../layers/adapters/hf/AGENTS.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/api.md](../layers/adapters/hf/docs/api.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/architecture.md](../layers/adapters/hf/docs/architecture.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/constraints.md](../layers/adapters/hf/docs/constraints.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/history/initial/api.md](../layers/adapters/hf/docs/history/initial/api.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/architecture.md](../layers/adapters/hf/docs/history/initial/architecture.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/bootstrap-evidence.md](../layers/adapters/hf/docs/history/initial/bootstrap-evidence.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/constraints.md](../layers/adapters/hf/docs/history/initial/constraints.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/decisions.md](../layers/adapters/hf/docs/history/initial/decisions.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/development-plan.md](../layers/adapters/hf/docs/history/initial/development-plan.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/HANDOFF.md](../layers/adapters/hf/docs/history/initial/HANDOFF.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/internals.md](../layers/adapters/hf/docs/history/initial/internals.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/overview.md](../layers/adapters/hf/docs/history/initial/overview.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/references.md](../layers/adapters/hf/docs/history/initial/references.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/roadmap.md](../layers/adapters/hf/docs/history/initial/roadmap.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/testing.md](../layers/adapters/hf/docs/history/initial/testing.md) | HF historical record |
| [layers/adapters/hf/docs/history/initial/usage.md](../layers/adapters/hf/docs/history/initial/usage.md) | HF historical record |
| [layers/adapters/hf/docs/integration/README.md](../layers/adapters/hf/docs/integration/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/internals.md](../layers/adapters/hf/docs/internals.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/migration/README.md](../layers/adapters/hf/docs/migration/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md](../layers/adapters/hf/docs/models/qwen3_5_0_8b/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/overview.md](../layers/adapters/hf/docs/overview.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/quantization.md](../layers/adapters/hf/docs/quantization.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/structure/README.md](../layers/adapters/hf/docs/structure/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/testing.md](../layers/adapters/hf/docs/testing.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/transport/framing/README.md](../layers/adapters/hf/docs/transport/framing/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/docs/usage.md](../layers/adapters/hf/docs/usage.md) | HF configuration, contract, verification |
| [layers/adapters/hf/README.md](../layers/adapters/hf/README.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/framing-20260913.md](../layers/adapters/hf/tests/plans/framing-20260913.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/migration-20260914.md](../layers/adapters/hf/tests/plans/migration-20260914.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/p4-integration-20260914.md](../layers/adapters/hf/tests/plans/p4-integration-20260914.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md](../layers/adapters/hf/tests/plans/qwen3_5_0_8b-20260913.md) | HF configuration, contract, verification |
| [layers/adapters/hf/tests/reports/framing/20260913_174516.md](../layers/adapters/hf/tests/reports/framing/20260913_174516.md) | HF historical record |
| [layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md](../layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md) | HF historical record |
| [layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md](../layers/adapters/hf/tests/reports/qwen3_5_0_8b/20260913_220709.md) | HF historical record |
| [layers/adapters/hf/tests/reports/migration/20260914_120000.md](../layers/adapters/hf/tests/reports/migration/20260914_120000.md) | HF internal integration verification and source cleanup results |

Connection reclaim WIP: [verification plan](../tests/plans/release-a-transport-20260915.md) · [stop report after 3 failures](../tests/reports/release-a/20260915_044735.md).

Routing through the ingress agent: [verification plan](../tests/plans/ingress-envelope-20260915.md) · [envelope and both-adapter verification](../tests/reports/release-a/20260915_113754.md).

Commission return context: [verification plan](../tests/plans/return-context-20260915.md).
Return context implementation verification: [common contract, two adapters, independent mutation](../tests/reports/release-a/20260915_121000.md).

Cluster return route: [test plan](../tests/plans/cluster-envelope-20260915.md) · [9 hosts and MI250 SSH verification](../tests/reports/release-a/20260915_124433.md).

Release A target change: [Qwen122B spec and corpus preparation](../tests/reports/release-a/20260915_131432.md).

Release A FINISH resumption: [deterministic review, full regression, both adapters](../tests/reports/release-a/20260915_142237.md).

Release A transport settlement: [test plan for unknown outcomes, hop receipts and reconnection](../tests/plans/release-a-transport-reconciliation-20260915.md).
Release A A-BYTES: [test plan for exact integer bytes of native result, completion, receipt and edge](../tests/plans/release-a-bytes-20260915.md).

Release A A-BYTES B0: [Qwen122B physical result upper bound, actual LOAD, removal mutation](../tests/reports/release-a/20260915_211000.md).

Release A A-BYTES B1: [versioned LOAD profile, actual remaining capacity, Qwen122B LOAD/reclaim](../tests/reports/release-a/20260915_222346.md).

Release A A-BYTES B2: [completion group reservation before native, side-effect-free rejection, independent mutation](../tests/reports/release-a/20260915_232600.md).

Release A A-BYTES B3/B4: [separated retention lifetimes, actual boundary, independent mutation, local power incident](../tests/reports/release-a/20260915_235900.md).

Release A A-BYTES B5: [both-adapter regression, Qwen122B 3-host correct request, reclaim, remote preflight](../tests/reports/release-a/20260916_013500.md).

Node LOAD/UNLOAD lifecycle M0: [current call paths and NL01–NL14 ownership mapping](../tests/reports/node-load-lifecycle/20260916_014500.md).
M1: [deterministic execution plan](../tests/plans/node-load-lifecycle-m1-20260916.md).
Typed adapter completion: [remote baseline and independent mutation report](../tests/reports/node-load-lifecycle/20260916_015702.md).
M1 supervisor complete: [real TCP LOAD, UNLOAD, reclaim retention, workspace off/on, removal mutation](../tests/reports/node-load-lifecycle/20260916_023059.md).
M2: [deterministic execution plan](../tests/plans/node-load-lifecycle-m2-20260916.md), [llama.cpp/HF real worker, saturation, cleanup and independent mutation report](../tests/reports/node-load-lifecycle/20260916_032621.md).
M3: [deterministic execution plan for the Rust/HF OUTER migration and legacy CREATE/DELETE removal](../tests/plans/node-load-lifecycle-m3-20260916.md), [real Agent/HF child, workspace and independent mutation verification report](../tests/reports/node-load-lifecycle/20260916_041848.md).
M4: [deterministic execution plan for real llama.cpp/HF generation, cancellation, release and reload](../tests/plans/node-load-lifecycle-m4-20260916.md), [acceptance report: real models on both adapters, full regression, independent mutation](../tests/reports/node-load-lifecycle/20260916_064306.md).

Release A Qwen122B H0 v3: [sealed benchmark-spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v3.json) · [report: stronger H1 latency verdict, second INVALID, clean reclaim](../tests/reports/release-a/20260916_104300.md). [H0 v2 spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v2.json) and its [report](../tests/reports/release-a/20260916_093739.md), and [H0 v1 spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v1.json) and its [report](../tests/reports/release-a/20260916_072100.md), are historical evidence.

Release A Qwen122B H1 run 1: [64-request concurrent spec RED, failure recovery, closed-loop correction implementation](../tests/reports/release-a/20260916_084650.md).

Release A Qwen122B H1 run 2: [INVALID for missing judge SLO and absent terminal artifact; clean UNLOAD and task resource reclaim](../tests/reports/release-a/20260916_104300.md).

Current Release A execution order: [I0–I4 integrity-first test plan](../tests/plans/release-a-integrity-first-20260916.md), [H0 v6 seal](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v6.json), [execution contract v2](../test/benchmarks/cluster-inference/release-a/integrity-test-spec-qwen122b-i0-v2.json), [contract validator](../test/benchmarks/cluster-inference/release-a/validate-integrity-test-spec.py), [raw evidence builder](../test/benchmarks/cluster-inference/release-a/build-integrity-i0-evidence.py), [result judge](../test/benchmarks/cluster-inference/release-a/judge-integrity.py). Move on to P0–P3 performance improvements only after integrity is GREEN.

Integrity-first contract report: [14 real-hardware arms and contract/result verdict mutation](../tests/reports/release-a/20260916_131446.md).

I0 raw execution contract: [sequential short/medium/long runs within a single LOAD; request/batch/stage/GPU verdicts](../tests/reports/release-a/20260916_133038.md).

Qwen122B H0 v4: [absolute execution window, deterministic stress contract, raw GPU/resource evidence builder, seal of three remote binaries](../tests/reports/release-a/20260916_135000.md).\nQwen122B H0 v5: [deterministic observation barrier, absolute execution window, stress contract, raw GPU/resource evidence, seal of three remote binaries](../tests/reports/release-a/20260916_143000.md).

Release A I0 first real-hardware run: [correct answers 1/3 RED, reverse check against the single-host baseline, new LOADs blocked](../tests/reports/release-a/20260916_161600.md).

Release A I0 wrong-answer root-cause diagnosis: [pre-sealed observation and verdict plan](../tests/plans/release-a-quality-cause-20260916.md).

Release A I0 wrong-answer root cause confirmed: [6 separated remote diagnostics, exact fact extraction versus arithmetic failure, clean reclaim](../tests/reports/release-a/20260916_172000.md).

Release A OUTER wrong-answer verdict boundary: [JSON oracle binding, remote regression, product fix incomplete](../tests/reports/release-a/20260916_180500.md).

Past Qwen122B H0 v7/I0: [report: single-request correct answers 3/3 on 3 physical hosts, useful TPS correction, clean reclaim, performance measurement](../tests/reports/release-a/20260916_204300.md). [H0 v6 past report and correction](../tests/reports/release-a/20260916_195500.md).

Current Qwen122B H0 v8/I0: [report: single request 3/3 on the final source, clean reclaim, performance measurement](../tests/reports/release-a/20260916_213700.md). Full I1–I4 integrity remains.

Qwen122B H0 v8/I1 pre-run contract: [sealed spec](../test/benchmarks/cluster-inference/release-a/benchmark-spec-qwen122b-h0-v8.json) · [report: observation barrier, 64-request verdict path, before I0 re-verification](../tests/reports/release-a/20260916_210000.md). The I1 real-hardware run has not been run yet.

Release A transport settlement acceptance: [R1–R9, physical receipt recovery, final both-adapter run](../tests/reports/release-a/20260915_183158.md).

Release A Qwen122B A-PLAN: [native PLAN on 3 physical hosts, shared pool, rejection before deployment](../tests/reports/release-a/20260915_190631.md).

Release A Qwen122B A-LOAD: [actual allocation and reclaim on 3 physical hosts](../tests/reports/release-a/20260915_195106.md).
