# P4 구조 설명 — 일반 개발자용

> 문서 지위: **구조 설명**. 슬라이드 덱 `p4-architecture.html`/생성기 `build-intro-pptx.js`와 같은 내용의 Markdown 판이다.
> 성능 수치는 조건에 묶인 별도 측정 기록이 소유한다. 현재 목표·상태·순서는 [실행 로드맵](../distributed-batching-roadmap.md),
> 층별 책임과 upstream 격리는 [계층 격리 계약](../layer-isolation-contract.md)을 따른다.

코드 수치 기준: `b3a0d51ef` (2026-09-12). 이 문서의 사실은 문서가 아니라 코드에서 확인했고, 각 절 끝에 확인한 위치를 적었다.

---

## 1. 한 줄 요약

GPU 한 장에 들어가지 않는 모델을 여러 머신에 레이어 단위로 나눠 싣고, 추론 중에는 계산 중간값만 주고받는다.
P4는 그 연결을 담당하는 계층이다 — 추론 엔진이 아니다.

| 수치 | 값 |
| --- | --- |
| 프로세스 타입 | 1종 (agent) |
| llama.cpp가 받치는 ggml backend | 18 |
| Rust | 114,154줄 · 419파일 · 12 crate |
| 고정된 llama.cpp pin | 10 (최신 `451b89bae`, 패치 27개) |

---

## 2. 왜 이런 계층이 필요한가

모델이 장치 한 개에 들어가지 않으면, **나누는 방법이 성능을 정한다.**

**텐서 병렬** — 레이어 하나를 여러 장치에 쪼갠다. 자르는 곳마다 전체 활성값을 맞춰야 하므로
장치 사이 대역폭이 곧 한계가 되고, 한 대 안의 초고속 링크를 전제한다.

**파이프라인 병렬** — 레이어 구간으로 자른다. 자른 경계에서만 계산 중간값이 건너가고
가중치와 KV 캐시는 각 머신에 머문다. 일반적인 네트워크로 여러 대를 묶을 수 있는 이유다.
P4가 구현한 쪽이 이것이다.

공짜는 아니다. 한 요청이 스테이지를 차례로 지나므로 스테이지 수만큼 지연이 쌓이고 앞 스테이지가 노는 시간이 생긴다.
여러 요청을 겹쳐 흘리는 배치 구성이 이 계층의 핵심 과제가 되는 이유이며, 6절과 7절이 그 이야기다.

P4가 **맡는 것**은 어느 머신의 어느 노드가 어느 구간을 맡는지, 요청이 그 사슬을 어떤 순서로 지나는지,
무엇이 언제 건너가는지다. **맡지 않는 것**은 행렬 곱, 커널, 양자화, 샘플링이다 — 그건 backend의 일이다.

---

## 3. 레이어 구조 — 위층은 아래층의 이름을 모른다

| 층 | 하는 일 | 갖지 않는 것 |
| --- | --- | --- |
| OUTER | 어디에 무엇을 싣고 누가 무엇을 받을지 정한다 | 엔진 KV 직접 변경 |
| `layers/protocol` | 봉투와 프레임 — 주소·순서·경계 | 내용 해석 (불투명 바이트) |
| `layers/agent` | 프로세스·큐·워커·노드 수명·전달과 역압 | backend 이름 |
| `layers/adapters/adapter` | 노드가 backend에 요구하는 것 — 제출·완료·취소 | 특정 engine의 타입 |
| 구상 어댑터 | `llamacpp-staged` · `llamacpp` · `vllm` · `sglang` · `mock` | 다른 backend의 상태 |
| backend | llama.cpp → ggml → CUDA · ROCm · Metal · CPU … | — |

이 구조가 만드는 세 가지 성질:

- **봉투만 읽으면 전달된다.** 중계 노드는 내용을 열지 않으므로 새 메시지 종류가 중계를 무겁게 만들지 않는다.
- **backend 등록은 한 파일이다.** `entrypoints/agent/src/adapters/mod.rs` — 이름 하나, 팩토리 하나, 구현 하나.
- **mock이 항상 들어 있다.** GPU 없이 전체 fleet을 띄워 배치와 순서를 검증할 수 있다.

agent가 바깥에 내보이는 표면은 소켓 위의 P4 하나뿐이다. 어댑터가 자기 backend와 HTTP로 말하든,
파이프로 말하든, 같은 프로세스 안에서 함수로 부르든 그 위에서는 보이지 않는다.

