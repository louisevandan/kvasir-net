# 2026-09-08 — no_alloc 메모리 계획이 compute 버퍼를 과소 보고하던 상류 결함

종류: 결함 재현·원인 확정·수정·검증. 성능 증거가 아니다.
대상 pin `0eadefebd3f8f92a86d634a0e5b8fffc9dc792c0`, 수정 후 patch set `961bd89cd119`.
현재 작업 순서는 [로드맵](../../../../../../../docs/distributed-batching-roadmap.md)이 소유한다.

## 증상

`StageRuntime::load`는 `inspect_stage_memory_with_initialized_backend`의 계획과 실제 적재 뒤
`measure_stage_memory`의 측정을 `same_stage_memory_allocation`으로 대조한다. 2026-09-07
Qwen3.5-122B-A10B 2-stage 오프로딩에서 tail이 이 대조에 실패해 exit 5로 종료했다
(`target/p4-4node/runs/20260907T105822Z-9888cd38`).

| 항목 | 계획 | 실제 |
| --- | ---: | ---: |
| host compute | 107,251,776 B | 142,951,040 B |
| host model / context | 40,186,750,976 / 138,936,320 B | 동일 |
| device compute | 1,283,469,440 B | 동일 |

같은 실행의 head stage는 계획=실제였고, 같은 날 통과한 Ornith-1.0-35B 오프로딩도 계획=실제였다.

## 원인

`llama-context.cpp`의 컨텍스트 예약은 pp → tg → pp 순서로 세 번 예약한다. 할당 경로는 세 번을
모두 실제로 예약한 뒤 `ggml_backend_sched_get_buffer_size`로 최종 크기를 읽으므로, 버퍼는 세 그래프
중 가장 큰 것에 맞춰 커진 상태다. 반면 `no_alloc` 계획 경로는 **첫 pp 예약에만** 크기 출력 배열을
넘긴다(`model.hparams.no_alloc ? backend_buf_exp_size.data() : nullptr`). 이어지는 tg·2차 pp 예약은
`sizes` 없이 호출돼 `ggml_backend_sched_split_graph`만 수행하고 크기를 재지 않는다. 따라서 tg 그래프나
2차 pp 예약이 더 큰 버퍼를 요구하는 모델에서 계획이 실제보다 작게 나온다.

이 코드는 우리 패치가 아니라 pin의 상류 원본이다(`git show HEAD:src/llama-context.cpp`에 존재).
실패한 tail은 `graph splits = 111 (with bs=512), 50 (with bs=1)`로 pp와 tg 그래프가 다르고,
통과한 Ornith tail은 `graph splits = 82`로 같았다. 이 차이가 두 사례를 가른다.

## 재현

같은 결함을 4.3 GiB 모델 한 프로세스로 27초 만에 재현했다. 하네스·agent·두 번째 노드가 필요 없다.
`p4_staged_server.exe`는 시작 플랜을 stdin에서 4바이트 LE 길이 접두로 읽는다.

```text
--model S:\models\unsloth\Qwen3.5-4B-MTP-GGUF\Qwen3.5-4B-Q8_0.gguf --memory-topology discrete
--layer-begin 16 --layer-end 32 --kv-layer-begin 16 --kv-layer-end 32 --n-seq-max 16
--spec-type none --kv-unified --batch-size 512 --ubatch-size 512 --ctx-size 32768
--n-gpu-layers 16 --device CUDA0 --flash-attn on --no-mmap --cache-type-k q8_0 --cache-type-v q8_0
```

| 소스 | MEMORY_PLAN host.compute | MEMORY_ACTUAL host.compute | 판정 |
| --- | ---: | ---: | --- |
| `0681d1c38` 빌드(patch set `3cfc636181e4`) | 72,648,768 | 73,220,736 | 불일치 → 적재 거부 |
| 수정 빌드(patch set `961bd89cd119`) | 73,220,736 | 73,220,736 | 일치 → 적재 진행 |

이 모델은 `graph splits = 35 (with bs=512), 6~8 (with bs=1)`로 두 그래프가 다르다.

## 수정

새 패치 `0025-noalloc-reserve-size-max.patch`, 분류 `upstream_fix`(상류 결함, `src/` 허용).
세 예약 모두에 측정 배열을 넘기고 예약마다 원소별 최댓값을 유지한다. 할당 경로(`no_alloc` 아님)는
`measured`가 널이므로 동작이 바뀌지 않는다.

검증:

- `validate-compat-manifest.mjs` valid, `validate-patch-classification.mjs` valid
  (upstream_fix 3 / stage_hook 18 / model_feature 4, 25건).
- 0001~0025를 pin에 순서대로 적용한 재현 트리가 편집한 트리와 바이트 동일.
- `prepare-pipeline-upstream.mjs`가 새 patch set으로 `0eadefebd3-961bd89cd119`를 검증 통과.
- CUDA Release 재빌드 CTest 15/15 통과. 새 exe `b66beffb479afa6db84cb7df697726a4e64e4b77a6a93343d48a545476767248`.
- 위 표의 전후 대조가 수정 제거 시 실패를 보인다(수정 전 빌드가 RED).

## 남은 것

- 최댓값 유지는 세 그래프 각각의 요구를 덮지만, 할당기가 서로 다른 그래프를 연속 예약하며 겪는
  단편화까지 모사하지는 않는다. 위 두 사례에서는 계획=실제였으나 일반 증명은 아니다.
- 이 수정은 적재 거부를 없앨 뿐 122B급 모델의 정상 응답·처리량을 증명하지 않는다.
