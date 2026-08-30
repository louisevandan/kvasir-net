# 어댑터 배치 레이어링 계약

llama.cpp 어댑터가 자기 큐를 배치로 소비하는 방식을 층으로 나누는 설계
계약이다. 2026-08-30의 4노드 gemma-4-E2B 실측과 코드 감사를 근거로 하며,
P4 코어(프로토콜·에이전트·서비스)는 이 문서의 어떤 개념도 알지 못한다.
배치는 어댑터의 사정이고, OUTER는 소켓 클라이언트로서 계획을 텍스트로
전달할 뿐이다.

## 근거 관측

| 관측 | 값 | 함의 |
| --- | --- | --- |
| 스텝 시간이 행 수와 무관 | 12.8행 105.1ms, 18.2행 96.1ms | 고정비(cut-set 전송)가 스텝을 지배 |
| 홉당 cut-set 폭 | gemma-4: 31/27/23 텐서, 스텝당 81 전송 | 모델별 상수. Qwen 계열은 1 |
| 혼합 물리 배치 | 수천 개 중 0~2개 | 파이프라인 깊이 1이 원인. 전략 평가 자체가 불성립 |
| 노드별 KV (동일 n_ctx) | 173.5 / 63.3 / 157.7 / 126.1 MB | 셀 단가는 노드별. 병목은 가장 비싼 노드 |
| compute buffer | 1,412MB@ubatch512 ↔ 386MB@128, KV의 8배 | 배치 폭이 VRAM 지배 knob |
| 40요청 슬롯 재사용 | `output token positions are not contiguous` | 원장의 불변식 검출 대상. 원인 미규명 — 규명·수정은 계획 P1b |
| KV 영속화 능력 | `kv=0` (`--kv-root` 미지정) | Persist/Restore 구현은 있으나 꺼짐 |
| SWA V 할당 | v_trans 시 256폭을 512로 확보 | 레이어별 과할당, 별도 수정 대상 |

## 전략이 지켜야 할 불변식

1. **상주 전제**: 행 (s, p)는 s의 0..p-1 KV(및 모델별 보조 상태)가 **모든
   스테이지에** 상주할 때만 유효하다. 배치에 토큰이 있어도 KV에 없으면
   무용하다.
2. **증명 없는 제출 금지**: 디코드 실패는 `hop_memory_dirty_`를 세워
   재적재를 요구한다. 시도-후-철회 전략은 불가능하다.
3. **in-flight 불변**: 제출된 멤버십은 전 노드에 복제된다. 자원을 쥔 큐
   항목은 완료 또는 Persist로만 회수한다.
4. 준비된 디코드 행은 강제다(이미 셀을 쥐고 있다). 폭은 1(MTP 3~4).
5. verify/replay 원자 창은 분할 불가, 해소 전 후속 UBATCH 금지.
6. recurrent/hybrid는 등폭. 디코드 1행이 폭을 1로 붕괴시킨다.
7. 프리필 행은 소유 노드 전부에 목적지 셀이 있어야 한다. 예산은 가장
   빡빡한 노드의 남은 셀이다.
8. Restore는 전부-아니면-전무, 단독 실행, (깊이 1인 동안) 배타적이다.
9. 배치 내 sampling/lora 정체성 균일(`compatibility`).
10. 모든 행·복원은 `load_generation`에 결속된다. 세대 교차 복원 금지.

## 레이어

```
L0  큐          (기존) 도착·보류. 정책 없음.
L1  원장        ID 매핑, 상주 상태, 셀 회계의 단일 진실
L2  수용·점유    분 단위: 수용 / 축출(Persist) / 복원(Restore) 결정
L3  구성        스텝 단위: demand → allocation. 모델군별 전략 모듈
L4  증명        제출 전 형태 증명 + 멤버십 캡처 (기존 강화)
L5  전송        (기존) cut-set 전송, 하류 재생. 효율화 대상
```

### L1 원장 (ledger)

무엇이 어디에 있는지에 대한 단일 진실. 다른 모든 층은 원장을 읽고,
상태 전이는 원장만 쓴다.

- ID 매핑: `request_id → SessionKey → adapter sequence_id → 노드별 local
  llama_seq`. OUTER `request_id`는 요청마다 새롭고, 세션 연속성은
  SessionKey가 진다.
