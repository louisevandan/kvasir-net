# Model load catalog

> Document status (2026-09-08): **OUTER operations material**. It records model load parameters and measured memory.
> P4 does not interpret plan strings; it only forwards them. Layer responsibilities are owned by the
> [isolation contract](../../../docs/layer-isolation-contract.md), and current goals and ordering by the
> [execution roadmap](../../../docs/distributed-batching-roadmap.md).

Model load parameters are not P4's knowledge; they are values supplied by OUTER. But which models can be loaded with which
split and offloading, and how much each memory component takes when they are, can only be known by measurement.
The layer composition of modern models cannot be computed with a simple formula. Even at the same parameter count, when hybrid attention,
recurrent state, MoE routing and NextN blocks are mixed in, the KV, compute and model buffers grow differently.
This folder keeps those measurements per model, so that a future web UI that controls P4 can look them up instead of computing them.

The fleet split is decided by [`tools/model-loading/src/placement-policy.ts`](../../../tools/model-loading/src/placement-policy.ts),
which reads this catalog's PLAN bytes and the per-workload service profiles. The policy does not use GPU names or
an even split by layer count. It first subtracts KV/runtime reservations, selects the minimum memory tier needed in the order GDDR, Mac unified,
GB10 unified, DDR offload, and then minimizes the predicted time of the slowest stage.
A device without a profile is not allowed as input to the final split.

## What is recorded

One `models/<model-id>.json` is one logical model.

| Section | Content |
| --- | --- |
| `identity` | Publisher, repository, shard file list, first shard path, byte count |
| `shape` | Architecture, block/trunk layer counts, NextN layers, expert count and active count, embedding width, training context, KV head/key/value lengths, full-attention interval, expert and non-expert bytes |
| `prompt` | EOS/BOS token ids and markers detected in the GGUF chat template |
| `runs[]` | One run = (strategy, context, stage count, seq count). For each stage, `model`, `context` and `compute` bytes per device/host and their sum, plus the raw buffer lines printed by the stage server |
| `runs[].totals` | Totals across all stages of the same run. This is the host RAM actually needed when two stages are loaded at the same time |
| `loaded_contexts` | List of contexts proven by a real load (`mode: "load"`) |

If `runs[].stages[].measured` is `actual`, the value was measured after loading the real weights; if `plan`, it is the value from
a planning pass that does not read weights. A run that obtained both records the comparison result in `plan_equals_actual`.

## Why the planning pass can be trusted

`--inspect-memory-plan` builds only the context with a `no_alloc` model and computes the memory breakdown. Because it does not read
the weights, even a 200 GiB model finishes in seconds. Until 2026-09-08 this path under-reported the tail stage's host compute
([evidence](../../../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-08-noalloc-plan-underestimate.md)),
which `0025-noalloc-reserve-size-max.patch` fixed. So the plan value is used as the catalog's default measurement,
while a real load is also run for each model and checked with `plan_equals_actual`.

## Reproduction

```bash
node test/benchmarks/model-catalog/inventory.mjs --root "S:\\models"
node test/benchmarks/model-catalog/build-jobs.mjs --mode plan --contexts 4096,32768,102400,262144 --out target/model-catalog/plan-jobs.json
node test/benchmarks/model-catalog/remote-probe.mjs run --jobs target/model-catalog/plan-jobs.json --tag plan1
node test/benchmarks/model-catalog/remote-probe.mjs status --tag plan1
node test/benchmarks/model-catalog/remote-probe.mjs fetch --tag plan1 --out target/model-catalog/plan1.jsonl
node test/benchmarks/model-catalog/render.mjs --results target/model-catalog/plan1.jsonl --jobs target/model-catalog/plan-jobs.json --out test/benchmarks/model-catalog/models
```

Run `inventory.mjs` on a machine that can see the model share. `remote-probe.mjs` controls the remote over SSH,
but the probe itself runs as an **interactive scheduled task**. A mapped drive is visible only to the logged-on user's session,
so an SSH command cannot open the model directly. This is the same method the harness agent uses.

