# HF adapter assembly

The P4 workspace member `layers/adapters/hf/adapter` is assembled into the agent through the `hf-transformers` feature.
The feature is off by default. The existing llama.cpp is provided in both builds, and creation and INSPECT consume the same factory list.
Model kinds and tensor/state semantics live in the HF Python; the common core does not interpret them.

| Purpose | Document |
| --- | --- |
| HF code, API, environments, tests | [HF guide](../layers/adapters/hf/README.md) |
| Wire, budget, deployment, restoration | [Integration contract](../layers/adapters/hf/docs/integration/README.md) |
| Current migration status, mapping to the original | [Migration record](../layers/adapters/hf/docs/migration/README.md) |
| Isolation | [Layer contract](layer-isolation-contract.md#external-hf-boundary) |
| Earlier acceptance evidence | [Past report](../layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md) |

```powershell
cargo build --locked -p p4-agent -p p4-event-drive --features hf-transformers
cargo test --locked --workspace --no-fail-fast --features hf-transformers
cargo test --locked --workspace --no-fail-fast
```

The root Cargo.lock is the only reference for how HF is consumed. Even with the feature off, no separate HF checkout is needed.
HF Rust tests are part of the workspace and require a standard Python fixture. This is separate from installing the model packages.
Earlier two-host FP32/cancellation/replacement/reclaim results are evidence for the source of that time. Results for the new migrated source follow the migration report.
The BF16 failure and the unfinished very large H0–H7/SLO work still stand.
