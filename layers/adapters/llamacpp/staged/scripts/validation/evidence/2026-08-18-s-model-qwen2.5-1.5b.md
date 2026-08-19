# S:\\models GGUF preflight evidence

Observed 2026-08-18 KST with the validation scripts in this directory.

## Input and metadata

Command:

```text
node layer-window-memory-report.mjs --models-dir S:\\models --model-name Qwen2.5-1.5B-Instruct-Q8_0 --build-dir .cache\\staged-server-llama
```

| Item | Observed |
| --- | --- |
| GGUF | `S:\\models\\Qwen2.5-1.5B-Instruct-Q8_0.gguf` |
| Shards | 1; 1,646,573,312 bytes / 1.5335 GiB |
| GGUF | v3; 38 metadata entries; 338 tensors |
| Architecture | `qwen2`; 28 layers; embedding 1536; 0 experts |
| Context metadata | 32768 |
| Transformer layer tensors | 1.2970 GiB |
| Boundary tensors | 0.2309 GiB |
| Total tensor bytes | 1.5279 GiB |

## 23/11 GiB tensor-byte windows

The report was run with `--budget 3090=23 --budget 4080=11` for each boundary
ownership mode. All three are `fits_tensor_bytes`; this is not a VRAM pass.

| Boundary ownership | 3090 window | 4080 window | Required tensor bytes |
| --- | --- | --- | --- |
| none | `[0,19)` | `[19,28)` | 0.8801 / 0.4169 GiB |
| first | `[0,18)` | `[18,28)` | 1.0647 / 0.4632 GiB |
| last | `[0,22)` | `[22,28)` | 1.0191 / 0.5089 GiB |

The calculation excludes KV cache, compute buffers, CUDA context, allocator
fragmentation, and runtime overhead. Boundary ownership remains an explicit
input because the GGUF bytes alone do not establish which stage owns those
tensors.

## Existing runtime capability

The inspected `.cache\\staged-server-llama` CMake cache reports:

- `P4_STAGED_BUILD_LLAMA=ON`
- prepared source present at `.cache/llama-pipeline-upstream/3e3a7a416`
- `GGML_CUDA=OFF`
- `p4_staged_server.exe` present
- source contains the staged llama target, `--validate-plan`, model load,
  layer-window hooks, and unload path

## Short smoke results

`smoke-stage-server.mjs` ran with `--n-gpu-layers 0`, a 45-second hard timeout,
and the existing cached executable. Full `[0,28)`, `[0,19)`, and `[19,28)`
load→HELLO→UNLOAD runs passed with exit code 0. The split-window runs emitted
matching `linkcpp graph stage: layers=[0,19)` and `layers=[19,28)` records.

These are CPU staged-runtime load/unload and graph-window observations. They do
not prove CUDA support, 3090/4080 VRAM residency, HOP logits equality, or
production inference capacity.
