# 2026-09-08 — Model load catalog measurement report

Kind: per-model loadability and measured memory. This is not acceptance of throughput or response quality.
Source: the catalog commit after `1249fdd1d`. staged server `b66beffb` (patch set `961bd89cd119`).
The catalog originals are owned by the [per-model records](../../../../../../../test/benchmarks/model-catalog/models/) and
the [usage guide](../../../../../../../test/benchmarks/model-catalog/README.md).

## 1. Summary of results

| Item | Value |
| --- | ---: |
| Logical models in inventory | 41 |
| Plan passed (judged loadable) | 33 |
| Models confirmed by real load | 33 |
| Successful load runs | 123 |
| Of which plan = actual | 123 |
| Failed load runs | 11 (all causes identified; 10 succeeded on retry) |

The largest load was **MiMo-V2.5 at 201.4 GiB (216 GB) with a 1M context**. It used 25.13 GiB GPU and
198.3 GiB host. Hy3 at 191.9 GiB used 24.52 GiB GPU and 184.5 GiB host at 100k.

## 2. Why it cannot be computed by formula

Even at the same size, memory grows completely differently when the layer composition differs. The following are measured.

- **Bonsai-27B Q1_0**: weights are 3.5 GiB, but GPU is 14.69 GiB at 256k. KV is 4 times the weights.
- **Nemotron-3-Nano-4B**: 3.04 GiB at 4k and 21.21 GiB at 1M, 7 times more.
- **MiMo-V2.5**: a 201 GiB model, yet GPU is 9.37 GiB at 100k. It only jumps to 25.13 GiB at 1M.
- **Qwen3.8-27B**: 26.93 GiB at 256k, which does not fit on a single 24 GiB card. It only opens with a 2-stage split.

## 3. Measurements by placement strategy

| GiB | arch | model | strategy | 4k | 32k | 100k | max ctx / GPU | host |
| ---: | --- | --- | --- | ---: | ---: | ---: | --- | ---: |
| 0.8 | qwen35 | Qwen3.5-0.8B-MTP | vram_only | 1.22 | 1.48 | 2.10 | 256k / 3.83 | 1.3 |
| 1.3 | qwen35 | Qwen3.5-2B-MTP | vram_only | 1.72 | 1.97 | 2.59 | 256k / 4.31 | 1.6 |
| 1.5 | qwen2 | Qwen_Qwen2.5-1.5B-Instruct-Q8_ | vram_only | 1.87 | 2.31 | - | 32k / 2.31 | 0.7 |
| 2.9 | nemotron_h | NVIDIA-Nemotron-3-Nano-4B | vram_only | 3.04 | 3.34 | 4.52 | 1024k / 21.21 | 2.8 |
| 3.5 | qwen35 | Bonsai-27B | vram_only | 4.17 | 5.20 | 7.99 | 256k / 14.69 | 1.3 |
| 4.3 | qwen35 | Qwen3.5-4B-MTP | vram_only | 4.42 | 5.02 | 6.51 | 256k / 10.62 | 2.1 |
| 4.4 | qwen3 | Qwen3-Embedding-8B | vram_only | 4.84 | 7.01 | - | 32k / 7.01 | 1.1 |
| 4.7 | gemma4 | gemma-4-E2B-it | vram_only | 2.87 | 3.02 | 3.43 | 100k / 3.43 | 3.6 |
| 6.1 | qwen35 | Qwen3.5-9B-MTP | vram_only | 5.45 | 6.04 | 7.54 | 256k / 11.66 | 2.3 |
| 12.7 | gemma4 | gemma-4-12b-it | vram_only | 13.18 | 13.42 | 14.18 | 256k / 16.22 | 2.5 |
| 16.1 | gemma4 | gemma-4-31B-it-qat | vram_only | 17.08 | 18.44 | 22.31 | 100k / 22.31 | 2.0 |
| 17.4 | qwen35 | Qwen3.8-27B | vram_only | 16.43 | 17.48 | 20.23 | 256k / 26.93 | 2.6 |
| 19.7 | gemma4 | gemma-4-31B-it | dense_ffn_cpu | 7.78 | 9.22 | 13.06 | 256k / 22.28 | 15.4 |
| 20.9 | nemotron_h_moe | NVIDIA-Nemotron-3-Nano-30B-A3B | expert_cpu | 3.17 | 3.34 | 3.68 | 1024k / 9.92 | 21.4 |
| 21.3 | qwen35 | Qwen3.6-27B-MTP | vram_only | 19.78 | 20.77 | 23.48 | 256k / 30.19 | 3.2 |
| 21.6 | qwen35 | Qwen3.8-27B-NVFP4-MTP | vram_only | 18.32 | 19.34 | 22.05 | 256k / 28.76 | 4.4 |
| 23.2 | qwen35moe | Ornith-1.0-35B | expert_cpu | 2.86 | 3.18 | 3.99 | 256k / 6.53 | 22.3 |
| 23.6 | qwen35moe | nex-agi_Nex-N2-mini | expert_cpu | 3.17 | 3.06 | 4.24 | 256k / 6.40 | 22.9 |
| 24.4 | nemotron_h_moe | NVIDIA-Nemotron-3.5-Lightning- | expert_cpu | 3.46 | 3.61 | 3.95 | 1024k / 9.96 | 24.0 |
| 27.1 | qwen35 | Qwen3.6-27B-Claude-Mythos-Dist | vram_only | 24.78 | 25.84 | 28.58 | 256k / 35.29 | 3.8 |
| 27.1 | qwen35 | Qwen3.8-27B-Uncensored-OrcaRou | vram_only | 24.78 | 25.84 | 28.58 | 256k / 35.29 | 3.8 |
| 30.1 | muse-glimmer | Muse-Glimmer-30B | vram_only | 27.32 | 27.51 | 27.97 | 100k / 27.97 | 4.1 |
| 31.3 | nemotron_h_moe | NVIDIA-Nemotron-3-Nano-30B-A3B | expert_cpu | 3.85 | 4.02 | 4.36 | 1024k / 10.38 | 31.8 |
| 52.0 | qwen3next | Qwen3-Coder-Next | expert_cpu | 3.64 | 4.04 | 5.02 | 256k / 7.59 | 50.7 |
| 77.0 | laguna | Laguna-S-2.1 | expert_cpu | 5.07 | 5.81 | 7.58 | 256k / 12.62 | 74.1 |
| 80.3 | mistral3 | Mistral-Medium-3.5-128B | dense_ffn_cpu | 20.83 | 26.05 | - | 32k / 26.05 | 61.4 |
| 82.2 | qwen35moe | Qwen3.5-122B-A10B-MTP | expert_cpu | 6.55 | 7.89 | 8.87 | 256k / 11.12 | 76.6 |
| 83.6 | nemotron_h_moe | NVIDIA-Nemotron-3-Super-120B-A | expert_cpu | 11.63 | 11.74 | 12.18 | 1024k / 18.80 | 78.7 |
| 103.7 | qwen4exp | Qwen3.8-Flash-Next | expert_cpu | 6.62 | 8.33 | 9.73 | 256k / 12.85 | 127.4 |
| 136.4 | step35 | Step-3.7-Flash | expert_cpu | 9.54 | 10.24 | 12.00 | 256k / 16.04 | 131.3 |
| 148.1 | minimax-m2 | MiniMax-M2.7 | expert_cpu | 5.79 | 9.44 | 18.26 | 100k / 18.26 | 145.3 |
| 191.9 | hy_v3 | Hy3 | expert_cpu | 8.36 | 13.10 | 24.52 | 100k / 24.52 | 184.5 |
| 201.4 | mimo2 | MiMo-V2.5 | expert_cpu | 8.15 | 8.54 | 9.37 | 1024k / 25.13 | 198.3 |