---

## 4. 토폴로지 — 머신마다 프로세스 하나, 그 안에 노드 여러 개

controller 프로세스는 없다. agent가 다른 agent에 닿는 길과, agent가 바깥에 답하는 길이 같은 길이다.

- **agent** — 머신마다 하나 뜨는 프로세스. 소켓으로 프레임을 받아 큐에 넣고, 워커가 주소만 보고 자기 것인지 판단한다.
  자기 것이 아니면 통째로 넘긴다.
- **node** — agent 안의 논리 실행 단위. id 하나로 시작해 LOAD가 어댑터를 물리면 실체가 된다.
  자기 큐를 갖고 긴 작업을 혼자 쥔다. 레이어 구간 하나를 맡는다.
- **OUTER** — 바깥에서 요청하는 쪽. 어느 노드가 어느 구간을 맡을지, 어떤 노드들이 한 세션을 이룰지 정한다.

agent ↔ agent 와 agent ↔ OUTER 는 같은 P4 프레임을 쓴다.

지금까지 돌린 구성 예: 1호스트 8스테이지, 2호스트 16스테이지, 5호스트 6스테이지.
노드 수는 모델 크기·KV 용량·합법적인 자르기 지점이 정하지, 카드 수가 정하지 않는다.

---

## 5. 적재와 파이프라인은 서로 다른 수명이다

**1단계 LOAD — 노드마다 독립 명령.** 각 노드가 GGUF에서 자기 레이어 구간만 읽어 장치에 올린다.
구간은 `stage_begin`/`stage_end` 반열린 구간이다. 이웃도, 순서도, 세션도 이 명령에 없다.

**2단계 SESSION — 순서를 설치.** 참여 노드 전체 목록과 각 노드의 자기 번호를 한 번에 알려 준다.

```text
stages: [ {agent, node, generation}, … ]   +   stage_index
```

설치 조건이 엄격하다. `stages[stage_index]`가 **실제로 이 워커의 endpoint**여야 하고
envelope의 target도 같아야 한다. `load_generation`이 현재 적재 세대와 다르면 거절한다.
first/previous/next/terminal은 **설치한 순서에서만** 파생한다 — 노드가 스스로 정하지 않는다.

이 분리가 사는 이유:

- 같은 적재 위에 세션을 여러 번 세울 수 있다. 순서를 바꾸는 데 재적재가 필요 없다.
- UNLOAD는 유휴 상태에서만 받고, 통과하면 세션·요청·비행 기록·세션 키를 모두 지우고 세대를 0으로 만든다.
  옛 세대를 든 SESSION은 그 뒤 stale로 거절된다.
- head는 정산과 출력 승인을 소유하고 terminal은 토큰을 만든다. 둘은 다른 노드다.

*확인: `v2/node/worker/control.rs`의 `session`/`unload`.*

---

## 6. 노드가 backend를 고르는 법

노드가 아는 것은 어댑터 계약 하나다. 그 아래는 갈아끼운다.

| 등록 이름 | 붙는 backend |
| --- | --- |
| `mock` | 산술만. 장치 없이 |
| `mock-instant` | 즉시 응답. 순서 시험용 |
| `llamacpp` | llama-server (HTTP) |
| `vllm` | vLLM 서버 |
| `sglang` | SGLang 서버 |
| `llamacpp-staged` | 레이어 분할 실행 (준비된 실행 파일이 있는 호스트에서만 노출) |

두 가지 모양의 llama.cpp가 있다. `llamacpp`는 한 프로세스가 모델 전체를 쥐고 HTTP로 답하므로
노드 하나로 끝나고 사슬 길이가 1이다. `llamacpp-staged`는 모델을 레이어 구간으로 잘라 여러 노드에 싣고,
사슬 길이는 노드 수만큼이다. vLLM·SGLang처럼 backend가 스스로 모델을 펼치는 쪽은 사슬 길이 1로 참여한다.

`mock`이 항상 빌드에 들어 있다는 점이 실용적으로 중요하다. GPU도 모델 파일도 없이 fleet 전체를 띄워
라우팅·순서·배치·취소를 끝까지 돌려 볼 수 있고, 계산이 산술로 대체되므로 결과가 결정론적이다.
두 번 돌린 결과가 다르면 그 차이는 P4에 있다.