## Load strategies

| Strategy | Content | When |
| --- | --- | --- |
| `vram_only` | Keeps all owned layers on the device. Only layers it does not own are excluded via a CPU pattern | When weights + KV + compute per stage fit on the device |
| `expert_cpu` | Keeps MoE routed expert weights on the host and computes them on CPU. Router, attention, norm and KV stay on the device | When a MoE model does not fit on the device |
| `dense_ffn_cpu` | Moves a dense model's FFN weights to the host. Attention stays on the device | When a dense model does not fit on the device |

The `vram_only`/`expert_cpu`/`dense_ffn_cpu` classification is determined by the actual buffer
placement reported in the stage log, not by option names. The `resource_tier` verdict of verification convention H0 uses the same principle.

## The two measurement machines

| Machine | GPU | RAM | Role |
| --- | --- | --- | --- |
| `M42-SERVER2` | RTX 3090 24 GiB ×2 | 256 GiB | Default probe host. Driven by `remote-probe.mjs` as an interactive scheduled task |
| `hikaTR` | RTX 3090 24 GiB + RTX 4080 16 GiB | 256 GiB | Fallback when the remote is logged out. Driven directly by `local-probe.mjs` |

The cards differ, so values from the two machines are not mixed in the same column. Each run record's `machine` field tells them apart.
Because the 4080 has 16 GiB, the local machine hits the context limit first.

## Comparison with another placement strategy

The same model can also be loaded by a single llama.cpp process instead of a P4 stage split. On 2026-09-07 the user
loaded Hy3 that way on `hikaTR` with the configuration below. It is recorded because it shows that the same model on the same
hardware can have a completely different memory placement.

```text
-m C:\hy3\Hy3-Q5_K_S-00001-of-00006.gguf -c 100000 -ngl auto -t 24 -b 2048 -ub 512 -np 1
-fa on -ctk q8_0 -ctv q8_0 --load-mode none --kv-offload --host 127.0.0.1 --port 8088
--split-mode layer --device CUDA0,CUDA1 --fit on --fit-device-limit 10500,23000
```

One process uses both cards with `--split-mode layer`, and `--fit` spills the rest to CPU to fit the device limits (about 10.5 GB and 23 GB).
Loading the same 100k context with a P4 2-stage split + `expert_cpu` used a GPU
total of 24.52 GiB (12.06/12.46 per card) and 184.5 GiB host. Different placement strategies also change which side is the bottleneck,
so whenever a value is cited, the strategy is stated with it.

`--fit-device-limit` is not in the llama.cpp pin that P4 fixes. The pin has `--fit`, `--fit-print` and
`--fit-target` (per-device headroom margin). The remaining options are not stage-specific, so they are carried as is in the startup plan
and passed to the llama.cpp parser.

## Cautions

- Use `--no-mmap` on shared drives. With mmap over SMB, loading effectively stalls on page faults.
- For a model with NextN (MTP) blocks, the trunk layer count is `block_count - nextn_predict_layers`. Cutting beyond this value
  trips the layer window assertion in `llama-graph.cpp` and kills the stage.
- gemma-4 shares KV from layer 13 on, so do not cut a stage inside that region.
- `fits_current_free` is a verdict for **that one stage**. When loading several stages at once, judge host RAM by
  `runs[].totals.host_required_bytes`.
- Stages of large models hold their weights in CUDA pinned host memory (Hy3 uses about 92 GiB per stage).
  It is not returned immediately even when the process dies, so starting the next load right away fails with
  `ggml_cuda_host_malloc: ... resource already mapped`. The probe waits until free memory recovers before the next job.
  The same wait is needed when swapping models in operation.
- For the same reason, a stage that just exited may still hold the model file handle. A file open failure
  (`Permission denied`) may be this delay rather than file corruption, so distinguish it by retrying.
- This folder only records loadability and measured memory; it does not accept response quality or throughput.
