# Execution API

The agent's adapter kind and optional feature is `hf-transformers`. The LOAD support advertisement and actual creation consume the same factory.
The public Rust types are `p4_hf_adapter::HfNodeAdapter`, `COMMAND` and `RESULT`.
An Agent-target `NODE_LOAD` creates a bounded node and bridge and completes through Python worker readiness.
`NODE_UNLOAD` returns success only after both the worker and the node route/owner have been removed.
Concrete packets, identity, epoch, abort and budgets follow the [integration contract](integration/README.md);
model planning and independent reference execution follow the [Qwen contract](models/qwen3_5_0_8b/README.md).

`p4hfadapter.models.qwen3_5_0_8b.planning.plan_loading(request, profiles)` returns
an executable `plan` together with per-resource summed capacity, objective value and input hash. Failures are distinguished as
`ValueError` for unmeasured/mismatched input and `LoadingInfeasible` for insufficient capacity within the given search space.
The CLI `profile` measures real stages, and `plan` generates a plan without the model package.
The optional `limits.prefill_chunk` in a generated plan is consumed by scenario admission and the Python worker.
If an existing plan lacks that field, the existing context cap is kept. For detailed inputs, see the
[automatic planning contract](models/qwen3_5_0_8b/README.md#automatic-loading-planner).
