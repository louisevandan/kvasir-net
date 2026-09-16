# Verification entry points

Run from the P4 root. The standard Python fixture takes its interpreter from `HF_TEST_PYTHON`.

```powershell
cargo test --locked --workspace --no-fail-fast
cargo test --locked --workspace --no-fail-fast --features hf-transformers
python layers/adapters/hf/scripts/testing/run.py
python layers/adapters/hf/scripts/verification/documents/run.py
python layers/adapters/hf/scripts/verification/package_graph/run.py --output target/hf/package-graph
python layers/adapters/hf/scripts/verification/rust_mutation/run.py target/hf/rust-mutations
```

HF Rust tests are included in both workspace commands. Turning the feature off disables registration in the agent; it does not hide the workspace member's tests.
Python model/cache/epoch/mutation verification requires the pinned torch/Transformers environment.
`scripts/verification/lifecycle/` verifies fixture errors and reclaim through the real broker,
`event_qwen/` and `event_matrix/` verify the real model, state, normal responses and replacement,
and `distributed_recovery/` verifies return-path disconnection and re-acceptance across two physical hosts.
Use a new output directory and generation. Do not count past reports as run evidence for a new commit.
Migration tests follow the [plan](../tests/plans/migration-20260914.md), and the verdict follows the [migration record](migration/README.md).

The automatic loading planner's functions, CLI and independent exhaustive search are part of the default Python suite,
and `npm run test:model-loading` at the P4 root runs both the llama.cpp TS suite and the HF Python suite.
Real CPU model consumption and removal mutations run in the pinned environment.

```powershell
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B layers/adapters/hf/scripts/verification/loading_planner/run.py --output layers/adapters/hf/target/loading-check/new-run
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B layers/adapters/hf/scripts/verification/loading_planner_shape/run.py
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B layers/adapters/hf/scripts/verification/loading_planner_mutation/run.py --output layers/adapters/hf/target/loading-mutations/new-run
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B layers/adapters/hf/scripts/verification/loading_planner_event/run.py --agent <HF-enabled-agent.exe> --plan <generated-plan.json> --scenario <scenario.json> --output layers/adapters/hf/target/loading-event/new-run
```

Role-specific source, tests and verification runners live inside HF, and loading-plan outputs go into HF's own `target/`.
The existing rule that other HF verification tools write to `target/hf/` under the P4 root still applies.
