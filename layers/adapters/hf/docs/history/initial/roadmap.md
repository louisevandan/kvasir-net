> Historical record: requirements, status and measurements from the time of the standalone repository. The current layout and usage follow the [HF guide](../../../README.md). The complete original is in the Git bundle preserved during the migration.

> 2026-09-14: P4 integration is being implemented under the user's §0 instruction. For the current status of the earlier unconnected/read-only descriptions, the [integration specification](../../integration/README.md) and its acceptance report take precedence.

# Execution roadmap

Status: the sole owner of this project's current phase, next action and phase completion conditions.

## Current status

2026-09-13: Python IPC framing and a dedicated local partition script for the user-specified Qwen3.5-0.8B are implemented.
They were compared against the official full model on the real weights across 8 combinations and 47 steps. There is no Rust bridge yet.
For the initialization result, see the [initialization record](bootstrap-evidence.md) and the Git log.
The P4 reference HEAD is `d122125bafeaa6d32790761669f1bfa5868d8078`; re-check it in the next session.

| Phase | Work | Exit condition | Status |
| --- | --- | --- | --- |
| S0 | Standalone folder/Git/plan/handoff | Document/link check, no P4 writes, concurrent changes recorded, initial commit | Done |
| S1 | Investigation of the first model, devices and quantization kernels; manifest | Pin the exact revision, supported combinations, legal cuts and quality/SLO criteria | Qwen dense combination pinned; quantization and product SLO unfinished |
| S2 | High-precision/quantized reference and partial loading | REF-01, Q-01~04; real compressed execution and quality | Qwen dense reference and assigned-weight loading passed locally; quantization not run |
| S3 | Per-model stage execution | MOD-01~02; prefill/decode/cache parity | Local process partition and repeated logits passed; element-wise cache comparison unfinished |
| S4 | Standalone Rust bridge, Python worker, local host | BR/WIRE/LIFE/SET real consumption paths and mutations | Qwen worker and controller implemented; Rust/retained consumption not implemented |
| S5 | PP execution on real physical computers, continuous batching | DIST/BATCH/WAVE; HET if heterogeneity is claimed | Not started |
| S6 | Approved P4 integration, compilation and deployment verification | INT-01, existing adapter regression, real product consumption waves | Future separate integration task |

This table lists technical verification phases; no phase is called an independent product release.
Results from S5's separate test host are not reported as support in unmodified stock P4.
Final product completion requires the real P4 integrated consumption path and evidence for the specified target model and physical fleet.

## First next task

The work breakdown and deliverables of the initial development follow the [development execution plan](development-plan.md).
Evidence for the preceding IPC implementation is in the [framing run report](../../../tests/reports/framing/20260913_174516.md).
It does not mean that all of WIRE-01, the model worker, the Rust bridge or the P4 consumption path passed.

The entry point of the current implementation is the [Qwen model document](../../models/qwen3_5_0_8b/README.md),
and the run scope and failures follow the [Qwen run report](../../../tests/reports/qwen3_5_0_8b/20260913_220709.md).
The BF16 heterogeneous split failed by exceeding the threshold; FP32 is a separate passing combination.
Depending on the next scope, verify BF16 per-operation numeric differences, cache parity or the standalone Rust bridge.
Before starting quantization, pin the intersection of vendor code, artifacts and actual execution kernels, and the module coverage.

S2 does not force high-precision GPU loading of an entire very large model from the first run.
First compute and verify the offload/resources the reference run requires and the method for sequential quantization.
Small models can serve as an auxiliary tool for running bridge/codec counterexamples quickly, but they are kept separate from target-model acceptance.

## Phase operations

Commit only this repository, at restorable implementation/regression pin points. A WIP commit records the exact failure and the next task.
At the end of each phase, update HEAD, changed files, actual commands/test IDs/results, failures and items not run, artifact locations, and the first next action.
If the verified source differs from the final source, do not mark the phase done.
Without the devices or access rights, the affected real-hardware run is BLOCKED and kept distinct from the local safety work that is still possible.

## Follow-up options

After the first supported combination is complete, add other models, devices and quantization recipes as the same verification unit.
Other models can be added as separate dedicated Python implementations; generalizing into a common model processor is not a goal.
Per-node recipe mixing, KV/transport quantization, same-host TP, automatic placement, KV persistence, MTP and external authentication/recovery
are organized as separate scope according to the purpose of use and the measurement results. They are not items approved for automatic implementation in the current conversation.
