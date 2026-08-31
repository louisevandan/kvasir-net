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
11. **스냅샷 정합 펜스**: Checkpoint·Persist·Fork는 대상 시퀀스의
    in-flight 행이 전무하고 전 스테이지가 정산된 정지점에서만 실행된다 —
    배치 도중 export된 스냅샷은 position이 모호한 오답이다. 캐시 연산은
    단독 실행 배리어로서 스텝 일정과 경쟁하며, 스케줄러는 OUTER 명령
    순서를 보존하되 삽입 지점(현재 스텝 종료 후)을 선택한다. 정지점의
    증거는 fragment credit이 아니라 **stage별 `SequenceQuiesced` attest**다
    — credit 반환은 "peer가 인수했다"이지 compute/KV 완료 증거가 아니다
    (plan.md §3; O9).

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
- 축출·체크포인트: L2는 **정책을 갖지 않는다.** 언제 어떤 세션을 어떤
  키로 영속화·체크포인트·폐기할지는 전부 OUTER의 명령이고(스냅샷 명령
  모델은 [kv-state-store-convention.md](kv-state-store-convention.md)
  소유), L2는 명령의 이행과 그 셀 회계만 담당한다. TTL은 OUTER가 이
  어휘로 표현하는 정책 중 하나일 뿐이다.
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
     pending_cache_ops,  // 복원·영속·체크포인트·분기: 단독 실행 배리어라
                         // 스텝을 통째로 가져가고, 대상 시퀀스 펜스를 요구
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

## 측정: 배치 폭 대 파이프라인 깊이 (2026-08-31)

도착 위상 파편화와 그 교정을 4노드 하네스로 A/B 측정했다. 결과는 이 계약의
채움 규칙을 실제로 정정한다.

**관측된 결함**: 배치는 그 순간 준비된 시퀀스로만 편성되므로, 함께 도착한
요청 무리가 영원히 같은 멤버로 재편성된다. 연속 도착(5초마다 2건, 24 병렬)
실행에서 4,483개 물리 배치가 **고유 멤버십 집합 38개**에 불과했고 폭은
2.84행이었다. 웨이브 도착(20/10/10) 실행에서는 3,000개 배치가 정확히
**3개 집합**(20행 1000회, 10행 2000회)으로 라운드로빈했다. 서로 다른 시각에
태어난 무리는 상대가 in-flight인 동안 준비되므로 결코 만나지 않는다.

**교정**: 스케줄러에 병합 임계값(`P4_STAGED_MIN_BATCH_ROWS`)을 넣어 충분한
행이 모일 때까지 계획을 미룬다. 의도한 대로 작동했다 — 연속 도착에서 폭
2.84 → 10.56행, 물리 배치 4,483 → 1,205(-73%), 혼합 배치 2 → 19.

**그러나 처리량은 손해였다.**

| 시나리오 | 정책 | rows/batch | ms/batch | 생성 TPS | 의미 판정 |
| --- | --- | --- | --- | --- | --- |
| 연속 도착 | 기본 | 2.84 | 24.5 | **108.9** | 40/40 |
| 연속 도착 | 임계값 8 | 10.54 | 102.3 | 96.9 | 40/40 |
| 연속 도착 | 전량 병합 | 10.56 | 103.2 | 96.2 | 40/40 |
| 웨이브 | 기본 | 13.57 | 68.2 | **195.4** | 40/40 |
| 웨이브 | 임계값 40 | 30.41 | 213.5 | 139.8 | 40/40 |

행이 3.7배 늘 때 스텝 시간이 4.2배(연속), 2.2배 늘 때 3.1배(웨이브) 늘었다.
**이 폭 구간에서 스텝 비용은 고정비가 아니라 행 수에 지배된다.** 따라서 폭을
넓혀도 상환할 고정비가 없고, 잃은 파이프라인 깊이만큼 손해다.

