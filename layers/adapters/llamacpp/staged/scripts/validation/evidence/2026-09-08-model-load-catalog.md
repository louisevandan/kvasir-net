# 2026-09-08 — 모델 적재 카탈로그 실측 보고

종류: 모델별 적재 가능성과 메모리 실측. 처리량·응답 품질 승인이 아니다.
소스: `1249fdd1d` 이후 카탈로그 커밋. staged server `b66beffb`(patch set `961bd89cd119`).
카탈로그 원본은 [모델별 기록](../../../../../../../test/benchmarks/model-catalog/models/)과
[사용법](../../../../../../../test/benchmarks/model-catalog/README.md)이 소유한다.

## 1. 결과 요약

| 항목 | 값 |
| --- | ---: |
| 인벤토리 논리 모델 | 41 |
| 계획 통과(적재 가능 판정) | 33 |
| 실제 적재로 확인한 모델 | 33 |
| 성공한 적재 실행 | 123 |
| 그중 계획=실제 일치 | 123 |
| 실패한 적재 실행 | 11(원인 전부 확인, 10건 재시도 성공) |

가장 큰 적재는 **MiMo-V2.5 201.4 GiB(216 GB)를 1M 컨텍스트로** 올린 것이다. GPU 25.13 GiB,
호스트 198.3 GiB를 썼다. Hy3 191.9 GiB는 100k에서 GPU 24.52 GiB, 호스트 184.5 GiB였다.

## 2. 왜 공식으로 계산할 수 없는가

같은 크기라도 층 구성이 다르면 메모리가 전혀 다르게 늘어난다. 아래는 실측이다.

- **Bonsai-27B Q1_0**: 가중치 3.5 GiB인데 256k에서 GPU 14.69 GiB. KV가 가중치의 4배다.
- **Nemotron-3-Nano-4B**: 4k에서 3.04 GiB, 1M에서 21.21 GiB로 7배.
- **MiMo-V2.5**: 201 GiB 모델인데 100k에서 GPU 9.37 GiB. 1M에서야 25.13 GiB로 뛴다.
- **Qwen3.8-27B**: 256k에서 26.93 GiB로 24 GiB 한 장에 못 들어간다. 2-stage 분할이라야 열린다.

## 3. 배치 전략별 실측

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

값은 stage 전체 합이다. `-` 는 그 컨텍스트를 시도하지 않았거나 학습 상한을 넘은 경우다.
`hikaTR`(3090+4080)과 `M42-SERVER2`(3090×2)의 값을 섞지 않으며, 각 실행 기록의 `machine`이 소유한다.

## 4. 적재하지 못한 모델과 사유

| 모델 | GiB | 사유 |
| --- | ---: | --- |
| GLM-5.3-Flash | 186 | `glm5next` 아키텍처가 고정 핀에 없음(`unknown model architecture`) |
| DeepSeek-V4-Flash | 151 | `llama_kv_cache_dsv4`가 stage-local residency 미지원. 기기 예산에는 들어감 |
| GLM-5.3 | 435 | `llama_kv_cache_dsa` 계열 미지원 + 호스트 RAM 초과 |
| GLM-5.2 | 491 | 같음 |
| MiniMax-M3 | 278 | 상류 `models/minimax-m3.cpp:50` `GGML_ASSERT(hparams.indexer_block_size > 0)` 실패 + RAM 초과 |
| nex-agi Nex-N2-Pro | 255 | 호스트 248.9 GiB 필요, 여유 초과 |
| Nemotron-3-Ultra-550B | 365 | 호스트 333.2 GiB 필요 |
| MiMo-V2.5-Pro | 467 | 호스트 447.6 GiB 필요 |

DeepSeek-V4-Flash만이 기기 예산 안에 있으면서 P4 쪽 이유로 막혀 있다. 그 메모리 구현에
stage-local residency를 지원시키는 것이 다음 확장 후보다. 단일 stage(전체 층 소유)로는 가드가
걸리지 않으므로 그 경로의 측정은 별도로 가능하다.

## 5. 운영에서 지켜야 할 제약

- **pinned 호스트 메모리 반환 지연.** stage 하나가 가중치를 CUDA pinned 메모리로 잡는다(Hy3는
  stage당 약 92 GiB). 프로세스 종료 직후 다음 적재를 시작하면
  `ggml_cuda_host_malloc: ... resource already mapped`로 실패한다. 여유 메모리 회복을 확인하고 시작한다.
- **모델 파일 핸들 지연.** 방금 종료한 stage가 GGUF를 쥐고 있어 `Permission denied`가 날 수 있다.
  파일 손상과 구분해 재시도한다.
- **NextN(MTP) 블록.** trunk 층 수는 `block_count - nextn_predict_layers`다. 이 값을 넘겨 자르면
  `llama-graph.cpp`의 층 창 단언에 걸려 stage가 죽는다. 122B(49→48), MiMo(51→48), Hy3(81→80)이 해당한다.
- **공유 KV 구간.** gemma-4는 13층부터 KV를 공유하므로 그 안에서 자르지 않는다.
- **네트워크 공유 mmap 금지.** SMB에서 mmap 오프로딩은 페이지 폴트로 사실상 정지한다(30분에 stage 실행 1회).
  `--no-mmap`을 쓰고, 반복 측정이 필요하면 로컬로 복사한 뒤 적재한다.

## 6. 이 보고가 증명하지 않는 것

- 처리량과 응답 품질. 이 기록은 적재와 메모리만 다룬다.
- 다중 물리 컴퓨터 분산(H6). 두 기기 모두 단일 호스트다.
- 측정하지 않은 컨텍스트의 값. 계획 패스가 실제와 일치함을 123회 확인했지만, 확인하지 않은
  조합까지 보장하지는 않는다.
