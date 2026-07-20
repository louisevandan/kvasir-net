# Test Plan — M0 scaffold + M1 distributed RPC baseline

Target: single-machine two-GPU cluster. Host GPU order: idx0 = RTX 3090, idx1 = RTX 4080.
Containers: `linkcpp-node1` → RTX 4080, `linkcpp-node2` → RTX 3090.

## Environment
- Docker 29.5.3, nvidia runtime enabled, driver 596.21 (CUDA 13.2 capable).
- Image `linkcpp-node:cuda` built from `docker/Dockerfile.cuda` (llama.cpp CUDA+RPC, arch 86;89).
- Model: `models/qwen2.5-0.5b-instruct-q4_k_m.gguf`.

## T0 — image builds
- Cmd: `docker compose build node1`
- Pass: image `linkcpp-node:cuda` created, no build error.

## T1 — GPU visible per container (M0)
- Cmd: `docker compose up -d && docker exec linkcpp-node1 /app/bin/linkcpp-node`
       `docker exec linkcpp-node2 /app/bin/linkcpp-node`
- Pass: node1 lists a CUDA device named for **RTX 4080**; node2 lists **RTX 3090**.
  (Confirms device pinning via compose `device_ids` + our smoke binary links libllama.)

## T2 — single-node inference sanity (M0)
- Cmd: `docker exec linkcpp-node1 /app/bin/llama-cli -m /models/<model> -ngl 99 -no-cnv -p "<prompt>" -n 64 --seed 42`
- Pass: coherent completion, runs on GPU (VRAM used), no crash.

## T3 — distributed RPC inference (M1)
- Cmd: `bash scripts/baseline_rpc.sh <model> 64`
- Pass: node1 llama-cli completes using local 4080 + remote 3090; rpc.log on node2
  shows graph_compute activity; only hidden states cross the boundary (layer split).

## T4 — equivalence single vs distributed (correctness)
- Same model, prompt, `--seed 42`, `-n 64`, greedy.
- (a) single: T2 output tokens. (b) distributed: T3 output tokens.
- Pass: identical (or numerically-equivalent) generated text → pipeline preserves semantics.

## Reports
Execution traces recorded under `tests/reports/` with timestamps.