*확인: `entrypoints/agent/src/adapters/mod.rs`의 `registry`.*

---

## 7. 배치 ① — 한 배치에 무엇을 넣는가

**논리 배치 하나를 채우는 순서.** decode가 1행씩 먼저 들어가고, 남은 행을 prefill이 회전 water-fill로 채운다.

- 어텐션 모델은 논리 배치를 `llama_n_batch`까지 채우고, 쪼개는 일은 llama.cpp가 `n_ubatch`에서 한다.
- recurrent·hybrid는 시퀀스마다 같은 폭을 요구하므로, 한 번의 호출이 정확히 물리 UBATCH 하나를 만든다.
- Verify·Replay는 쪼갤 수 없는 한 트랜잭션이다 — 한 물리 UBATCH 안에 있어야 한다.

**묶음의 폭은 "지금 빈자리"가 아니라 모집단이 정한다.** 준비됨 + 비행 중 + 대기 중을 창 수로 나눠
코호트 폭을 파생한다. 요청 하나가 잠깐 돌아왔다고 묶음이 넓어지지 않고, 프롬프트를 다 낸 뒤 아직
정산되지 않은 요청은 자기 코호트에 남되 새 묶음을 넓히지 않는다.

**생성이 살아 있으면 prefill은 몫만 쓴다.** 디코드가 하나라도 진행 중이면 prefill 행이
`mixed_prefill_rows`로 제한된다. 순수 prefill이면 전체 토큰 예산을 쓴다 — 시험이 고정한 예로
생성 중 128행, 순수 prefill 512행이다. 선점이 아니라 비선점 작업 단위다.

**`PREFILL_PATIENCE = 8`.** decode에 연속 8배치를 준 뒤에도 프롬프트가 기다리고 있으면 다음 배치는
프롬프트 차례다. 몫이 아니라 상한이다. 코드 주석이 스스로 밝힌다 — *8은 측정값이 아니다. 바운드가
존재하게 만드는 최소한일 뿐이고, 실제 하드웨어의 처리량으로 판정된 적이 없다.*

준비된 선택은 수락되기 전까지 공정성을 소비하지 않는다. 거부되거나 취소된 후보는 순번을 쓰지 않고,
남의 계획과 낡은 계획은 이름을 붙여 거절한다. 선택 층은 순수하다 — 여기서 KV도 실행 권한도 확정되지 않는다.

*확인: `v2/scheduler.rs`(`Phase`·`Demand`·`PREFILL_PATIENCE`·`PreparedPlan`), `v2/scheduler/pipeline.rs`의 `PipelinePolicy::select`.*

---

## 8. 배치 ② — 언제 내보내는가

고른 배치를 즉시 보내면 꼬리에 줄이 서고, 무조건 기다리면 깊이를 잃는다.
그래서 **실제로 걸린 시간을 모아 다음 작업의 비용을 예측한다.**

관측 → 예측 → 투영 → 판정. 스테이지마다 Frame 왕복 시간을 표본으로 쌓고, 후보 배치 모양의
스테이지별 시간을 프로파일에서 추정하고, 아직 확정되지 않은 비행분까지 전 스테이지 FIFO로 더한 뒤 판정한다.

| 판정 | 뜻 |
| --- | --- |
| `PurePrefill` | 생성이 없다 — 전체 폭 |
| `DecodeOnly` | prefill 행이 없다 |
| `Cold` | 내가 내지 않은 배치가 열려 있다 — 프로파일 없음 |
| `CalibrationWait` | 표본이 아직 모자라다 |
| `Admit` | 예산 안에 들어온다 |
| `DeferPrefill` | 넘는다 — 이번에는 prefill을 넣지 않는다 |
| `ProgressProbe` | 목표가 닿지 않아도 굶기지 않으려고 한 몫을 낸다 |

예산은 `P4_STAGED_PREFILL_SERVICE_MS`로 밀리초를 주면 마이크로초 예산이 된다. 주지 않으면 이 정책 자체가 꺼져 있다.

코드가 스스로 못을 박아 둔다 — **이것은 예측 정책이지 실행도, KV 권한도, 전송 credit도, 응답 시간 보장도 아니다.**
투영에는 측정하지 않은 전송·반환 지연이 빠져 있고, 클라이언트가 체감하는 토큰 간 간격을 약속하지 않는다.
decode만 모으는 지연은 최대 2 ms로 묶여 있다.

