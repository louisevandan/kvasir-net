> Current run location: P4 root. The model-specific contract below still applies; for current commands follow the [usage guide](../../usage.md). When using this document's HF-relative paths, run from `layers/adapters/hf`. The environment/model cache is in P4 root `.cache/hf`.

> This describes the v1 standalone controller. The new v2 P4 path follows the [integration specification](../../integration/README.md).

# Qwen3.5-0.8B node-split execution

<a id="automatic-loading-planner"></a>

## Automatic loading plan

`planning.plan_loading` and the CLI `plan` take this model's real stage profiles and generate the existing v1 plan JSON.
The model format is safetensors, and only fixed revision, dense FP32 and CPU/CUDA are planned automatically.
It does not parse GGUF, apply Mac/GB10 tier priorities, or apply the same KV formula to every layer.

An example request is [loading_cpu/request.json](../../../plans/qwen3_5_0_8b/loading_cpu/request.json).
The 16 GiB in the example is a test budget. Replace it with the investigated real available capacity and leave an OS/allocator/unobserved peak reserve.

| Input | Contract |
| --- | --- |
| `model_id`, `revision`, `dtype`, `quantization`, `limits` | Must match the current model run contract. limits are context·max_requests·max_new_tokens |
| `prefill_chunk` | Maximum measured chunk. Passed as `limits.prefill_chunk` of the generated plan so that scenario/worker reject before running |
| `cuts` | Ascending boundaries including 0 and 24. Every front/back boundary combination must be measured per device |
| `hosts` | host ID, available_bytes, reserve_bytes. Process RAM of multiple stages is summed |
| `devices` | ID, host, cpu/cuda:N, available_bytes, reserve_bytes, enabled. One physical host/device has one ID |
| `slots` | Order of node_id/device_id. Stage positions that may be skipped; the same device may be referenced more than once |
| `minimum_hosts` | Lower bound on the number of host IDs selected. IDs/profiles alone do not certify the number of physical machines |

From P4 root, use a new output folder.

```powershell
.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py profile --request layers/adapters/hf/plans/qwen3_5_0_8b/loading_cpu/request.json --output layers/adapters/hf/target/loading-profile/new-run --timeout 600
python -B layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py plan --request layers/adapters/hf/plans/qwen3_5_0_8b/loading_cpu/request.json --profiles layers/adapters/hf/target/loading-profile/new-run/profiles.json --output layers/adapters/hf/target/loading-plan/new-run
python -B layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py inspect --plan layers/adapters/hf/target/loading-plan/new-run/plan.json
```

`profile --host <id>` explicitly maps the current local machine to that ID. It does not connect to remote hosts.
Merge the profiles.json produced on each machine with `plan --profiles <file1> <file2>`. The model path can be set with `--model-dir`.
`plan` does not read the checkpoint's large tensors or torch; it checks the profiles' binding to the fixed model, source, runtime and workload.
Missing or stale profiles are rejected, distinct from insufficient capacity. Re-measure profiles after model/worker changes.

Each candidate is loaded with `load_stage` in a fresh Python process. It measures the actual weight bytes selected by the safetensors index,
the KV/conv/recurrent cache of max_requests requests alive at the same time up to the full context, the process peak RSS, and the
CUDA allocator peak reserved bytes. At the end, an actual release reclaims all active state.
Duplication of the tied embedding across the first/last stages and the alias in a single stage are reflected exactly as the loader executed them.

The current HF controller waits for stage returns in order. The objective is therefore **total stage service → maximum stage service → number of stages**.
Multiple stages on the same GPU accumulate against device capacity, and multiple GPU/CPU stages on the same host accumulate against host RAM.
The Pareto search is exact within the slot subsets of the given order and the cuts. Exceeding the state-count limit is an undecided error and
is not turned into insufficient capacity. Cut/device orders outside the candidates, IPC/network cost and shared-GPU contention are outside the optimality claim.
Measurement uses synthetic tokens/hidden states and cold stage execution; it does not guarantee real TPS, quality, or memory limits for every input.

The outputs are `plan.json`, `assessment.json`, `request.json` and `profiles.json`. The existing `run`/`verify` and the P4 event
pipeline consume the plan as is. Standalone execution uses `host=local`; P4 remote deployment addresses and bundles are covered by the existing separate specification.
The plan, tests, profile generator, verification runner and this feature's ignored `target/` results all live in the HF subfolder.

