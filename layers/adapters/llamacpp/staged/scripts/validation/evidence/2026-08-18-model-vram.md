# Model and VRAM validation

> Document status (2026-09-06): **Date- and environment-scoped evidence**. These are observations for the date, commit, model and topology stated in the body. They are not evidence that the current implementation or any other distributed environment is complete.
> Current goals, status and ordering follow the [execution roadmap](../../../../../../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../../../../../../docs/document-map.md).

## Scope

The source model root is `S:\models`. The policy budgets are 23 GiB for an
RTX 3090 and 11 GiB for an RTX 4080. GGUF planning numbers exclude KV,
compute buffers, CUDA context, and allocator overhead; runtime log buffer
sizes are the authoritative residency evidence.

## Inventory

The recursive inventory found 100 GGUF files, 86 model-body files after
excluding `mmproj`, 27 model directories, and 11 multi-shard sets. The
Nemotron 120B set is incomplete because one expected shard is absent.

## Real staged tests

Using the copied CUDA artifact
`.cache/staged-server-cuda-real-20260818/Release/p4_staged_server.exe`:

- Qwen3.5 4B MTP: CUDA stage load/HELLO/unload and two-sequence cut-set test
  passed.
- Qwen3.5 9B MTP: CUDA stage load/HELLO/unload passed; observed stage model
  buffers were approximately 3101 MiB and 2305 MiB.
- Qwen3.8 27B Q6: CPU partial stages passed. CUDA tail stage `[49,64)`
  passed with `--device CUDA0 --flash-attn 0` and a CUDA model buffer of
  5555.92 MiB. CUDA head stage `[0,49)` also passed when pinned to the
  physical RTX 3090 (`CUDA_VISIBLE_DEVICES=1` on this host), with approximately
  14918.81 MiB of weights and `READY`/`UNLOAD` exit 0. The earlier failure was
  an incorrect attempt to place the 14.9 GiB head on the 4080-sized device.
- Gemma 4 12B: CUDA head `[0,32)` on the 3090 and tail `[32,48)` on the 4080
  both passed `READY`/`UNLOAD` with `--device CUDA0 --flash-attn 0`.

## Planner example

For Qwen3.8 27B Q6 (`22.884 GB`), the tensor-byte planner reports a fitting
split with 3090 `[0,49)` at 14.5691 GiB and 4080 `[49,64)` at 6.7334 GiB when
the boundary is assigned to the last stage. This is a placement lower bound,
not a runtime success claim.

## Explicit non-claims

MTP/speculative parsing is validated, but execution remains deliberately
disabled (`mtp_execution=0`, `speculative_execution=0`). MoE and large
multi-shard CUDA execution still need dedicated runtime tests. The reported
GPU mapping is host-specific; deployment must discover and bind the physical
device per node rather than assume the `CUDA_VISIBLE_DEVICES` index.