**계약 정정**: 앞서 이 문서가 "고정비가 지배하므로 빈 행 슬롯은 순수한
낭비"라고 적었던 것은 서로 다른 실행을 비교한 결과였고, 동일 조건 A/B는 그
반대를 보인다. 채움 규칙 "고정비 대비 한계 이득이 양수인 동안 채운다"는
유지되지만, **현재 gemma-4 4노드 구성에서 그 한계 이득은 음수**이므로 기본값은
병합하지 않는 것이다. 임계값은 계약으로 남긴다 — cut-set이 훨씬 넓은 모델,
스냅샷 정합 펜스(불변식 11), 등폭 강제 계열에서는 부호가 바뀔 수 있고,
그때 재측정으로 결정한다.

**남는 진짜 레버**: 처리량은 대략 `활성 시퀀스 수 / 왕복 지연`이다. 폭도
깊이도 그 곱을 바꾸지 못하므로, 개선은 (a) 활성 시퀀스를 늘리거나 (b) 왕복
지연 자체를 줄이는 것 — 즉 홉당 cut-set 전송 비용(D7/P6)이다.

## 동시성의 세 축

llama.cpp의 `-np`는 역사적으로 KV 분할 수·server slot 수·동시 추론 폭을
한 숫자에 묶었고, unified KV가 첫 번째 의미를 지운 뒤에도 나머지 둘은
묶여 있다(upstream discussion 22401이 정확히 이 분리를 요구한다). 이
계약은 처음부터 셋을 분리한다.

| 축 | 뜻 | 소유 |
| --- | --- | --- |
| `kv_capacity` | 공유 셀 풀의 논리 용량(n_ctx) | 저장 규약·OUTER 계획 |
| `max_resident_sequences` | KV에 상태를 살려 둘 수 있는 세션 수 | L1 등록부 + L2 수용. 요청값은 코디네이터, 물리 상한은 스테이지별 교집합(아래) |
| `decode_parallelism` | 이번 스텝의 UBATCH에 태울 시퀀스 수 | L3가 스텝마다 선택. runnable 수와 행 예산이 상한 |

KV 상주와 연산 동시성은 별개의 차원이다: 20개 세션이 상주해도 스텝에는
4개만 태울 수 있고, 노드는 전달된 membership만 실행한다(멤버십 재생
불변식). 다만 소유는 두 층으로 갈린다 — **요청값**은 코디네이터가 정하고
**물리 상한**은 스테이지마다 다르다:

```
effective_resident     = min_i( stage_i의 상주 용량 )
effective_decode_width = min_i( stage_i의 n_ubatch·backend 상한, edge credit )
```

CUDA 3090 스테이지와 Metal/CPU 스테이지를 같은 값으로 간주할 수 없다.
또한 `n_seq_max`는 백엔드가 발견해 보고하는 고유 capability가 아니라
OUTER가 컨텍스트 생성 시 설정한 **구성값**이며, HELLO는 그것을 되돌려줄
뿐이다 — 발견이 아니라 계약의 echo다.

단서 두 가지가 실측에서 나왔다. 첫째, `max_resident`는 공짜가 아니다 —
SWA 캐시 셀 수는 `n_swa×n_seq_max+n_ubatch`로 이 값에 비례하고(실측
12,800셀), sampler graph 메타데이터 예산도 활성 시퀀스에 비례한다(256세션
실험의 368바이트 부족 사건). 상주 한도는 셀 회계에 자기 비용 항을 갖는다.
둘째, 현행 OUTER 플랜은 `parallel` 하나로 `--n-seq-max`와 스케줄러 폭을
함께 정한다 — 분리는 계획 P3의 수용·점유 구현에서 일어난다.

고정 slot 수를 없앤 "상주 시퀀스 등록부 + runnable 스케줄러"가 unified
KV의 논리적 종착점이며, 그것이 정확히 L1과 L3다.

## 영속화 프로토콜

`p4-adapter`의 `CacheAction`(PreparePersist/Persist/PrepareRestore/
Restore/Reconcile)과 `CacheReceiptState`가 이미 이 구조를 계약한다.
세션 상태는 노드 4개에 조각나 있으므로(`stage_id`, `operation_id`,
`generation`) 모든 영속·복원은 다단계 조율이다.