2026-09-15 local verification: confirmed Python 42/42, existing TS 34/34, 180 independent exhaustive-search cases, and removal mutations 7/7.
In 3 real CPU measurement segments, confirmed an added split tied weight of 1,017,118,720 bytes and a live cache of 44,433,408 bytes.
The generated 2-stage CPU plan passed a 6-step logits/greedy comparison against the official model; the two responses were `4` and `서울` (English: "Seoul"), both ending with EOS.
The assertion failure in which the initial verification runner compared an empty set with an empty dict is preserved in `real-cpu/`; the new run with the type comparison fixed is
recorded in `layers/adapters/hf/target/loading-planner-20260915/real-cpu-final/summary.json`. Model expected values and tolerances were not changed.
Actual StageSessions rejection of an oversized chunk was confirmed with 0 forward, active and retired effects. This is not multi-physical-host acceptance, a BF16 fix, or H0–H7 promotion.
The same generated plan was also consumed through NODE_LOAD→HF worker→release/NODE_UNLOAD on a task-owned localhost P4 agent.
`target/loading-planner-20260915/p4-event/summary.json` records 6 logits/greedy comparisons, 12 cache comparisons and agent shutdown.
The HF-enabled `cargo test --locked --workspace --no-fail-fast --features hf-transformers`, run in the separate
`--target-dir layers/adapters/hf/target/loading-planner-20260915/cargo`, gave 1460 passed/0 failed/7 ignored,
61 summaries and exit 0. The first attempt in the default target failed the build with exit 101 because removing the in-use `target/debug/p4-agent.exe`
was refused; the running agent was not stopped. The logs are kept separately as `workspace-hf.log` and `workspace-isolated.log`.

The selected model is `Qwen/Qwen3.5-0.8B`, revision `2fc06364715b967f1860aea9cf38778875588b17`.
Prefill and decode of text input run as **real local Python processes per node**.
Each node owns only the weights of its assigned layers and the per-request cache. Transfer between nodes is the bounded IPC of the standalone test controller.
P4 connection, remote host execution, images/video, MTP, quantization and physical continuous batching are not included.

## Running

Run from PowerShell at the repository root. The verified environment is Windows/CPython 3.13.15,
PyTorch `2.14.0+cu130`, Transformers `5.17.0`, safetensors `0.8.0`.
The runtime package list is pinned in the [environment lock](../../../environments/qwen3_5_0_8b/requirements.lock).

```powershell
uv venv ../../../.cache/hf/environments/qwen3_5_0_8b --python 3.13.15
uv pip install --python ../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -r environments/qwen3_5_0_8b/requirements.lock --index https://download.pytorch.org/whl/cu130 --default-index https://pypi.org/simple
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/models/qwen3_5_0_8b/preparation/run.py

python -B scripts/models/qwen3_5_0_8b/cli/run.py inspect --plan plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/models/qwen3_5_0_8b/cli/run.py run --plan plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json --scenario scenarios/qwen3_5_0_8b/short/scenario.json --output ../../../target/hf/qwen/my-run
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/models/qwen3_5_0_8b/cli/run.py verify --plan plans/qwen3_5_0_8b/balanced_two_gpu_fp32/plan.json --scenario scenarios/qwen3_5_0_8b/interleaved_cancel/scenario.json --output ../../../target/hf/qwen/my-verify
```

`inspect` checks a plan without installing or downloading the model or accessing a GPU. `run` performs generation, and `verify`
also feeds the same input tokens to the official full model and compares logits and greedy tokens at every step.
`--output` must be a new folder; existing results are never overwritten. Specify an already obtained checkpoint with `--model-dir`.
Before running, all files are checked against the [sealed hashes](../../../manifests/qwen3_5_0_8b/artifact/identity.json).

## Node plans

`layers: [start, end]` is a 0-based **end-exclusive range**. In order, the ranges must cover `[0, 24)` with no gaps or overlaps.
`node_id` is unique and `device` is `cpu` or `cuda:N`. The first stage owns the embedding and the last stage owns norm/head.
The head's tied weight is taken from the original embedding tensor; if the two are on different stages it is explicitly duplicated and counted for capacity.
Execution allows only `host: "local"`. Placement plans with other host values can go as far as `inspect`, and are not passed off as remote execution.

| Plan directory | Split | Precision · device |
| --- | --- | --- |
| `single_gpu/` | 0–24 | BF16, cuda:0 |
| `balanced_two_gpu_fp32/` | 0–12 / 12–24 | FP32, cuda:0 / cuda:1 |
| `uneven_three_stage_fp32/` | 0–5 / 5–19 / 19–24 | FP32, cuda:0 / cuda:1 / cuda:0 |
| `attention_boundaries_fp32/` | 0–3 / 3–4 / 4–24 | FP32, includes a DeltaNet-only stage and an attention-only stage |
| `cpu_gpu/` | 0–4 / 4–24 | FP32, CPU / cuda:0 |
| `single_cpu/` | 0–24 | FP32, CPU |

Every plan file is `plans/qwen3_5_0_8b/<name>/plan.json`.
`balanced_two_gpu/`, `uneven_three_stage/` and `attention_boundaries/` also keep the BF16 experiment plans.
**The BF16 even split across RTX 4080 + 3090 exceeded the logits tolerance relative to the single-4080 baseline.**
This BF16 combination is not treated as a verified configuration. A BF16 split within the same 4080 had an error of 0, and
FP32 is a separate recipe with its own run results. The baseline, inputs and precision of the BF16 failure were not changed after the fact to reclassify it as a pass.