별도로, 이미 충분한 배치가 비행 중이면 계획을 head에서 잠시 쥔다. 그 근거도 주석에 숫자로 남아 있다 —
꼬리가 바쁠 때 도착한 배치는 앞 배치를 p50 128 ms 기다렸고 전체의 63%가 그랬다. 배치 하나는 꼬리에서
첫 레이어까지 약 55 ms의 고정비를 쓴다. 자리가 있으면 얇아도 즉시 보낸다 — 자리와 무관하게 폭을
기다렸던 이전 실험은 26%를 잃었다.

**이 knob들은 전부 기본 off다.** `DECODE_MEMBERS` · `PREFILL_MEMBERS` · `PREFILL_ROWS` ·
`PREFILL_ROWS_PER_REQUEST` · `MAX_OPEN_BATCHES` · `MAX_ISSUE_ROWS` · `MIN_BATCH_ROWS` ·
`PREFILL_FRAGMENTS` · `PIPELINE_BATCHING` · `MIXED_BATCH_ROWS` · `MIXED_PREFILL_ROWS` · `PREFILL_SERVICE_MS`.
켜지 않으면 기존 경로 그대로 돈다. 위 수치는 그 가설의 근거이지 승격 결과가 아니다.

*확인: `v2/scheduler/service.rs`(`ServiceSample`·`ServiceVerdict`·`ServiceBudget::decide`), `v2/node/worker/service.rs`, `v2/node/worker/drive.rs`, `v2/node/state.rs`의 기본값.*

---

## 9. 적재된 모습 — 노드마다 자기 구간의 가중치와 KV

80레이어 모델을 네 노드에 나눈 예:

| 노드 | 레이어 | 장치 | 그 노드가 갖는 것 |
| --- | --- | --- | --- |
| node 0 | `[0, 20)` | `CUDA0` | 가중치 · KV · compute buffer — 이 구간만 |
| node 1 | `[20, 40)` | `CUDA1` | 〃 |
| node 2 | `[40, 60)` | `ROCm0` | 〃 |
| node 3 | `[60, 80)` | `MTL0` | 〃 |

같은 `n_ctx`인데도 노드마다 KV 비용이 다르다. 과거 관측에서 173.5 / 63.3 / 157.7 / 126.1 MB였다 —
레이어 구성이 구간마다 다르기 때문이고, 파이프라인의 한계는 가장 비싼 노드가 정한다.

계획과 실제가 같은지 확인하는 장치가 있다. `--expect-layer-device begin:end:name`으로 선언하면
구간이 빈틈도 겹침도 없이 전체 컷을 덮는지 검사하고, 적재 전(PLAN)과 적재 후(LOAD) 두 번 장치 배치를
질의해 대조한다. 메모리 총량이 맞는 것만으로는 통과하지 않는다. CPU에 남길 구간도 명시적으로 선언한다.

"노드 = GPU 한 장"이 규칙인 것은 아니다. 한 장치에 여러 스테이지를 둘 수도, 한 스테이지가 여러 장치를
쓸 수도, 일부 구간을 CPU에 둘 수도 있다.

*확인: `staged/adapter/src/config.inc.rs`의 `stage_begin`/`stage_end`, `server/src/runtime/stage_memory_plan.hpp`, `docs/llamacpp-stage-memory.md`.*

---

## 10. 추론 중 네트워크를 넘는 것

**가중치도 KV 캐시도 compute buffer도 노드를 떠나지 않는다.**
한 스텝에서 건너가는 것은 자른 경계의 텐서 묶음 — in-flight 배치다.

| 무엇 | 크기 | 움직임 |
| --- | --- | --- |
| 가중치 | 수십~수백 GB | 한 번 적재, 이후 이동 없음 |
| KV 캐시 | 노드·요청마다 수십~수백 MB | 이동 없음 — 그래서 요청은 자기 KV가 있는 노드 집합에 묶인다 |
| 스텝당 전송 | 경계 텐서 몇 개 | gemma-4에서 31·27·23개, 스텝당 81회. Qwen 계열은 1개 |

terminal이 만든 토큰은 head로 돌아가 **승인된 뒤에야** 바깥으로 나간다. head는 원본 제출의 권한 지문과
꼬리가 돌려준 발행 증거를 대조한 뒤 출력을 만든다.

