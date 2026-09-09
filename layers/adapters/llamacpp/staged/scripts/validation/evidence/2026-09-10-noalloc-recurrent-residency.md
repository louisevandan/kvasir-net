# 2026-09-10 — 계획 모드가 recurrent 상태를 실제로 할당하던 결함

종류: 결함 원인 확정·수정(상류 compat 패치)·적용/컴파일 검증.
**메모리 계획과 실제 할당의 대조, 실기 적재·추론 검증은 아직이며 CUDA 빌드와 원격 실행이 필요하다.**
기준 HEAD `ea202000a`. 현재 작업 순서는 [로드맵](../../../../../../../docs/distributed-batching-roadmap.md)이 소유한다.

## 원인 — 상류 자신이 두 메모리에서 다르게 한다

`stage_memory_plan.cpp`의 계획 경로는 모델을 `no_alloc = true`로 만든다. 어텐션 KV는 그 뜻대로
움직이지만 recurrent 상태는 그러지 않는다. 같은 pin의 상류 소스에서 나란히 보면 분명하다.

| | `llama_kv_cache` | `llama_memory_recurrent` (수정 전) |
| --- | --- | --- |
| 버퍼 할당 | `no_alloc`이면 **크기 0 dummy 버퍼**를 만들고 모든 tensor의 `buffer`를 그것으로 지정 | 언제나 `ggml_backend_alloc_ctx_tensors_from_buft` — **실제 할당** |
| `memory_breakdown()` | `no_alloc`이면 `ggml_backend_alloc_ctx_tensors_from_buft_size(ctx, buft)` — **정렬을 반영한 예상 크기** | 언제나 `ggml_backend_buffer_get_size(buf)` |

그래서 계획을 세우는 동안 recurrent 저장 공간이 **실제로 카드에 잡힌다.** 그 뒤 `stage_memory_plan.cpp`는
줄어든 `free`를 읽고, 방금 잡은 그 비용을 포함한 `required`와 비교한다.

09-09 35B 실행 5회의 계획이 이 산술을 그대로 보여준다(카드 초기 여유 22.76 GiB).

| arm | n_seq | `CUDA0 RS buffer` | 계획 `free` | free + RS | `required` | 판정 |
| --- | ---: | ---: | ---: | ---: | ---: | :-- |
| 2 stage | 96 | 2,814 MiB | 20.01 | **22.76** | 14.01 | ✓ |
| 2 stage | 256 | 7,504 MiB | 15.43 | **22.76** | 19.72 | ✗ |
| 2 stage(재시도) | 256 | 7,504 MiB | 15.43 | **22.76** | 19.72 | ✗ |
| 4 stage | 96 | 1,407 MiB | 21.38 | **22.75** | 6.96 | ✓ |
| 4 stage | 256 | 3,752 MiB | 19.09 | **22.75** | 10.14 | ✓ |

**원인은 가중치의 중복 계산이 아니라 계획용 recurrent 할당이다.** GPU `context` 항에는 이미 할당된
RS와 어텐션 KV가 함께 있고 둘의 취급이 다르므로, `model + compute + 2 × context`도 정확한 조건식이
아니다. r256에서 실제로 필요한 19.72 GiB는 24 GiB 카드에 들어간다.

## 수정

상류 `llama-memory-recurrent.cpp`가 `llama_kv_cache`와 같은 답을 하도록 두 곳을 맞췄다.
새 compat 패치 `0026-noalloc-recurrent-residency.patch`(`layer: upstream_fix`)다.

- 할당 루프: `hparams.no_alloc`이면 크기 0 dummy 버퍼를 만들고 모든 tensor의 `buffer`를 지정한다.
  실행 모드의 할당·초기화 의미는 그대로다.
- `memory_breakdown()`: `no_alloc`이면 `ggml_backend_alloc_ctx_tensors_from_buft_size`로
  **정렬과 패딩을 포함한 예상 크기**를 보고한다. 실제 크기 보고 경로는 그대로다.

**fit 검사를 없애거나 `free`에 context를 더하는 보정은 하지 않았다.** 공간이 실제로 부족한 구성은
계속 거부되어야 하며, 이 수정은 계획이 계획으로 남게 할 뿐이다.

수정 위치는 채택 상류의 compat 경계 안이고, llama 비공개 타입이 위층으로 올라가지 않는다.

## 지금까지의 검증

| 검사 | 결과 |
| --- | --- |
| 26개 패치 적용 | `git apply --check` 전부 통과 |
| model-agnostic 경계 | `validateCompatibilityPatch` 통과(모델·아키텍처 지식 없음) |
| 준비 트리 diff 해시 | `patch_set_sha256` = `f37f181c9d38afed04993921b30470dd69d8cfd5d232d05405f384a01439e738`로 재계산·검증 |
| `patched_tree` | `7d66751252406ad0e952409355c5cc0a34530c55` |
| Pipeline ABI 심볼 | `llama_linkcpp_runtime_configure` 존재 확인 |
| **컴파일** | CPU Release 빌드에서 `llama.dll` **214/214 링크 성공** |
| 고정된 상류 checkout | `git status --porcelain` 비어 있음 — 작업은 전부 분리된 worktree에서 |

수정 전 `patch_set_sha256`은 `961bd89cd1197cef0d683f22d99d9451e25aba5eba5f360dac6aaa792b993d81`이었고,
재현 절차(고정 pin의 worktree에 25개 패치 적용 → `git diff --binary --full-index`)로 그 값을 먼저
그대로 얻어 절차 자체가 맞는지 확인한 뒤 26번째를 얹었다.

## 아직 하지 않은 것 — 이것이 다음 단계다

컴파일은 통과했지만 **동작은 검증하지 않았다.** 다음 전부가 남아 있고 CUDA 빌드와 원격 3090×2가 필요하다.

1. 계획용 RS 실제 할당이 사라졌음을 backend 초기화 비용과 구분해 확인.
2. resident 96·160·256에서 host/device별 `model`·`context`·`compute` 계획과 실제 할당 대조.
3. attention·recurrent·hybrid 및 host/device 경로 무회귀.
4. **실제로 공간이 부족한 구성은 계속 거부**되는지 — 오거절만 고치고 과승인을 만들지 않았는지.
5. 수정 뒤 r256의 실제 적재·최대 메모리·추론·UNLOAD 별도 통과.

**“오거절을 고쳤다”와 “r256이 안전하게 실행된다”는 서로 다른 완료 조건이다.**
같은 카드를 여러 process가 쓸 때의 합산 예약도 개별 stage의 fit 통과로 대체할 수 없다.
그리고 **r256 적재 성공은 resident 상향의 서비스 승인이 아니다** — 강한 연속 웨이브에서의 resident
평가는 B2/B3 수용·반환 예산을 연결한 뒤에 한다.

빌드 비용에 대해: recurrent는 C++로 컴파일되어 `llama.dll`에 들어가므로 개발 중 증분 검증은
위와 같이 CPU 빌드로 충분했다. 실기 검증은 CUDA 산출물 갱신이 필요하며, 배포 시 서버 옆의 실제
DLL 해시까지 확인한다.