Values are totals across all stages. `-` means that context was not attempted or exceeds the trained limit.
Values from `hikaTR` (3090+4080) and `M42-SERVER2` (3090×2) are not mixed; each run record's `machine` owns them.

## 4. Models that could not be loaded, and why

| Model | GiB | Reason |
| --- | ---: | --- |
| GLM-5.3-Flash | 186 | The `glm5next` architecture is not in the fixed pin (`unknown model architecture`) |
| DeepSeek-V4-Flash | 151 | `llama_kv_cache_dsv4` does not support stage-local residency. It fits within the machine budget |
| GLM-5.3 | 435 | `llama_kv_cache_dsa` family unsupported + exceeds host RAM |
| GLM-5.2 | 491 | same |
| MiniMax-M3 | 278 | Upstream `models/minimax-m3.cpp:50` `GGML_ASSERT(hparams.indexer_block_size > 0)` fails + exceeds RAM |
| nex-agi Nex-N2-Pro | 255 | Needs 248.9 GiB host, exceeds headroom |
| Nemotron-3-Ultra-550B | 365 | Needs 333.2 GiB host |
| MiMo-V2.5-Pro | 467 | Needs 447.6 GiB host |

DeepSeek-V4-Flash is the only one that fits the machine budget yet is blocked for a P4-side reason. Making its memory
implementation support stage-local residency is the next expansion candidate. A single stage (owning all layers) does not
trip the guard, so that path can be measured separately.

## 5. Operational constraints

- **Delayed return of pinned host memory.** One stage holds its weights in CUDA pinned memory (Hy3 uses about 92 GiB
  per stage). Starting the next load right after the process exits fails with
  `ggml_cuda_host_malloc: ... resource already mapped`. Confirm that free memory has recovered before starting.
- **Delayed model file handle.** A stage that just exited may still hold the GGUF, producing `Permission denied`.
  Distinguish this from file corruption and retry.
- **NextN (MTP) blocks.** The trunk layer count is `block_count - nextn_predict_layers`. Cutting beyond this value
  trips the layer window assertion in `llama-graph.cpp` and kills the stage. This applies to 122B (49→48), MiMo (51→48) and Hy3 (81→80).
- **Shared KV region.** gemma-4 shares KV from layer 13 on, so do not cut inside that region.
- **No mmap over network shares.** mmap offloading over SMB effectively stalls on page faults (1 stage execution in 30 minutes).
  Use `--no-mmap`, and if repeated measurements are needed, copy locally before loading.

## 6. What this report does not prove

- Throughput and response quality. This record covers only load and memory.
- Distribution across multiple physical computers (H6). Both machines are single hosts.
- Values for contexts that were not measured. The plan pass was confirmed to match reality 123 times, but that does not
  guarantee combinations that were not checked.