그래서 노드 사이에 요구되는 대역폭이 텐서 병렬보다 훨씬 작다. 대신 값을 치른다 — 스테이지가 늘수록
지연이 쌓이고, 한 번에 한 요청만 흘리면 앞 스테이지가 논다. 그 빈 시간을 메우는 것이 7·8절의 주제다.

*확인: `v2/capsule.rs`의 `PhysicalCapsule`/`Tensor`, `v2/node/worker/release.rs`의 `prepare_outputs` 호출부(head), `docs/adapter-batching-layers.md`의 관측표.*

---

## 11. KV 캐시 영속화

스테이지가 자기 레이어 구간의 KV를 갖고 있으므로 저장도 복원도 노드마다 따로 일어난다.
llama.cpp의 공개 상태 API를 그대로 쓴다.

1. **Persist** — `llama_state_seq_get_size_ext` / `llama_state_seq_get_data_ext`
2. `<kv-root>/<key>.lkv` 에 manifest와 상태 바이트, 체크섬을 함께 기록
3. **셀 회수** — `llama_memory_seq_rm`으로 저장한 뒤 KV 셀을 비운다
4. **Restore** — 파일을 읽어 `llama_state_seq_set_data_ext`로 되돌리고, 그 업로드가 끝나기 전에는 다음 디코드를 허용하지 않는다

되살릴 자격은 manifest 여섯 항목이 정한다: `build_identity`, `runtime_identity`, `context_identity`,
`kv_format`(K=타입;V=타입;flags), `token_position`, `checksum`/`bytes`.
**하나라도 다르면 거부한다** — 다른 빌드·다른 컨텍스트·다른 KV 타입의 상태를 되살리지 않는다.

- `--kv-root`를 주지 않으면 기능 자체가 꺼진 것으로 보고된다. 조용히 메모리에만 남는 경로가 없다.
- 디코드가 실패해 메모리가 더러워진 런타임은 저장·복원·삭제를 **모두** 거부한다.
  어느 시퀀스가 망가졌는지 llama.cpp가 알려 주지 않으므로, 찢어졌을 수 있는 상태를 파일로 만들지 않는다.
- 시퀀스 하나의 상태가 128 MiB를 넘으면 거절한다.

이것은 대화를 이어 붙이기 위한 저장이지 장애 복구 장치가 아니다.

*확인: `server/src/runtime/llama_stage_runtime_kv.cpp`의 `save`/`restore`, `state_store.cpp`의 manifest 대조, `main.cpp`의 capability 설정.*

---

## 12. MTP와 그 밖의 스페큘러티브 — 자동이 아니다

**"llama.cpp가 지원하면 자동으로 지원된다"는 이 경로에는 해당하지 않는다.**
스테이지를 자른 경로에서 제안·검증·롤백은 노드 경계를 넘는 상태이므로, 상류가 새 방법을 추가해도
자동으로 켜지지 않도록 일부러 반대로 만들어져 있다.

- **구현된 것**: `COMMON_SPECULATIVE_TYPE_DRAFT_MTP` 하나 — 모델이 스스로 다음 토큰을 제안하는 방식.
- **거부되는 것**: 별도 draft 모델을 요구하면 `CAPABILITY_UNAVAILABLE: draft_context_and_proposal_state_not_in_hop`,
  그 밖의 speculative 방법이면 `CAPABILITY_UNAVAILABLE: proposal_accept_rollback_state_not_in_hop`로 **LOAD가 실패한다.**

지원 목록을 한곳에 적어 두는 이유를 주석이 밝힌다 — 호출 지점에서 상류 enum을 훑는 방식이었다면
새 열거자가 조용히 지원되는 것으로 오분류된다.

MTP를 위해 따로 있는 것들: 스케줄러의 1급 phase로 `Verify`·`Replay`가 있고 둘은 원자 트랜잭션이다.
꼬리 스테이지가 제안을 만들고 시퀀스마다 제안 상태를 따로 관리한다. 메모리 계획이 draft 컨텍스트의
사용량까지 적재 전에 함께 측정한다. compat 패치에 MTP 꼬리 스테이지와 speculative 시퀀스 수명이
별도 항목으로 들어 있다.