- 스테이지별 상주 상태: `Resident{pos} | Persisting{op} | Persisted{pos,
  manifest} | Restoring{op} | Absent | Inconsistent`. 배치 적격 조건은
  "전 스테이지 Resident이고 pos 일치"다.
- 셀 회계: GGUF 메타에서 파생한 노드별 셀 단가표(full 1,568B, SWA 784B,
  reuse 0, recurrent 0/cell)와 사용·잔여 셀. 실측 대조 오차 0.1MB.
- 토큰 이력(또는 위치별 해시): 재요청 프롬프트와 영속 KV의 LCP 판정 근거.
- 시퀀스 슬롯 수명: 전 스테이지 release 정산 전 재배정 금지. 40요청
  position 불연속 결함이 이 규칙의 부재를 증명한다.

### L2 수용·점유 (admission)

느린 축(분). 한 번의 결정이 수 분간 셀을 묶는다.

- 수용: `빈 슬롯 ∧ 가장 빡빡한 노드의 남은 셀 ≥ 프롬프트+max_tokens`.
  오버커밋 비율(출발점 0.5)은 셀 수에만 적용하고 단가에는 적용하지
  않는다. 단가는 계산 가능한 값이지 추정 대상이 아니다.
- 축출: 유휴 ≥ TTL(예: 30분)인 세션에 Persist를 발행하고 셀을 회수한다.
  재방문 확률이 낮은 것부터. 잘못 축출하면 복원 정지 비용으로 되갚는다.
- 복원: 재요청의 SessionKey가 Persisted에 매칭되면 Restore를 스케줄한다.
  전량 셀 선확보, 단독 실행. 복원 후 초과 suffix만 프리필.
- 공정성: ITL은 같은 UBATCH 동승으로 자동 공정하다. TTFT가 수용 정책의
  산물이다(실측: parallel 10→40에서 TTFT p50 207.9s→1.4s). 셀이 남는 동안
  "많이 받는 것"이 속도와 공정을 동시에 만족하며, 충돌은 셀 고갈
  시점에만 생긴다. 그때의 우선순위는 축출 > 대기 > 거절이다.

### L3 구성 (strategy)

스텝 축(~100ms). 순수 함수로 유지한다 — 입력은 전부 데이터, 출력은
할당. 그래야 기록된 트레이스로 재생·검증할 수 있다.

```
plan(demands,            // 원장이 상주 전제를 통과시킨 것만
     row_budget,         // n_batch / n_ubatch
     cell_budget,        // 가장 빡빡한 노드의 남은 셀
     pending_restores,   // 단독 실행이라 스텝을 통째로 가져감
     shape_rules,        // HELLO 협상값
     cost_model)         // 모델별 고정비·행당비
  → allocations
```

전략은 trait 뒤의 모델군별 모듈이다. 선택 키는 HELLO capability와 GGUF
메타(memory family, equal_sequence_ubatch, swa, shared_kv, nextn …)이며
모델명 하드코딩은 금지한다.

- `waterfill` (기본 attention+unified): 디코드 전원 선점 → 대기 프롬프트
  1행씩 → 회전 커서 잔여 채움. 현행 `plan_ordinary`가 이것이다.
- `equal_width` (recurrent/hybrid): 등폭 강제. 디코드가 폭을 1로
  붕괴시키므로 프리필 전용 스텝과 디코드 전용 스텝을 분리하는 편이 낫다.
- `atomic` (MTP/speculative): 원자 창 + 펜스. 다른 전략과 합성된다.
- 향후: index-aware(DSA/MSA/DSV4), paged/radix 계열. vLLM PagedAttention과
  SGLang RadixAttention은 참조 대상이나, 노드별 셀 단가가 다르다는 조건이
  우리 고유의 추가 제약이다.

채움 규칙은 "꽉 채운다"가 아니라 **"고정비 대비 한계 이득이 양수인 동안
채운다"**로 적는다. 지금은 고정비가 지배하므로 꽉 채우는 것과 같고,
cut-set 합치기·깊이 개선 후에는 자동으로 프리필 상한이 SLO에 걸린다.

### L4 증명 (proof)

제출 직전, 논리 배치가 llama.cpp의 split 규칙 아래 **정확히 예측된
물리 UBATCH**로 쪼개짐을 증명한다. 기존 검사((seq,pos) 일대일, 등폭,
verify 펜스)에 셀 예산과 예상 분할 수 검증을 더한다. 통과하면 제출하고
ubatch 콜백 멤버십을 캡처하며, 하류는 재도출 없이 재생한다.

