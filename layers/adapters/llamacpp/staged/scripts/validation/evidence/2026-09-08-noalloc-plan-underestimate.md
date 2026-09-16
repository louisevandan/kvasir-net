# 2026-09-08 — Upstream defect: the no_alloc memory plan under-reported the compute buffer

Type: defect reproduction, root cause, fix and verification. This is not performance evidence.
Target pin `0eadefebd3f8f92a86d634a0e5b8fffc9dc792c0`; patch set after the fix `961bd89cd119`.
The current work order is owned by the [roadmap](../../../../../../../docs/distributed-batching-roadmap.md).

## Symptom

`StageRuntime::load` compares the plan from `inspect_stage_memory_with_initialized_backend` with the measurement
from `measure_stage_memory` after the actual load, using `same_stage_memory_allocation`. On 2026-09-07,
in a Qwen3.5-122B-A10B 2-stage offloading run, the tail failed this comparison and exited with exit 5
(`target/p4-4node/runs/20260907T105822Z-9888cd38`).

| Item | Plan | Actual |
| --- | ---: | ---: |
| host compute | 107,251,776 B | 142,951,040 B |
| host model / context | 40,186,750,976 / 138,936,320 B | same |
| device compute | 1,283,469,440 B | same |

In the same run the head stage had plan = actual, and the Ornith-1.0-35B offloading run that passed the same day also had plan = actual.

## Cause

The context reservation in `llama-context.cpp` reserves three times, in the order pp → tg → pp. The allocating path actually
performs all three reservations and then reads the final size with `ggml_backend_sched_get_buffer_size`, so the buffer has grown to fit
the largest of the three graphs. The `no_alloc` planning path, by contrast, passes the size output array **only to the first pp reservation**
(`model.hparams.no_alloc ? backend_buf_exp_size.data() : nullptr`). The following tg and second pp reservations
are called without `sizes`, so they only perform `ggml_backend_sched_split_graph` and do not measure sizes. As a result, for models where the tg graph
or the second pp reservation needs a larger buffer, the plan comes out smaller than the actual allocation.

This code is not one of our patches; it is the pin's upstream original (it exists in `git show HEAD:src/llama-context.cpp`).
The failing tail had `graph splits = 111 (with bs=512), 50 (with bs=1)`, so its pp and tg graphs differed,
while the passing Ornith tail had `graph splits = 82` for both. This difference separates the two cases.

## Reproduction

The same defect was reproduced in 27 seconds with a single process and a 4.3 GiB model. No harness, agent or second node is needed.
`p4_staged_server.exe` reads the start plan from stdin with a 4-byte LE length prefix.

```text
--model S:\models\unsloth\Qwen3.5-4B-MTP-GGUF\Qwen3.5-4B-Q8_0.gguf --memory-topology discrete
--layer-begin 16 --layer-end 32 --kv-layer-begin 16 --kv-layer-end 32 --n-seq-max 16
--spec-type none --kv-unified --batch-size 512 --ubatch-size 512 --ctx-size 32768
--n-gpu-layers 16 --device CUDA0 --flash-attn on --no-mmap --cache-type-k q8_0 --cache-type-v q8_0
```

| Source | MEMORY_PLAN host.compute | MEMORY_ACTUAL host.compute | Verdict |
| --- | ---: | ---: | --- |
| `0681d1c38` build (patch set `3cfc636181e4`) | 72,648,768 | 73,220,736 | mismatch → load rejected |
| Fixed build (patch set `961bd89cd119`) | 73,220,736 | 73,220,736 | match → load proceeds |

This model has `graph splits = 35 (with bs=512), 6~8 (with bs=1)`, so its two graphs differ.

## Fix

New patch `0025-noalloc-reserve-size-max.patch`, classified `upstream_fix` (upstream defect; `src/` allowed).
It passes the measurement array to all three reservations and keeps the element-wise maximum across reservations. On the allocating path (not `no_alloc`),
`measured` is null, so behavior does not change.

Verification:

- `validate-compat-manifest.mjs` valid, `validate-patch-classification.mjs` valid
  (upstream_fix 3 / stage_hook 18 / model_feature 4, 25 in total).
- The reproduction tree built by applying 0001~0025 to the pin in order is byte-identical to the edited tree.
- `prepare-pipeline-upstream.mjs` verified `0eadefebd3-961bd89cd119` with the new patch set.
- CUDA Release rebuild: CTest 15/15 passed. New exe `b66beffb479afa6db84cb7df697726a4e64e4b77a6a93343d48a545476767248`.
- The before/after comparison in the table above shows the failure when the fix is removed (the pre-fix build is RED).

## Remaining

- Keeping the maximum covers the requirement of each of the three graphs, but it does not model the fragmentation the allocator
  experiences when it reserves different graphs in succession. In the two cases above plan = actual, but this is not a general proof.
- This fix only removes the load rejection; it does not prove normal responses or throughput for 122B-class models.
