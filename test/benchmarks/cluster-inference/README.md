Compose distributed inference experiments from model, placement, policy, workload and immutable runtime evidence on main.

| Purpose | File |
| --- | --- |
| Current work and promotion gates | [Roadmap](../../../docs/distributed-batching-roadmap.md#v11-plan) |
| Runtime acceptance | [Verification](../../../docs/distributed-batching-verification.md#v11-gates) |
| Composer and CLI | [compose.mjs](compose.mjs) |
| Model templates | [Hy3](models/hy3-no-think.json), [Step3.7](models/step37-no-think.json) |
| Experimental policies | [decode2/open8](policies/decode2-open8.json), [decode4/min4/CPU4](policies/decode4-min4-cpu4.json) |
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