**진짜로 자동인 자리는 따로 있다 — 장치 backend다.** 스테이지 런타임은 `ggml_backend_load_all()`을
부르고 끝이며, CUDA·Vulkan·HIP·Metal·OpenCL 분기를 하나도 갖고 있지 않다. 모델 구조·양자화·샘플러·문법도
같은 뜻에서 llama.cpp의 것을 그대로 쓴다. 따라오는 것과, 경계를 넘는 상태라서 명시 구현이 필요한 것이
나뉘는 지점이 여기다.

*확인: `compat/p4_llama_compat.cpp`의 `LlamaPlan::requests_unsupported_speculative`, `runtime/llama_stage_runtime.cpp`의 `StageRuntime::load`.*

---

## 13. KV 말고 더 잡는 모델 — 선언한 것만 자를 수 있다

llama.cpp의 메모리는 평범한 KV 하나가 아니다. 핀된 upstream에 구현이 10종 있다:

| 스테이지 잔여 선언 | 구현 |
| --- | --- |
| 선언함 (4) | `llama-kv-cache` · `llama-kv-cache-iswa` · `llama-memory-recurrent` · `llama-memory-hybrid` |
| 선언 없음 (6) | `llama-kv-cache-dsa` · `llama-kv-cache-dsa-iswa` · `llama-kv-cache-dsv4` · `llama-kv-cache-msa` · `llama-memory-hybrid-idx` · `llama-memory-hybrid-iswa` |

게이트는 두 겹이다. 기본값은 **거부**(`linkcpp_stage_residency_supported = false`)이고,
구현이 명시적으로 `true`로 덮어써야 자를 수 있다.

1. 팩토리에서 컴파일 타임 상수로 한 번 — 부분 stage인데 선언이 없으면 메모리를 아예 만들지 않는다.
2. 컨텍스트 생성에서 가상 호출로 다시 한 번 — "스테이지 잔여를 선언하지 않았다"로 던진다.

iSWA는 자기 저장소가 없고 base·swa 두 캐시에 위임한다. 재사용되는 KV 영역을 가르는 경계는
그 둘의 생성자가 거절한다.

적재 전에 장치별로 쪼개 계산한다 — `model`(가중치) · `context`(KV와 그 밖의 상태) · `compute`(실행 버퍼).
셋의 합을 장치의 여유와 대조하고, 맞지 않으면 **할당하기 전에** LOAD가 실패한다.

그래서 "캐시를 더 잡는 모델도 처리된다"는 반만 맞다. 추가 저장소가 계획에 잡히고 스테이지에 상주하는 것은
선언한 4종에 한정되며, DSA·DSV4·MSA 같은 희소 어텐션 계열은 아직 선언이 없어 자르면 거부된다.
한 노드에 통째로 싣는 경로에서는 upstream 그대로 동작한다.

*확인: `upstream/src`의 메모리 구현 목록, `staged/compat/451b89bae/0016·0017·0022` 패치, `server/src/runtime/stage_memory_plan.hpp`.*

---

## 14. 정리 — 개발자에게 무엇을 주는가

1. **큰 모델을 노드를 더해 돌린다.** GPU 한 장, 머신 한 대의 한계가 모델 크기의 한계가 아니게 된다.
   레이어 구간으로 자르므로 노드를 더해도 늘어나는 통신은 경계 하나뿐이다.
2. **backend를 갈아끼운다.** 노드가 아는 것은 어댑터 계약 하나다. 이름 하나와 구현 하나를 등록하면
   그 위층은 한 줄도 바뀌지 않는다.
3. **플랫폼 대응을 빌려 쓴다.** 장치 대응은 llama.cpp가 이미 하고 있고, 상류 변화의 충격은 pin 디렉터리 안에서 끝난다.
4. **느린 곳을 지목할 수 있다.** agent의 큐 깊이와 노드 안에서 실행 중인 수를 따로 보고하므로,
   느려졌을 때 P4가 쥐고 있는지 backend가 쥐고 있는지가 관측값으로 갈린다.

경계를 분명히 해 둔다 — **TLS·인증·인가는 없다.** 주소가 스스로를 밝히는 것을 믿는 구조이므로
신뢰할 수 있는 망 안에서만 쓴다. **내구 상태도 없다.** agent가 재시작하면 노드는 사라지고,
무엇이 있어야 하는지에 대한 기록은 OUTER가 쥐고 있다.
