# Test Report — M2 distributed large-model loading + RPC local cache

- Date: 2026-07-04
- Cluster: node1 = RTX 4080 (16GB), node2 = RTX 3090 (24GB), CUDA-UUID pinned
- Model: `Qwen2.5-32B-Instruct-Q4_K_M.gguf` (18.49 GiB) — does NOT fit one GPU comfortably
- rpc-server on node2 with `-c` (local cache at /root/.cache/llama.cpp/rpc)

## Results

| Test | Result | Evidence |
|------|--------|----------|
| Distributed VRAM loading | PASS | 32B split across both GPUs: **RTX 4080 = 10,181 MiB**, **RTX 3090 = 11,372 MiB**. Neither holds the whole model. |
| Distributed inference (large model) | PASS | Coherent output: "Sure, I'll count slowly for you... 1 ... 2 ..." generated across 4080+3090 |
| node2 local weight cache | PASS | cache dir = 11 GB (200 tensor files) = node2's layer share cached locally |
| Cold vs warm network transfer | PASS | **COLD (empty cache): node1 TX = 11,134 MiB** (weights over network). **WARM (cache): node1 TX = 256 MiB** → **~43× less network traffic**; weights loaded from node2's local cache, not re-transferred |

## Interpretation

- **F2 (distribute a huge model across nodes)**: achieved — a model too big for the
  4080 alone runs by splitting weights + KV across both GPUs. KV cache is owned by the
  layer-owning device (F3), inherited from the RPC backend.
- **Slow-LAN weight locality**: the RPC `-c` local cache makes each node load its weight
  share from its own disk after the first run, cutting inter-node weight transfer ~43×.
  This is the primary practical goal of "each node loads its own share" — achieved with
  **zero core patch** (upstream `-c` flag), honoring N1 (upstream tracking).

## Gap → next (true P1)

The RPC path still requires node1 (master) to READ the full GGUF from its own disk each
load (to compute tensor hashes and load its own share). The deeper P1 patch — node2 opens
its OWN local GGUF and loads only its layer window, so node1 never reads remote layers —
would remove that. Designed in DEVELOPMENT_PLAN §5/§15.2; deferred behind the higher-value,
lower-risk upstream-native milestones (M4 MoE offload, M5 parallel).

## Environment note

C: filled up (Docker vhdx) and crashed Docker mid-M2; recovered by relocating
`docker_data.vhdx` to D: (10 TB) via junction. See docs/OPS_STORAGE.md. All user
containers/images preserved.