Plan limits are context ≤4096 and executing requests ≤16. The provided examples are fixed within context 2048, 8 requests and 64 output tokens.
These limits are not the model's official maximum context or a guarantee of fitting in total GPU memory. CUDA numbers follow PyTorch enumeration order;
on this machine cuda:0=RTX 4080 and cuda:1=RTX 3090 were observed.

## Scenarios

| Scenario directory | Processing |
| --- | --- |
| `short/` | generates arithmetic and Korean-language requests in order, releasing each and reusing the nodes |
| `chunked_prefill/` | prefills a long input 32 tokens at a time, then decodes repeatedly |
| `interleaved_cancel/` | alternates requests of different lengths/chunks round robin; one is cancelled and released after 2 tokens |

The files are `scenarios/qwen3_5_0_8b/<name>/scenario.json`. Each request specifies prompt, max_new_tokens,
prefill_chunk and cancel_after. Cancellation happens at step boundaries and does not mean immediate interruption of a running CUDA kernel.
Round robin consumes the state of several independent requests in turn. A physical compute batch is always one request, with no padding.
EOS, output limit and cancellation are distinguished, and after completion removal of that state is confirmed on every stage.

## Model-specific implementation and audit

The text part of the official config has 24 layers and hidden size 1024, repeating 3 Gated DeltaNet layers and 1 full attention layer.
The output head is tied to the embedding. The sources for the vendor's architecture are the
[pinned config](https://huggingface.co/Qwen/Qwen3.5-0.8B/blob/2fc06364715b967f1860aea9cf38778875588b17/config.json) and the
[model card](https://huggingface.co/Qwen/Qwen3.5-0.8B).

The actual call and cache APIs are bound to the [Transformers v5.17.0 implementation](https://github.com/huggingface/transformers/blob/595ff117c8412ec01262084058276e6b27a857d9/src/transformers/models/qwen3_5/modeling_qwen3_5.py) and the
[cache code](https://github.com/huggingface/transformers/blob/595ff117c8412ec01262084058276e6b27a857d9/src/transformers/cache_utils.py).
`forward/` calls these official modules and does no generic layer introspection.

| Dedicated role folder | Owns |
| --- | --- |
| `identity/`, `configuration/` | fixed model identity, and node plan validation, respectively |
| `preparation/`, `evidence/` | obtaining the checkpoint, and file/source/run hash verification, respectively |
| `loading/` | after creating meta modules, selectively loads only the assigned weights via the index |
| `forward/` | partial forward of concrete Qwen layers; separates global weight names from local cache indices |
| `state/` | stage-local DynamicCache, per-request position/issue, rejection, release and late-request prevention |
| `boundary/` | JSON metadata + safetensors tensor payload; no pickle |
| `worker/` | local process entry point for one node |
| `processes/`, `routing/` | child process/IPC lifetime, and node traversal for approved steps, respectively |
| `scenarios/`, `execution/` | request admission, and prefill/decode schedule execution, respectively |
| `reference/`, `cli/` | comparison with the official full model, and the user command entry point, respectively |

Partial loading does not allocate other layers or the vision module first. Each node report records the actual tensor names and bytes.
The cache holds only that stage's layers, and a stage without full attention uses an explicit position instead of estimating it from KV length.
Only the last stage returns logits, and only for the last input position. Full logits for earlier prefill positions are not sent over IPC.

## Verification and limits

```powershell
python -B scripts/testing/run.py
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/verification/qwen_state/run.py
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/verification/qwen3_5_0_8b/run.py
../../../.cache/hf/environments/qwen3_5_0_8b/Scripts/python.exe -B scripts/verification/qwen_mutation/run.py
```

The fixed verdict is logits `atol=0.125, rtol=0.01` plus a greedy token match at every compared step.
This checks execution consistency for the selected scenarios; it is not a general quality/SLO certification. Detailed evidence
is in the [test plan](../../../tests/plans/qwen3_5_0_8b-20260913.md) and the [run report](../../../tests/reports/qwen3_5_0_8b/20260913_220709.md).

Attention is eager, and DeltaNet/causal convolution use the installed PyTorch fallback. No performance claim is made for compressed or optimized
kernels. Exceeding the request count or context, unsupported models/quantization, duplicate issues and malformed tensors are rejected.
Worker errors/timeouts are not retried; that run is marked failed, and only the children it started are cleaned up.
A recoverable distributed ledger, durable exactly-once, and an authenticated remote service are not implemented.

The result folder contains the fixed plan/scenario, `summary.json` and per-node stderr.
The summary holds the full output text, tokens, stop reason, release, per-step state bytes/positions, actual PIDs/devices,
checkpoint/source/upstream code hashes, package versions, and the first error and cleanup errors. Do not read the performance figures as a general benchmark.
