# HF Qwen loading planner reproduction plan

Created 2026-09-15 KST, while taking over the implementation and measurements of a concurrent development task.
The goal is to consume CPU/CUDA stage measurements and shared capacity to produce a plan that the existing Qwen runner reads,
and to enforce the prefill limit promised in the plan at the actual point where state is consumed as well.

The environment is the P4 root, the pinned Qwen3.5-0.8B checkpoint and `.cache/hf/environments/qwen3_5_0_8b`.
New run results are recorded under the root `target/hf/<run-name>`. The source and binaries of the running 550B arm are not changed,
and Rust uses a separate `--target-dir`. The scripts paths below are relative to `layers/adapters/hf`.

1. `npm run test:model-loading`: both the existing llama.cpp TS tests and the HF Python tests must pass.
   This includes the 180 independent exhaustive-search cases in `tests/models/qwen3_5_0_8b/planning/test_planning.py`.
2. With the pinned-environment Python, `scripts/verification/loading_planner_shape/run.py` checks that the real `StageSessions`
   rejects over-limit input and preserves forward, active and retired state.
3. `scripts/verification/loading_planner_mutation/run.py --output <new folder>` detects, in an independent copy, the removal of
   reserve/disabled/shared-capacity/serial-objective/source-binding and of the two consumption limits.
4. Run `scripts/verification/loading_planner/run.py --output <new folder>` with the pinned-environment Python.
   It checks the CPU 0–12/12–24/0–24 measurements, the tied-weight duplicate size, cache totals, active0 after release,
   and the per-step logits/greedy tokens and arithmetic and Korean EOS of the auto-generated plan against the existing verify.
5. `scripts/verification/loading_planner_event/run.py --agent <separate agent> --plan <generated plan>
   --scenario <same scenario> --output <new folder>` checks real P4 event logits/cache,
   UNLOAD/DELETE, and termination of the owned agent.
6. Sum the final exit and all summaries of `cargo test --workspace --no-fail-fast --features hf-transformers --target-dir <separate folder>`,
   and run docs-lint.

Preserve standard output, the first failure and cleanup errors, source and bundle hashes, commands and exits, the real-model comparison and PIDs.
Do not approve cut, device or workload sizes by extrapolation when they were not measured. Remote physical execution and SLO are a separate acceptance.
