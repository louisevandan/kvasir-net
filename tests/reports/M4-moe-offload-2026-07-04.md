# Test Report — M4 MoE FFN expert CPU offloading

- Date: 2026-07-04
- Node: linkcpp-node1 (RTX 4080)
- Model: `OLMoE-1B-7B-0924-Instruct-Q4_K_M.gguf` (3.92 GiB, MoE, 64 experts)
- Mechanism: upstream `-ot 'exps=CPU'` (--override-tensor) + `--no-mmap`

## Results

| Config | GPU (4080) VRAM | Model share on GPU (Δ over idle 2560 MiB) |
|--------|-----------------|-------------------------------------------|
| all layers on GPU (`-ngl 99`) | **6758 MiB** | ~4198 MiB |
| experts→CPU (`-ot 'exps=CPU'`) | **3029 MiB** | ~469 MiB |

- **~3.7 GB of expert FFN weights moved from GPU to host RAM** by the override.
- Correctness preserved: prompt "The three primary colors are" → "Red, Blue, and Yellow."
- Override confirmed active (loader warns "tensor overrides to CPU are used").

## Interpretation (F4)

Selective offload of MoE expert FFN tensors to CPU RAM works and is upstream-native
(zero patch). Because experts are the bulk of a MoE and only a few are active per token,
this lets a MoE whose experts far exceed VRAM run on a small GPU, keeping attention/router
on the GPU. In the cluster, each node applies `-ot` to the layer window it owns, so its
experts sit in its own CPU RAM — matching "offload the FFN on the node that owns the layer."

## Note / next

Offloading a REMOTE node's experts to that node's CPU (fully distributed MoE offload)
requires the `ggml-rpc-server` to expose its CPU device as an `-ot` target (it exposes only
its accelerator by default). Config/enhancement tracked for the distributed-MoE variant.