영속·복원·절단의 실행 순서(복원 판정 사다리 포함)와 2PC 수렴 규칙은
[kv-state-store-convention.md](kv-state-store-convention.md)가 단독
소유한다. 이전 판이 여기 두었던 흐름 요약이 규약과 어긋나게 낡는 것이
7차 리뷰에서 확인되어, 요약 자체를 제거했다.

현행 구현(`llama_stage_runtime_kv.cpp`)은 저장 후 `seq_rm`, 복원 후
`llama_synchronize` + 위치 대조까지 갖췄다. 남은 결함: 노드당 128MB
상한(장문 세션은 청크 persist 필요), `--kv-root` 미지정으로 `kv=0`.

## 모델 다양성: 3축 게이트

전략 모듈이 존재할 조건은 세 감사의 교집합이다.

- 축 A — 스테이지 분할: `linkcpp_stage_residency_supported` 옵트인.
  현재 OPT-IN: kv_cache, kv_cache_iswa, memory_hybrid, memory_recurrent.
  DENIED: msa, dsa, dsv4, hybrid_iswa.
- 축 B — unified 시퀀스 분리: KV는 KQ mask로 분리되지만 보조 상태
  (인덱서 등)가 position만 키로 쓰면 시퀀스 간 충돌한다(qwen4exp 사례).
- 축 C — backend conformance: llama 추상층 아래의 구상 백엔드(CPU/CUDA/
  Metal/…)가 `{memory_family × backend}` 조합에서 load, cut 텐서
  alias/view, batch split, Persist/Restore, TrimTo, **수치 동등성**을
  통과해야 한다. bit-for-bit logits 동일성은 llama.cpp 자신도
  backend·배치 구성 간에 보장하지 않으므로, 판정 가능한 기준으로
  정의한다: 고정 모델·프롬프트·seed에서 dtype/backend별 NMSE와
  절대/상대 오차 한계, greedy 토큰열(또는 top-k 순서) 일치. 같은
  backend의 Persist→Restore 왕복과 cross-backend 이동은 서로 다른 별도
  기준을 갖는다. 추상층이 같아도 구상 백엔드의 버퍼 레이아웃과
  연산 경로는 다르고, 이 동등성은 public llama.cpp 계약이 아니다.
  CPU는 매 pin 필수, production 백엔드는 승격 전 필수(계획 U0).

미통과 조합은 지금처럼 로드 시점 fail-closed로 거부된다.
## llama.cpp 업데이트 내성

두 층을 구분한다. "pull 후 값만 바뀌고 코드는 안 바뀐다"는 주장은 아래
첫 층에만 성립한다.

- **정책 계층(전략·원장·수용)**: llama.cpp에 링크하지 않고 HELLO 협상값,
  GGUF 파생 단가표, 캘리브레이션 상수만 본다. upstream이 갈리면 값이
  갈리고 이 코드는 갈리지 않는다.
- **native compat 계층**: llama core 내부(context·graph·memory·loader)를
  패치하므로 갱신은 값 변경이 아니라 **매 pin 의미 기반 rebase**다.
  d7a207411→d7bd3bfc dry-run에서 24개 패치 중 5개가 충돌했다(5차 리뷰
  관측: model-loader header, public API impl, stage/recurrent residency,
  MTP tail). 이 비용은 per-pin 호환성 게이트(공식 prepare + conformance)가
  소유하고, 패치 큐는 stage hook / 독립 upstream fix / 모델·speculative
  feature 포트의 3분할로 관리해 독립 수정 하나의 upstream 흡수가 전체
  포팅과 함께 충돌하지 않게 한다(계획 U0).

배치 위치는 `layers/adapters/llamacpp/batching/`(신규 크레이트, 의존성
최소) — staged adapter가 소비하고, 기록된 `batch_observations` 아티팩트를
재생하는 골든 테스트로 GPU 없이 검증한다.
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