### L5 전송 (기존, 효율화 대상)

전략의 소관이 아니나 비용 모델의 최대 항이다. 두 가지가 고정비를
직접 줄인다: 홉당 cut-set 텐서를 연속 버퍼 하나로 합쳐 전송하는 것,
통과만 하는 텐서를 매 홉 재전송하지 않는 것.

## 영속화 프로토콜

`p4-adapter`의 `CacheAction`(PreparePersist/Persist/PrepareRestore/
Restore/Reconcile)과 `CacheReceiptState`가 이미 이 구조를 계약한다.
세션 상태는 노드 4개에 조각나 있으므로(`stage_id`, `operation_id`,
`generation`) 모든 영속·복원은 다단계 조율이다.

```
Persist:  L2 victim 선정 → PreparePersist ×4 → 전원 Prepared → Commit
          → 각 노드 seq_rm → 원장 Persisted{pos}, 셀 반환
Restore:  재요청 SessionKey 매칭 + 토큰 이력 LCP → 셀 선확보 →
          PrepareRestore ×4 → pos 검증 ×4 → Commit → Resident
          → suffix만 프리필
실패:     수렴은 [kv-state-store-convention.md](kv-state-store-convention.md)의
          2PC 수렴 규칙이 단독 소유한다. 여기서는 재서술하지 않는다.
```

현행 구현(`llama_stage_runtime_kv.cpp`)은 저장 후 `seq_rm`, 복원 후
`llama_synchronize` + 위치 대조까지 갖췄다. 남은 결함: 노드당 128MB
상한(장문 세션은 청크 persist 필요), `--kv-root` 미지정으로 `kv=0`.

## 모델 다양성: 2축 게이트

전략 모듈 등록 조건은 두 감사의 교집합이다.

- 축 A — 스테이지 분할: `linkcpp_stage_residency_supported` 옵트인.
  현재 OPT-IN: kv_cache, kv_cache_iswa, memory_hybrid, memory_recurrent.
  DENIED: msa, dsa, dsv4, hybrid_iswa.
- 축 B — unified 시퀀스 분리: KV는 KQ mask로 분리되지만 보조 상태
  (인덱서 등)가 position만 키로 쓰면 시퀀스 간 충돌한다(qwen4exp 사례).
  KV 외 상태를 가진 모델은 이 감사를 별도로 통과해야 한다.

미통과 모델은 지금처럼 로드 시점 fail-closed로 거부된다. 이 게이트가
전략 계층을 "모든 모델을 덮는 하나의 휴리스틱" 강박에서 해방시킨다.

## llama.cpp 업데이트 내성

전략 크레이트는 llama.cpp에 링크하지 않는다. 보는 것은 세 가지뿐이다:
HELLO 협상값, GGUF 파생 셀 단가표, 캘리브레이션 상수. llama.cpp가
풀업데이트되어 compat patch set이 갈리면 **값이 갈리고 코드는 갈리지
않는다**. 배치 위치는 `layers/adapters/llamacpp/batching/`(신규 크레이트,
의존성 최소) — staged adapter가 소비하고, 기록된 `batch_observations`
아티팩트를 재생하는 골든 테스트로 GPU 없이 검증한다.

## 실패 이력의 층별 귀속

| 관측된 실패 | 귀속 층 |
| --- | --- |
| 40요청 position 불연속 (슬롯 재사용) | L1이 불변식 위반을 검출; 원인 규명·수정은 계획 P1b |
| 혼합 배치 0건 | 전제 — 파이프라인 깊이 1 |
| gemma-4 로드 거부 후 5GB 로드 낭비 | L1 단가표 + 로드 전 preflight |
| Qwen3.6 VRAM 초과 사후 발견 | L1 단가표 (로드 전 계산) |
| cut-set 81전송 고정비 | L5 + cost_model |
| compute buffer 과할당(점유 3.5%) | 계획 시점 n_ubatch (OUTER 입력) |
| 셀 고갈 시 무작위 세션 실패 | L2 — unified에 격리가 없으므로 정책이 격리 |

## 도입 순서

단계·순서·수용 기준은 [adapter-restructure-plan.md](adapter-restructure-plan.md)가
단독 소유한다. 이 문서는 L0~L5 계약만 소유하며 순서를 재서술하지 않는다.
