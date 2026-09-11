# 어댑터 배치 레이어링 계약

> 문서 지위 (2026-09-06): **분야 계약·구현과 구별**. 소유 분야의 계약/목표를 읽되 구현 완료로 간주하지 않는다. 현재 개발 순서와 충돌하면 로드맵의 명시적 이관을 따른다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

llama.cpp 어댑터가 자기 큐를 배치로 소비하는 목표 레이어 계약이다. 이 문서의
L0~L5가 전부 구현됐다는 뜻은 아니다. 현재 코드 상태·단계는
[분산 배치 로드맵](distributed-batching-roadmap.md), 실행 검증은
[검증 규약](distributed-batching-verification.md)이 소유한다.
OUTER는 모델·토폴로지·SLO·요청 도착·스냅샷 트리거를 정하고 어댑터는 적격 행과
배치를 구성한다. P4 코어에 llama 전용 배치/KV 규칙을 넣지 않는다.
P4 전체와 native/llama/backend의 허용 의존·타입·빌드 경계는
[계층 격리 계약](layer-isolation-contract.md)이 소유한다. 아래 L0~L5는 그 안의 배치 의미론이다.

## 과거 관측 — 현재 상태나 병목의 확정이 아님

| 관측 | 값 | 함의 |
| --- | --- | --- |
| 스텝 시간이 행 수와 무관했던 표본 | 12.8행 105.1ms, 18.2행 96.1ms | 고정비 후보; 전송/계산/샘플링/대기를 분해하기 전 원인 미확정 |
| 홉당 cut-set 폭 | gemma-4: 31/27/23 텐서, 스텝당 81 전송 | 모델별 상수. Qwen 계열은 1 |
| 혼합 물리 배치 | 당시 수천 개 중 0~2개 | 멤버십 합류 관측; 이것만으로 pipeline 깊이를 판정하지 않음 |
| 노드별 KV (동일 n_ctx) | 173.5 / 63.3 / 157.7 / 126.1 MB | 셀 단가는 노드별. 병목은 가장 비싼 노드 |
| compute buffer | 1,412MB@ubatch512 ↔ 386MB@128, KV의 8배 | 배치 폭이 VRAM 지배 knob |
| 40요청 슬롯 재사용 | `output token positions are not contiguous` | 원장의 불변식 검출 대상. 원인 미규명 — 규명·수정은 계획 P1b |
| KV 영속화 능력 | `kv=0` (`--kv-root` 미지정) | Persist/Restore 구현은 있으나 꺼짐 |
| SWA V 할당 | v_trans 시 256폭을 512로 확보 | 레이어별 과할당, 별도 수정 대상 |

## 전략이 지켜야 할 불변식

1. **상주/의존 전제**: 행 (s,p)가 각 스테이지에서 실행되기 전에 그 스테이지의
   필요한 prefix KV·보조 상태가 유효해야 한다. 여러 prefill fragment의 동시 비행은
   선행 fragment의 stage별 실행 순서를 증명해야 하며, 모든 행의 발행 전에 전 pipeline을 비우라는 뜻이 아니다.
2. **증명 없는 제출 금지**: 실행 실패 후 KV가 보존됐다고 가정하지 않는다.
   native의 dirty/불확실 결과는 격리·복구 계약을 따른다. speculative한 시도 후 로컬 counter만 철회하는 것은 금지다.
3. **in-flight 불변**: 제출된 멤버십과 위치를 발행 기록에 결속한다. 완료·취소·실패의
   정산/해제 증거 없이 자원을 회수하지 않는다. Persist가 진행 중 compute의 해제 증거를 대신하지 않는다.
4. 준비 decode는 bounded service를 받아야 하지만 capacity를 넘는 전원을 매 배치에 넣을 수는 없다.
   일반 decode의 의존 행은 1개이며 speculative/verify의 폭과 원자성은 capability와 실제 proposal에 따른다.
5. verify/replay 원자 창은 합의된 단위로만 처리하고 해소 전 **같은 시퀀스의 의존 후속 작업**을 금지한다.
   무관한 시퀀스까지 전 pipeline barrier로 막는 규칙은 아니다.
6. `equal_sequence_ubatch`를 요구하는 memory 계열은 등폭을 지킨다. 디코드 1행과
   긴 프리필을 무조건 동승시켜 전체 프리필 폭이 1로 붕괴하지 않게 한다.
7. 프리필 행은 소유 노드 전부에 목적지 셀이 있어야 한다. 예산은 가장
   빡빡한 노드의 남은 셀이다.
8. Restore는 전 stage의 검증/장벽 후에만 runnable로 공개한다. 부분 실패 수렴은 저장 규약을 따르며,
   대상 시퀀스 quiescence와 native context의 실제 배타 요구를 구분한다.
9. 배치 안의 engine/model/LoRA/shape 호환성을 대조한다. 현행 `compatibility`가 더 엄격하게 묶는
   sampler 옵션을 완화하려면 시퀀스별 sampler 독립성과 정상 출력 시험이 먼저다.
10. 실행과 반환은 load/sequence generation에 결속한다. 영속 레코드의 재적재 허용 여부는
    실행 세대와 별개인 저장 정체성·호환 행렬로 결정한다([저장 규약](kv-state-store-convention.md)).
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
L2  수용·점유    요청 수용/예약과 OUTER의 영속·복원 명령 이행
L3  구성        스텝 단위: demand → allocation. 모델군별 전략 모듈
L4  증명        제출 전 형태 증명 + 멤버십 캡처 (기존 강화)
L5  전송        (기존) cut-set 전송, 하류 재생. 효율화 대상
```

### L1 원장 (ledger)

무엇이 어디에 있는지에 대한 단일 진실. 다른 모든 층은 원장을 읽고,
상태 전이는 원장만 쓴다.

- ID 매핑: 외부 request ID·SessionKey·adapter incarnation·노드별 slot은 별도 축이다.
  SessionKey는 대화 연속성, incarnation은 한 번의 실행 소유권이다. 완료 후 같은 request ID와
  slot의 재사용을 금지하는 것으로 늦은 메시지 문제를 우회하지 않는다. 아래 실행 소유권 계약을 따른다.
- 스테이지별 상주 상태: `Resident{pos} | Persisting{op} | Persisted{pos,
  manifest} | Restoring{op} | Absent | Inconsistent`. 각 stage의 완료 frontier와 비행 중 구간을 함께 보존한다.
  정상 wavefront에서는 stage별 pos가 다를 수 있다. 발행마다 전 stage pos 일치를 요구해 pipeline을 직렬화하지 않는다.
  해당 stage 실행 전에 필요한 prefix/보조 상태가 유효하고 선행 작업 순서가 보장되는지를 검사한다.
- 셀 회계: 모델·cut·backend/layout별 파생/실측 단가와 used/reserved/free를 결속한다.
  recurrent가 token-cell 방식이 아니더라도 보조 state/buffer bytes는 별도로 예약하며 0 메모리로 해석하지 않는다.
- 토큰 이력(또는 위치별 해시): 재요청 프롬프트와 영속 KV의 LCP 판정 근거.
- 시퀀스 슬롯 수명: 전 스테이지 release 정산 전 재배정 금지. 과거 40요청
  position 불연속은 재현·원인 감사 대상이지 이 규칙 부재의 인과 증거가 아니다.

#### 승인된 출력의 발행자

현재 adapter OUTPUT 계약에서 꼬리는 native 결과를 head에 반환하고, head의 원장 검증·commit 뒤에만
OUTER 출력이 발행된다. 따라서 OUTPUT의 envelope source는 정산을 소유한 **configured first endpoint**다.
OUTER는 agent 주소·node ID·node generation을 포함한 해당 endpoint 전체와 load/session/request,
target/return route·correlation·position을 대조한다. 꼬리도 configured node라는 이유로 동시 허용하지 않는다.
native 계산 위치와 출력 승인 권위를 구분하며, broker가 llama 토큰을 해석하거나 source를 바꾸지 않는다.
별도 head 승인 receipt 없이 꼬리 직접 출력으로 돌아가는 것은 이 계약의 우회다.

생산자/소비자 회귀는 같은 의미의 wire fixture를 양쪽 실제 경로에 결속한다. 현재 worker가 만들어내는
출력도 대조하지 않는 고정 fixture 소비 시험만으로 경계 일치를 승인하지 않는다. 불안정한 envelope 필드를
투영에서 뺄 때의 조건·부정 시험은 검증 규약 T20을 따른다. 이 source 대조 자체는 사용자 수신 ACK나
model 품질·수량·해제 멤버십의 완전한 검증이 아니다.

#### OUTER의 sampled 출력 예산과 fresh-prefill 관측 대조

`tools/event-drive/src/run/output_budget.rs::validate_output`은 제출 max_tokens를 sampled OUTPUT
개수에 적용한다. 빈 text의 EOS도 한 개이며, 상한에서 terminal이 필요하고 length는 정확히 상한에서만
성립한다. stop/eos는 상한 이내 조기 종료가 가능하고 다른 종료 문자열은 자동 승인하지 않는다.
`inference.rs::drive`는 요청 응답/토큰 목록에 추가하기 전에 검사한다. 최종 acceptance도 보존된 전체
outcome을 다시 검사하며, 응답 절단이나 EOS 삭제로 상한 위반을 감추지 않는다. 프로토콜상 정상인
빈 EOS 하나의 출력은 비어 있지 않은 정상 응답/최소 생성량이라는 별도 품질 게이트를 만족하지 않는다.

현재 event-drive는 위치 0에서 시작하는 새 PREFILL 요청을 실행한다. `inference_evidence.rs::apply_observations`가
서로 다른 프롬프트의 승인된 관측을 요청별로 집계하고 첫 OUTPUT 위치와 대조한다. 동일 observation ID의
동일 payload는 한 번만 세고, 다른 observation ID로 같은 physical execution을 재사용하면 거부한다.
전체 요청 후보의 검증과 checked 합산이 끝난 뒤에만 artifact 행 수를 설치한다. OUTPUT 뒤에 오는 관측도
완료/해제 경계 전이면 허용하지만, 그 경계에서 관측이 없거나 위치와 다르면 성공 artifact를 반환하지 않는다.
execute에서 다시 합산하지 않는다. 선택적 공통 expected_prefill_rows는 추가 workload 조건이지 이 대조의 대체가 아니다.
이 관측은 head가 보고한 fresh-prefill 작업량이며 별도 native tokenizer/KV 증명, 재개/Restore의 원점,
OUTER 실시간 사용자 전달 ACK 또는 아래 별도의 해제 멤버십 원장을 대신하지 않는다.

#### 해제 완료 권위와 소유자별 통지 — 현재 run의 정상 종료만 부분 구현

`worker/release.rs::Worker::tail`/`released`, `worker/effects.rs`, `completion.rs`와 OUTER의
`run/inference.rs`/`release_ledger.rs`를 함께 이관했다. 아래는 후속 미커밋 작업 트리의 **정상 sampled
stop/eos/length 종료, 같은 run 수명** 계약이다. OUTPUT 없는 실패·Cancel, 재시작·재연결·내구 전달,
전체 다중 OUTER 관측까지 완료한 것은 아니다. P4 broker는 이 모델 의미를 소유하지 않는다.

**현재 SESSION wire** (`commands.rs::SessionCommand`, `worker/control.rs::Worker::session`,
`worker.rs::Worker::require_stage_source`, 후속 미커밋 작업 트리):

- SESSION/SESSION_READY content-type은 **v4**다. SESSION은 `load_generation`, `session_id`,
  전체 순서 `stages: [{agent,node,generation}, ...]`, 수신 노드의 `stage_index`를 필수로 받는다.
  별도의 role/first/next 선언은 제거했으며 최상위 unknown 필드와 구 v3 명령은 거부한다.
- 노드 주소·nonzero generation·중복 없는 정체성·유효한 index를 검사하고, index의 전체 endpoint와
  실제 worker 및 envelope target이 같아야 설치한다. 같은 agent/node의 서로 다른 generation도
  동시에 다른 stage로 선언할 수 없다. 이미 설치된 같은 session의 다른 선언은 거부한다.
- first/previous/next/terminal은 설치한 순서에서만 파생한다. PHYSICAL/RELEASE/SETTLE의 source는
  previous, TAIL/RELEASED/SETTLED의 source는 terminal의 전체 endpoint여야 하며 target도 실제 worker여야 한다.
  검사는 원장/KV/슬롯/출력 변경 전에 수행한다. 특히 head의 next와 terminal은 3-stage에서 다르다.
- `tools/event-drive/src/run/mod.rs::session_events`는 실제 실행 루프가 사용하는 생산 경로이며
  모든 노드에 같은 순서와 자기 index를 보낸다. 수신 노드별 검증만으로 fleet 전체가 같은 선언을
  받았다는 합의, 사용자/네트워크 인증, process 재시작 후 freshness를 증명하지 않는다.
- 현재 stage 경로는 별도 head/tail이 필요한 **2개 이상**만 지원한다. 이는 단일 stage 새 구현이
  없다는 제한이지 장치당 노드 수나 필요한 모델/KV 노드를 줄이라는 배치 정책이 아니다.

**현재 OUTER wire와 처리 순서**:

- `ApprovedOutputPayload`의 OUTPUT content-type은 **v4**다. 기존 OutcomePayload 필드에
  `submission_event_id`, nonzero `incarnation`, 선택적 `release_operation_id`를 더했다. terminal에만
  nonzero operation이 있고 nonterminal에는 없다. operation은 head가 RELEASE를 만들 때 정하며,
  출력 전에 후보 전체를 검증한다. OUTPUT 승인 자체는 전 stage KV 해제 완료가 아니다.
- OUTER 통지는 별도 **release-receipt-v1**의 `ReleaseReceipt {load_generation, session_id, members}`다.
  member는 `{request_id, submission_event_id, sequence_id, incarnation, operation_id}`이며 빈 목록,
  중복 request/slot, 잘못된 신원과 unknown 필드를 거부한다. 내부 RELEASED v4는 여전히 stage 간
  ReleaseCommand ACK이고 OUTER receipt가 아니다. scalar-only DTO는 제거했고 구 OUTPUT v3·
  scalar/internal ACK를 현재 OUTER 완료 증명으로 소비하지 않는다.
- 실제 `send_wave`는 송신할 PREFILL event ID를 **send 전에** 등록한다. 송신 실패는 해당 run의 실패이며
  같은 attempt/sequence로 되감지 않는다. head PREFILL은 source가 원본 return_route의 OUTER와 같고
  target이 실제 head인지 기억/수용 전에 검사한다. 이 필드 대조를 peer 인증으로 부르지 않는다.
- OUTER는 기존 route/예산/위치 검증을 통과한 첫 OUTPUT으로 slot/incarnation을 고정하고, terminal
  OUTPUT에서만 release member 기대를 보존한다. receipt가 먼저 오면 거부한다. 새로운 무제한
  reorder queue로 순서를 감추지 않는다. receipt 전체 대조가 끝난 뒤에만 신규 해제를 반영한다.
- fresh envelope의 정확한 member 재전달은 신규 해제 **0**이다. 같은 event ID 재전달은 기존 OUTER
  envelope 정책대로 거부한다. 변경된 replay와 A 정상/B 무효는 전량 거부한다. 현재 event-drive의
  correlation은 제출 request ID이며 receipt의 member에 속해야 한다. generic 어댑터는 임의의 정상
  correlation을 보존하므로 이 OUTER 구현의 선택을 P4 전체 문법으로 올리지 않는다.
- `RequestArtifact`는 실제 송신 attempt, terminal에서 보존한 기대 member, receipt 반영 여부를 남긴다.
  최종 acceptance의 일치 검사는 보존 artifact의 일관성 재검사다. 원시 receipt 전체 재검증이나
  외부의 서명/인증 증명이 아니며, 실제 소비 경로의 검사를 대체하지 않는다.

- **제출 권위**: OUTER가 송신 전에 확정한 request attempt를 저장하고 head가 원본 제출과 결속한다.
  durable session_key, request 문자열, 빈 slot, 처음 받은 receipt는 attempt 권위가 아니다. 현재 envelope
  event_id를 쓸 때에도 발급 OUTER 전체 endpoint/connection generation과 load/session 범위를 포함한다.
  같은 connection generation과 sequence를 새 Sender에서 재사용하면 restart freshness가 없으므로,
  그 수명 문제를 해결하기 전에는 현재 run 안의 결속만 증명했다고 기록한다.
- **계산/해제 권위**: head가 발급·보존한 slot/incarnation과 release operation을 실제 terminal 승인에
  결속한다. OUTER의 해제 기대 신원은 해제 receipt보다 먼저 제출/승인 경계에서 확정한다. receipt의
  operation을 그대로 자신의 기대값으로 설치하는 대조는 금지한다. terminal 전에 해제 통지부터 받는
  순서의 처리 계약도 명시하며, count를 증가시키고 나중에 검증하지 않는다.
- **스테이지 증거**: SESSION에서 독립 확정한 ordered pipeline의 predecessor/successor/first/terminal
  endpoint와 load/session 세대에 내부 control/ACK를 결속한다. 마지막 ACK의 발신 source를 읽어
  terminal을 새로 등록하지 않는다. configured middle 또는 외부 발신자는 pending member를 정확히
  알고 있어도 전 stage 정산 증거를 대신하지 못한다. endpoint 대조는 논리 역할 검증이며 전송 인증의 대체는 아니다.
- **멤버십**: 정상 receipt는 명시된 요청 시도 집합을 운반하고 consumer는 제출/terminal 기대 집합과
  원자적으로 대조한다. A의 중복은 B의 완료가 되지 않는다. unknown/stale/변형 member가 하나라도 있으면
  다른 member의 완료 수·슬롯·예약·효과도 적용하지 않는다. 여러 요청을 묶은 정상 control/receipt는 유지한다.
- **라우팅/효과 수명**: 요청을 resident에서 지우기 전에 pending release에 원래 ReplySpec과 제출 참조를
  보존한다. 필요한 작은 신원/경로 메타데이터를 추출하며 긴 prompt payload 전체를 해제 대기 동안 다시
  보유하지 않는다. 통지는 해당 소유자 전체 route/correlation/deadline으로만 보낸다. 여러 소유자의 native release
  batching과 OUTER 통지 묶음은 다르며 단일 physical base의 route로 통지를 몰지 않는다. 각 통지 의도를
  상태 commit과 함께 보존하고, emit 실패 후에도 미발행 부분이 남아야 한다. KV가 이미 해제됐다는 이유로
  통지 실패를 성공 또는 새 native 재해제 요청으로 바꾸지 않는다. enqueue 성공과 OUTER ACK는 별도다.

`PendingRelease`는 실제 RELEASE identity와 원본 Envelope·ReplySpec만 보존한다. prompt payload를
다시 들고 있지 않다. ACK 후보 전체의 source/member/admission과 원본 source/target/return_route/
correlation/deadline 대조를 끝낸 뒤, **전체 ReplySpec이 같은 경우만** 영수증을 묶는다. 같은 OUTER라도
correlation/deadline이 다르면 별개 통지다. 슬롯 반환·pending 제거와 notification intent를 commit하고,
emit 실패 시 아직 발행하지 못한 intent를 남기고 fence한다. 이미 발행한 통지는 되돌리거나 다시 native
해제하지 않는다. commit 전 ID 의무 검사에서 여력이 부족하거나 합계가 넘으면 반환 후보 전체를
거부하고 요청·원장·예약·효과를 보존한다. 이미 commit된 효과의 발행에서 Full은 동일 송신물
대기·재시도이며, Closed 또는 사후 ID 발급 실패는 미발행 intent 보존·fence다. 사전 거부의
부분 적용이나 재연결 복구로 해석하지 않는다.

같은 connection generation에서 Sender를 다시 만들거나 Worker/load를 재시작하는 freshness는 아직
없다. 동시 동일 request_id를 여러 OUTER가 쓰는 것도 현재 request_key 범위에서 지원하지 않는다.
이것과 OUTPUT 없는 Cancel/실패의 승인, bounded outbox·graceful drain은 목표 계약으로 남는다.
판정/반례는 검증 규약, 작업 순서는 현재 로드맵만 소유한다.

같은 다중 OUTER 배치의 관측도 별도 결속이 필요하다. 이관 전 `worker/observe.rs::Worker::emit_batch_observation`은
모든 ReplySpec에 전체 requests 관측을 보내고, `inference_identity.rs::InferenceIdentity::observation`은
자기 제출 집합 밖의 request를 거부한다. `emit_stage_span`은 첫 owner에게만 보낸다. 그러므로 OUTPUT의
소유자별 라우팅만으로 다중 OUTER 관측이 성립했다고 하지 않는다. **목표 계약**은 full route별 허가된
요청 투영과 physical execution 전체 작업량을 명시적으로 구별하는 것이다. 외부 request를 조용히 무시하거나
전체 행 수를 소유 행 수로 바꿔 계측을 왜곡하지 않는다. 각 route에는 자신의 요청 관측을, 선언된 계측
수신자에는 execution별 전체 통계를 한 번씩 제공한다. 같은 route의 요청마다 전체 span을 복제해 생긴
양을 새로운 계산으로 세지 않는다. 투영/통계 버전의 producer와 consumer가 함께 통과하기 전에는 미완이다.

#### 소유자별 관측과 발행 증거의 완결 — 계약과 버전별 이관

이관 전 `inference.rs::drive`는 마지막 terminal와 해제 통지를 받은 직후 종료했으며, head/downstream은
Forward/TAIL을 발행한 뒤 관측을 보낼 수 있다. 따라서 정상 관측이 늦게 와도 놓칠 수 있다. 또한
받은 head 관측의 execution마다 stage span을 검사하는 것만으로는 **head 관측과 span을 통째로
누락한 실행**을 발견할 수 없다. 아래는 그 두 공백을 함께 닫는 계약이다. OUTPUT v4와 구 관측은
이 계약을 구현하지 않았다. 후속 작업 트리의 wire 이관은 아래 버전 절, 검증 상태는 로드맵 최신 기록을 따른다.

- 물리 전체 rows/phase 합·logical 폭·request/sequence 수·RPC 시간은 global 값으로 유지하고,
  각 full OuterEndpoint의 `owned_requests` 투영을 분리한다. head는 원제출 event ID·slot/incarnation을
  자기 RequestState와 대조해 붙인다. correlation/deadline은 해당 route의 실제 원본 ReplySpec에서
  결정적으로 선택한 carrier이며, 다른 소유 요청 전체의 identity/deadline을 대신하지 않는다.
- downstream span의 소유자는 execution별 request/slot/incarnation으로 보고하고 trusted head의
  원제출 결속 및 승인 OUTPUT과 join한다. native RowOwner나 P4 envelope에 상위 보고 정책을 넣지 않는다.
  새 native 실행만 span을 만들며 cached replay를 새 계산으로 계수하지 않는다.
- 발행 원장에 요청 시도별 **고정 크기 issued-work witness**를 둔다. 실제 승인된 logical issue마다
  count와 SHA-256 chain을 갱신한다. 입력에는 버전화된 도메인, head/OUTER endpoint,
  load/session/원제출/request/slot/incarnation, logical ordinal, 해당 요청의 실제 physical execution
  집합 및 phase/정확한 position 구간 목록/행 membership을 결속한다. min/max/count만으로 중간
  위치를 생략하지 않는다. 시간·수신 순서·JSON 객체 필드 순서·platform hash는 identity가 아니다.
  바이트 encoding과 정렬 순서는 wire 이관 전에 독립 literal 양·음성 vector로 고정하며, canonical
  encoding 없이 해시 이름만 추가하지 않는다. 소유 membership 증거가 전체 RPC 시간까지 인증하는 것은 아니다.
- 해당 요청이 없는 issue 때문에 ordinal에 간격이 생기는 것은 정상이다. 요청별 strict increase와
  같은 issue의 canonical physical/member 순서로 계산한다. 조각 수와 logical issue 수를 혼동하지 않는다.
  Verify/Replay의 정상 위치 재사용은 phase와 ordinal/execution으로 구분한다. count/phase 합만으로는
  같은 수의 다른 execution/range 교체를 검출하지 못하므로 충분한 결속이 아니다.
- head의 `accept_prepared_issue`에서 split/소유자/witness 후보 및 overflow를 **원장 commit 전에**
  전량 검사한다. 이미 prepared된 요청 후보에 작은 witness를 설치하고 flight/요청 상태를 비실패 구간에서
  함께 확정한다. plan·native 시작·실패·Uncertain·관측 송신 성공에 count를 증가시키지 않는다. 매 발행마다
  과거 execution 목록/프롬프트를 다시 복제하는 방식으로 증거를 구현하지 않는다.
- 정상 terminal 승인 OUTPUT의 **새 버전**에 최종 witness를 결속한다. 별도 송신된 관측이 자기 기대
  count/digest를 정하지 않게 하며, 기존 출력/해제 권위 사슬과 함께 producer/consumer를 이관한다.
  현 v4를 조용히 확장하거나 구 캡처에 사후 증거를 붙이지 않는다. 이 hash는 무결성 대조이지 네트워크
  peer 인증·내구 outbox·재시작 freshness·KV 정지점 증명이 아니다.
- 모든 관측 수신자를 사전 검증한 뒤 작은 effect intent로 보존한다. Full/Closed/번호 고갈로 미전송
  관측을 버리지 않고 기존 fence/재시도 계약을 적용한다. 관측 실패 때문에 native를 다시 실행하지 않는다.
- OUTER 완료는 terminal/해제뿐 아니라 witness와 자기 소유 execution의 전 stage coverage가 함께
  성립할 때다. Missing은 기존 overall deadline까지 수집, 충돌은 즉시 거부, Complete 후보만 최종 반영한다.
  head와 stage 관측 역순을 허용하며 반복 전체 이력 재검사를 피한다. 오류를 null/빈 통계로 바꿔 승인하지 않는다.
- span identity는 configured stage·load/session·canonical fresh execution 집합이다. 시각은 key가 아니라
  대조할 body다. 정확한 재전달은 무증가, 같은 identity의 다른 body 또는 다른 그룹에 겹친 execution은
  conflict다. 필요한 coverage는 해당 OUTER가 소유한 execution에 한정하며 B-only 계산을 A에 요구하지 않는다.
- 한 OUTER 자료의 범위는 **owner-visible physical work**다. B-only batch가 없는 자료를 전체 fleet의
  총비용/활용률이라고 하지 않는다. 여러 owner 자료를 합칠 때도 같은 물리 계산은 한 번만 집계한다.
  stage RPC span과 실제 GPU 시간, 검증된 clock 오차 범위의 비교와 미검증 cross-host 비교를 구분한다.

실행 순서는 로드맵의 현재 단계만 소유하며, 부정·지연·생산/소비·비용 시험은 검증 규약 T20/T25/T57/T58을 따른다.

##### 내부 issued-work v1 — 2026-09-07 작업 트리의 구현 범위

`v2/issue_witness.rs::IssueWitness`와 `node/state.rs::AdapterState::accept_prepared_issue`에 위 계약의
내부 승인 증거를 먼저 구현했다. L1의 요청 시도별 고정 크기 값이며 정책·P4 중립 코어·native/llama/backend가
증거를 독립 갱신하지 않는다. 생산 의존 `sha2`는 concrete staged adapter에만 추가했다. 이 내부 추출
자체는 native ABI 변경이 아니며, 후속 노출은 아래 별도 wire 버전을 사용한다. 소스 기준과 실행 결과는 증거 기록을 따른다.

정규 입력은 아래와 같다. 이 절이 encoding의 소유자이며 다른 문서는 이를 참조한다.

- 모든 정수는 명시 폭의 little-endian이고 문자열은 `u32` UTF-8 바이트 길이 뒤 원문이다. NUL/빈 식별자,
  잘못된 endpoint 및 0 generation/incarnation은 거부한다. 주소는 P4 `Address`의 display→parse 왕복이
  동일한 표현을 사용하며 DNS 해석·대소문자 접기·서로 다른 IP 표기를 임의로 동치화하지 않는다.
- seed는 `P4_ISSUE_AUTHORITY_V1` 뒤 NUL 1바이트, head 주소·node·generation(u64), OUTER 주소·channel·
  connection_generation(u64), load(u64), session·request·원제출 event ID, sequence_id(u32), incarnation(u64)
  순서의 SHA-256이다. 원본 return_route/source/target과 ReplySpec의 route/correlation/deadline도 대조한다.
  입력 필드 일치는 peer 인증이 아니다.
- issue는 `P4_ISSUED_WORK_V1` 뒤 NUL 1바이트, 이전 digest(32바이트), 다음 요청별 count(u64), logical
  ordinal(u64), execution 수(u32), 각 execution ID(u64)·row 수(u32)·각 row의 phase(u8)·position(u32)
  순서다. execution ID 오름차순, row는 `(phase, position)` 오름차순으로 정렬한다. phase는
  Prefill=0/Decode=1/Verify=2/Replay=3이다. 양쪽 정렬은 수신 배열 순서를 권위로 만들지 않는다.
- 한 issue 안의 0/중복 execution, 빈 실행/행 집합, 반복 `(phase, position)`은 거부한다. 요청별 ordinal은
  strict increase이며 요청이 없는 issue의 간격은 허용한다. 다른 issue의 Verify/Replay 위치 재사용은
  정상이다. 같은 개수·최소/최대 위치만 보존한 다른 중간 위치 또는 execution 교체도 다른 digest다.
- 상태 자체는 seed32+digest32+count8+last_ordinal8의 **80바이트 Copy 값**이다. Option/RequestState
  전체의 크기가 80바이트라는 뜻은 아니다. 현재 issue의 owner 행만 모아 검증·정렬하며 과거 실행 이력은
  저장하지 않는다. 기존 RequestState/plan의 프롬프트 clone 비용까지 해결한 것은 아니다.
- `accept_prepared_issue`는 모든 요청의 witness 후보를 먼저 만들고 flight 등록이 성공한 뒤 요청에
  설치한다. 후순위 owner 오류·overflow·flight 등록 거부는 committed/prepared 후보 모두 보존한다.
  prepare/begin/Uncertain·selector 부기·정산·전달 재시도는 witness를 증가시키지 않는다. 저수준
  `register_issued_batch`만 직접 호출해도 요청 증거는 생기지 않는다.

`validate_submission_identity`는 원본 제출의 공통 형식을 PREFILL에서 토큰화·세션키 기록·admission보다
먼저 검사한다. 원장 승인 때도 같은 검사를 재사용한다. 아직 배정되지 않은 slot/incarnation을 가짜 값으로
채워 witness를 미리 생성하지 않는다. NUL 식별자의 adapter 거부 뒤 같은 worker의 정상 요청은 계속된다.
정상 Unicode·별도의 correlation ID는 허용한다. 후속 `capsule.rs::validate_reply_options`는 실제 serialized
ReplySpec과 원문 options에 기존 LB/PB v4의 4096 UTF-8 byte 상한을 적용하며 Logical/Physical 검증과
PREFILL이 공유한다. reply는 nonempty, options는 empty도 허용한다. 정확히 4096은 유효하고 JSON escape
이전 필드 길이·문자 수·trim한 options 길이로 대신하지 않는다. PREFILL은 세션키 기록·Tokenize·slot/
incarnation 수용 전에 이 검사를 한다. wire 버전/한도·native parser 의미는 바꾸지 않았다. canonical
session/request/key는 기존 명령/owner 검사를 유지한다. 이 형식 검사만으로 다른 admission/자원 거부의
원자성까지 구현됐다는 뜻은 아니다.

##### OUTPUT v5 / BATCH_OBSERVATION v4 / STAGE_SPAN v4 이관 계약

이 절은 이번 작업 트리의 producer/consumer 이관이 따라야 할 명세다. 일부 하위 시험 통과를 전체
관측 게이트·최종 성능 승격으로 읽지 않는다. 현재 통과 범위/미완은 로드맵과 날짜별 증거가 소유한다.

- `ApprovedOutputPayload.issued_work`는 정상 sampled terminal에서만 필수다. revision=1,
  issue_count/last_ordinal은 u64, authority_digest/digest는 각각 **정확히 32개의 u8 배열**이다.
  unknown 필드/revision, count=0, last_ordinal<count, nonterminal에 proof 존재를 거부한다.
  flat OUTPUT의 명시 decode DTO는 unknown 필드를 거부하며 serde flatten의 느슨한 역직렬화에 의존하지 않는다.
- head는 terminal 정산 후보에서 원래 RequestState의 witness와 원제출 authority를 대조한 뒤 복사한다.
  요청 제거·flight 정산·출력 효과 commit 전에 실패할 수 있는 검사를 마친다. 반환/telemetry에서 새
  witness를 만들거나 저수준 flight 등록에 가짜 witness를 붙이지 않는다. 기존 v3/v4 캡처는 불변으로 보존한다.
- BatchObservation은 logical_ordinal과 물리 전체 통계를 유지한다. 각 물리 execution의
  `owned_requests`에는 request/submission_event_id/sequence_id/incarnation/request_issue_index와
  정확한 `rows:[{phase,position}]`를 싣는다. phase wire는 prefill/decode/verify/replay만 허용한다.
  요청별 index는 head가 실제 승인한 count와 같아야 한다. 관측의 index가 terminal 기대 총량은 아니다.
- 소유자 투영은 파싱한 full OuterEndpoint별 하나다. 동일 OUTER의 다른 correlation은 중복 전체
  관측을 만들지 않는다. carrier는 그 route의 원래 ReplySpec 중 실제 행 순서에서 처음 만난 것을
  선택하며 모든 구성원의 route/provenance를 먼저 검증한다. foreign 요청 ID를 싣지 않고, foreign
  행이 포함된 물리 전체 counts를 해당 OUTER 행 수로 줄이지 않는다.
- StageSpan의 execution_ids와 executions의 key 집합은 같아야 한다. execution별 owned_requests는
  request/sequence_id/incarnation만 보낸다. downstream이 모르는 submission event ID를 꾸며 넣지 않는다.
  Fresh만 새 span으로 보고하며 cached-only replay는 새 계산 span을 내지 않는다.
- 준비된 관측은 ForwardObserved 효과와 함께 보존한다. **로컬 completion mailbox가 Forward를
  수용한 직후** 시각을 한 번 고정해 Telemetry intent로 바꾼다. 이는 네트워크 송신/원격 수신 시각이 아니다.
  Full 동안 동일 이벤트를 기다리고, Closed/ID 고갈이면 미발행 intent와 고정 시각을 보존·fence한다.
  자동 재연결·crash 복구·내구 전달 또는 bounded 전체 RSS를 이 효과 큐만으로 주장하지 않는다.
- OUTER는 실제 송신한 제출 권위에 관측을 결속한다. 요청별 issue_index의 역순 수신을 보관하고
  연속 prefix가 될 때 각 issue를 한 번 해시한다. OUTPUT 승인 owner와 terminal chain이 맞아야 완료다.
  execution별 owned membership과 선언 stage coverage도 별도로 대조한다. 관측보다 먼저 온 span은
  잠정 증거이지 스스로 head의 기대 집합을 만들 수 있는 권한이 아니다.
- 종료/해제까지의 기존 `elapsed_ms`는 그 경계에서 latch한다. 추가 관측 대기가 끝난 시각은
  `telemetry_complete_elapsed_ms`로 별도 기록한다. 정상 응답 토큰량과 기존 TPS 분모를 조용히 바꾸지 않는다.
  Missing은 원래 overall deadline까지, Invalid는 즉시 실패이며 성공한 완결 후보만 행 합계를 적용한다.

DTO와 canonical 검증 API는 concrete llama adapter 소유다. P4 공용 protocol/core, native RowOwner,
llama.cpp/ggml/CUDA 타입에 보고 정책을 넣지 않는다. peer 인증·restart freshness·KV 정지점·실제 GPU
시간·다중 컴퓨터 성과는 이 wire의 보장 범위 밖이다.

### L2 수용·점유 (admission)

요청 도착·완료·취소·메모리 변동 시 동작한다. 점유는 오래 유지될 수 있지만
수용 반응을 분 단위로 지연시키는 계약은 아니다.

- 수용: 슬롯뿐 아니라 전 stage의 모델별 최악 셀/보조 상태 예산과 queue/byte/token 상한을 검사한다.
  prepared 예약도 사용량에 포함한다. 실측/감사되지 않은 단가와 오버커밋 비율을 사실로 쓰지 않는다.
- 축출·체크포인트: L2는 **정책을 갖지 않는다.** 언제 어떤 세션을 어떤
  키로 영속화·체크포인트·폐기할지는 전부 OUTER의 명령이고(스냅샷 명령
  모델은 [kv-state-store-convention.md](kv-state-store-convention.md)
  소유), L2는 명령의 이행과 그 셀 회계만 담당한다. TTL은 OUTER가 이
  어휘로 표현하는 정책 중 하나일 뿐이다.
- 복원: OUTER 명령과 [저장 규약](kv-state-store-convention.md)의 복원 판정 사다리·셀 예약·정지점 계약을 따른다.
- 공정성: admission 대기와 runnable 대기를 분리한다. 요청별 age/deadline/최대 미선택 간격을
  검증한다. 같은 UBATCH 동승이나 셀 여유만으로 ITL/TTFT가 자동 공정하다고 가정하지 않는다.
  부족 시 queue/reject 또는 승인된 OUTER 스냅샷 정책을 이행하며 자동 축출은 장애 게이트 전 비활성이다.

#### PREFILL의 준비와 확정 — 수용 거부 원자성의 제한된 구현

위 L2의 전체 자원 계약과 달리 현재 `worker.rs::Worker::prefill`의 변경은 **요청 상태의
Result 거부 전이**만 다룬다. 실행·검증 여부는 로드맵/증거 기록을 따른다.

- 명령/owner/문자열/기존 identity 검증 뒤 incarnation과 추가될 pending FIFO 접두를 먼저 검사한다.
  `release.rs::Worker::prepare_prefill_admission`은 기존 pending 뒤 새 후보를 가상으로 붙여,
  이번에 실제로 배정할 슬롯/요청 전체를 검증한다. ACK의 기존 접두 검사도 같은 validator를 쓰며
  전체 대기열의 다른 구간을 새로 거부하지 않는다. 새 후보가 이미 pending에 있으면 거부한다.
- Tokenize와 prompt+max_tokens 검증까지 성공한 뒤에만 session key 기억, 요청 삽입, incarnation
  증가, pending 추가와 검증된 FIFO 배정을 확정한다. sole worker에서 그 사이 다른 handler/yield/
  publication은 없다. `P4_SESSION_KEY_ADMITTED` 기록은 확정 뒤에만 낸다.
- 오류 우선순위는 바뀐다. identity 검증 뒤 과거 Tokenize→context→incarnation→admission 순서가
  incarnation→admission→Tokenize→context가 된다. 이미 무효인 수용 상태로 불필요한 native 조회를
  하지 않기 위한 fail-fast이며, 여러 오류가 겹친 입력의 메시지가 그대로라고 주장하지 않는다.
- Tokenize는 동기 native 조회이며 KV 발행이 아니다. 조회 실패에서 요청 수용 상태를 보존하는 것과
  native/lifecycle의 모든 내부 상태가 불변이라는 것은 다르다. 일반 ERROR의 정확한 1회 발행과 ID
  소비는 거부 진단 효과로 별도 검사한다. allocator panic·기록 채널 실패까지 롤백하는 계약은 아니다.
- 이 준비 결과는 비동기 보류를 가로질러 쓸 수 있는 durable ticket이나 공간 claim이 아니다.
  실제 request/출력/미래 반환의 count·byte 예약은 여전히 첫 쓰기 **전**에 연결해야 한다.
  이 수정만으로 bounded admission, blocked-input 소비 또는 actor 교착 해결을 선언하지 않는다.

### L3 구성 (strategy)

매 발행 기회에서 동작한다. 순수 함수로 유지한다 — 입력은 전부 데이터, 출력은
할당. 그래야 기록된 트레이스로 재생·검증할 수 있다.

```
plan(demands,            // 원장이 상주 전제를 통과시킨 것만
     row_budget,         // 논리 n_batch, physical n_ubatch를 각각 보존
     cell_budget,        // 가장 빡빡한 노드의 남은 셀
     pending_cache_ops,  // 대상 시퀀스 펜스 + 실제 context 배타 요구
     shape_rules,        // HELLO 협상값
     cost_model)         // 모델별 고정비·행당비
  → allocations + proposed fairness delta
```

전략은 trait 뒤의 모델군별 모듈이다. 선택 키는 HELLO capability와 GGUF
메타(memory family, equal_sequence_ubatch, swa, shared_kv, nextn …)이며
모델명 하드코딩은 금지한다.

- `waterfill` (기본 attention+unified): 적격 decode와 대기 프리필을 제한된 row budget과
  요청별 service bound 안에서 배분하고 회전한다. 현행 `plan_ordinary`는 출발점이며 완성된 공정성 증명이 아니다.
- `equal_width` (recurrent/hybrid): 등폭 강제. 디코드가 폭을 1로
  붕괴시키므로 프리필 전용 스텝과 디코드 전용 스텝을 분리하는 편이 낫다.
- `atomic` (MTP/speculative): 원자 창 + 펜스. 다른 전략과 합성된다.
- 향후: index-aware(DSA/MSA/DSV4), paged/radix 계열. vLLM PagedAttention과
  SGLang RadixAttention은 참조 대상이나, 노드별 셀 단가가 다르다는 조건이
  우리 고유의 추가 제약이다.

채움은 shape·의존성·credit·KV·요청별 지연 bound를 먼저 만족해야 한다.
그 안에서 폭과 issue 빈도를 실제 비용으로 비교한다. 현재 고정비가 모든 모델에서 지배한다거나
넓은 배치가 항상 최선이라고 가정하지 않는다. 탐색·승격은 검증 규약의 고정 토폴로지 A/B로 판정한다.

계획 후보 생성은 정책 상태를 바꾸지 않는다. L4/발행 원장이 승인한 경우에만 후보의 회전·cohort
delta를 commit한다. 새 계획이 이미 commit되어 revision이 바뀐 오래된 후보는 재사용하지 않는다.
거부된 후보로 공정성 순번을 소모하지 않는 것과, 실제 수신 루프가 유한한 발행 기회를 주는 것은
별도 조건이다. selector 시험만으로 무제한 input drain의 기아를 부정할 수 없다.

### L4 증명 (proof)

제출 직전, 논리 배치가 llama.cpp의 split 규칙 아래 **정확히 예측된
물리 UBATCH**로 쪼개짐을 증명한다. 기존 검사((seq,pos) 일대일, 등폭,
verify 펜스)에 셀 예산과 예상 분할 수 검증을 더한다. 통과하면 제출하고
ubatch 콜백 멤버십을 캡처하며, 하류는 재도출 없이 재생한다.

### L5 전송 (기존, 효율화 대상)

전략과 구분되는 비용 항이다. cut-set packing과 불필요한 재전송 제거는 후보이며,
실제 다중 머신에서 serialize/copy/network/queue 시간을 분리해 우선순위를 정한다.
텐서 값·alias/view·membership 손실 없이 동등성 게이트를 통과해야 한다.

## 실행 소유권과 native 제어 결속 — 2026-09-06 작업 트리 계약

이 절은 실행 wire와 메모리 내 소유권의 단독 정의다. 저장 캐시의 정체성/내구 epoch를 대신하지 않는다.
구현 근거는 adapter의 `v2/node/ownership.rs::StageOwners`, `v2/control_identity.rs`,
native의 `runtime/physical_authority.hpp::PhysicalAuthority`다. 현재 검증 범위는
[정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의 최신 기록을 따른다.

- 실행 소유자는 `(load_generation, session_id, sequence_key, sequence_id, incarnation)`이다.
  incarnation은 0이 아닌 head 발행 단조 ID이며 같은 key/slot 재사용 때 새 값이 필요하다.
  native slot은 한 active 소유자만 가진다. 새로운 소유권은 빈/해제 슬롯의 Prefill position 0에서만 시작한다.
- LB/PB codec은 revision **4**, owner의 `load_generation` 뒤에 LE u64 incarnation을 싣는다.
  physical/tail/release/released/settle/settled content-type도 v4다. 옛 codec을 자동 변환하지 않는다.
  `ReleaseSequence`·`SettlementSequence`의 incarnation과 operation_id는 필수, 0과 누락은 거부한다.
- identity 지원은 HELLO의 `physical_identity_revision=1`로 따로 협상한다. physical batch 지원만으로
  추론하지 않는다. event 제품 LOAD는 capability 확인 뒤 **BindLoad(opcode 23)**에 LE u64 generation을
  보내 정확한 echo를 받은 후에만 용량/슬롯을 공개한다. bind 실패/다른 echo/응답 유실은 native를 닫는다.
  같은 worker에서는 시도한 generation도 소모하여 재사용/감소를 거부한다.
- native의 BindLoad는 현재 Session에 명시적으로 load identity를 결속한다. 첫 PHYSICAL 소유자를 보고
  암묵적으로 bind하지 않는다. bound mode에서는 bare slot Cancel·legacy Hop·기존 KV mutation 우회가
  실행 권한을 얻지 못한다. 해당 state 기능 재활성화에는 같은 소유권을 따르는 별도 통합이 필요하다.
- native 제어 prefix는 `P4ID | revision:u16=1 | reserved:u16=0 | load:u64 | incarnation:u64 |
  operation:u64 | slot:u32 | session:(len:u32,UTF8) | key:(len:u32,UTF8)`이며 정수는 LE다.
  key는 정확한 `session + NUL + request`이다. session/request 내부 NUL을 허용하지 않는다.
  RELEASE body/응답은 정확한 prefix다. SETTLE은 prefix 뒤 retain/replay_position/count(u32)와
  replay token(i32) 배열, 응답은 같은 prefix 뒤 proposal count/token 배열이다. 짧음·trailing·다른 echo는 거부한다.
- 연산 ID는 head가 승인된 정산 후보에 배정한다. 각 슬롯의 최신 `(operation, 요청 본문, 결과 본문)`만
  재생할 수 있다. 같은 ID/본문은 native 추가 실행 없이 동일 결과, 같은 ID/다른 본문·종류는 conflict,
  더 오래된 연산/다른 incarnation은 무효다. 해제 후 새 소유자가 들어오면 옛 제어 receipt로 새 KV를 만질 수 없다.
- 해제된 incarnation high-water는 다른 session이 슬롯을 임시 사용해도 잊지 않는다. 상한 때문에
  watermark를 버리지 않고 신규 수용을 거부한다. 이 정책은 안전성을 위한 한정 메모리 계약이며
  오래 실행할 때 무제한 신규 session을 지원한다는 뜻이 아니다.
- 슬롯별 최신 control receipt의 요청/응답 각각 1MiB, 전체 64MiB, watermark 65,536개가 현재의
  안전 상한이다. 여러 sequence를 담은 control은 첫 native 호출 전에 전체 최악 응답 공간을 검사한다.
  정확한 Replay는 추가 예약이 없고, New의 기존 receipt를 뺀 양수 증가분만 합산한다. 뒤 연산의
  예정된 축소를 앞 연산에 미리 대출하지 않는다. 이 검사는 한 worker의 직렬 명령 안에서만 유효하다.

### PHYSICAL 수신 receipt — 2026-09-07 후속 작업 트리 계약

`v2/node/physical_receive.rs::PhysicalReceiveLedger`는 중간/꼬리의 `Worker::physical`에서 native 호출
**전에** 사용한다. head terminal 정산이나 SETTLE/RELEASE receipt와는 다른 원장이다.

- 실행 번호 발급자는 head의 native Session이다. 수신 key는 수신 load 안의
  `(SESSION.first의 전체 Endpoint(agent,node,generation), execution_id)`다. 이미 설정된 session 경로에서
  발급 권위를 가져오며 incoming event ID나 payload의 자칭 issuer를 사용하지 않는다.
  같은 head의 다른 session/body로 ID를 재사용하면 conflict지만, 다른 head의 같은 숫자는 별개 정상 실행이다.
- 이벤트 전체 입력을 먼저 검증한다. canonical 입력 바이트에는 invocation·소유 행·incarnation·tensor가 모두
  결속된다. Fresh만 native에 보내고, 보존된 정확한 Replay는 기존 응답을 돌려준다. 혼합 이벤트의 출력은
  원래 구성원 순서로 조립한다. 뒤 conflict가 앞 Fresh의 owner/ID/native 효과를 먼저 소비하면 실패다.
- native 진입 직전에 Fresh를 Running으로 등록한다. 결과 membership/role이 다르거나 응답 유실/실행 오류면
  해당 시도의 모든 Fresh는 Uncertain이고 worker를 fence한다. 이미 바뀐 KV를 rollback했다고 보고하지 않는다.
- 완료 receipt의 canonical 입력+결과 바이트 합계 64MiB/4096개, 전체 Seen ID 65,536개,
  발급 권위 1024개가 현재 안전 상한이다. 큰 정상 응답은 한 번 전달할 수 있으나 보존 불가하면 Expired다.
  active input·encoding 임시 메모리·publisher intent·전체 RSS는 이 cache 예산에 포함된 것으로 주장하지 않는다.
- 숫자 수신 창은 head별 65,536이며 최고 ID에서 창 밖으로 밀린 ID는 미도착 gap도 포함해 거부한다.
  다른 head의 ID 진행으로 그 창을 움직이지 않는다. cache 퇴출 뒤 재계산하거나 issuer 기억을 자동 폐기하여
  과거 ID를 새 작업으로 되살리지 않는다. 신규 identity 상한 초과는 사전 거부한다.
- 보장은 **같은 Worker/load 수명에서 보존 중인 정확한 재전달의 native 추가 실행 0**과 만료 후 fail-closed다.
  cache/창/issuer 상한은 아직 재시도 기간·edge credit·재연결 계약과 협상되지 않았다. 이를 무손실 재전달 또는
  장애 후 exactly-once로 승인하지 않는다. B3에서 유효 재시도 기간과 보존/역압/명시 만료를 결속해야 한다.
- Replay만 있는 이벤트는 stage 소유권을 새로 획득하지 않고 계산 span도 추가하지 않는다. 해제/슬롯 재사용 후
  옛 결과를 재응답하는 것과 옛 KV를 다시 계산하는 것을 구분한다.

**이 receipt만으로 보장하지 않는 것:** 새 PHYSICAL ID의 행 순서는 아래 별도 frontier가 담당한다.
credit, 모든 이벤트의 다중 native 원자 실행, crash 후 내구 receipt/outbox 복원은 별도다. 제어 preflight가 성공했어도 뒤 native 호출이
불확실하게 실패하면 이미 실행한 KV를 rollback했다고 하지 않고 fence한다. worker/native 프로세스의
재시작을 가로지르는 load/run epoch 권위도 아직 없다. fresh fleet identity와 재접속/이전 세대 차단을
구현하기 전에는 이 메모리 내 guard를 restart exactly-once로 승격하지 않는다.

이 변경은 **adapter-owned protocol 의미의 의도적 버전 변경**이다. P4 공용 envelope나 CUDA/ggml에
요청 개념을 넣지 않았으며, llama.cpp API 변경에 끌려 올라온 필드도 아니다. 반대로 이것만으로
stage ABI/state ABI/actual placement/공통 타입 격리의 B5 게이트가 완성되는 것은 아니다.

### stage KV frontier — 수신 ID와 별개인 위치·phase 계약

`v2/node/frontier.rs::StageFrontiers`는 순수 어댑터 원장이다. llama/ggml/backend 타입이나
native 호출을 갖지 않는다. 같은 slot의 load/session/key/incarnation, 다음 KV 입력 위치,
생성 토큰 수·예산·options/reply, 미결 Verify와 허가된 Replay를 함께 검사한다.
`Worker::drive_one_batch`의 실제 발행과 downstream `Worker::physical`, SETTLE/RELEASE가 소비한다.
native C++의 `PhysicalAuthority` 자체가 이 위치 검사를 수행한다는 뜻은 아니다.

| 현재 상태 / 입력 | 승인 조건과 다음 상태 |
| --- | --- |
| 새 소유자 / Prefill | 위치 0부터, 연속 입력. 마지막 output 표식 전에는 생성량 0. output은 그 요청의 마지막 prompt 행 한 번만 |
| 계속 Prefill / Prefill | 바로 다음 위치만. 이미 final 표식을 처리한 뒤 Prefill로 회귀 금지 |
| Ready / Decode·Verify | 정확한 다음 위치·생성량·예산. Decode는 1행, Verify는 분할하지 않는 새 speculative round |
| Verify 계산 후 전량 수용 | 별도 SETTLE 없이 다음 연속 append가 head/middle에 전량 수용을 확인. tail은 실제 outcome으로 확인 |
| Verify 부분 수용 / SETTLE | 진행 중 Verify 범위 안의 유효 경계만. tail에서는 직전 rollback outcome과 정확히 일치. 새 operation ID만으로 임의 trim 금지 |
| checkpoint SETTLE / Replay | 실제 복원 끝은 `replay_position`. `retain_from`은 앞으로 다시 채울 끝. 같은 round·정확한 token 배열·범위를 한 번만 Replay |
| 종료 / RELEASE | 이미 검증한 제어 identity·receipt와 결속해 slot frontier 삭제. 새 incarnation 수용 권위는 StageOwners watermark가 유지 |

Replay의 wire `output=false`는 내부 logits 계산을 금지하거나 실제 생성 결과가 없다는 뜻이 아니다.
checkpoint Verify의 미확정 결과는 출력하지 않고, 허가된 Replay가 재계산·확인한 결과를 한 번 출력한다.
native 배치의 logits 요청 mask와 logical/capsule output mask는 별도 의미다. 엔진에 필요한 logits를
요청하더라도 원래 owner/capsule의 logical mask를 바꾸지 않는다. 이 번역은 native 어댑터의 책임이며
순수 원장·배치 정책에 llama_batch 필드나 backend 타입을 넣지 않는다. 실제 logits/샘플러/checkpoint
정상성은 native 모델 시험으로, adapter의 생성량·위치·제어 정산은 actual worker 시험으로 구분한다.

tail에서 다음 발행은 이전 sampler가 반환한 **전체 proposal token 배열**과 Decode/Verify 구분에
일치해야 한다. head/middle은 tail proposal을 직접 보지 않으므로 동일한 token 증명을 주장하지 않는다.
continuation의 proposal/replay 길이는 요청의 남은 token budget뿐 아니라 로드된 atomic physical
capacity 안이어야 한다. tail PHYSICAL과 native SETTLE 응답의 승인 전에 대조하며, head 반환
승인도 독립 대조한다. 폭을 잘라 정답을 바꾸거나 head의 뒤늦은 거부로 stage 승인을 대신하지 않는다.
native 효과 뒤 위반은 성공 receipt/frontier/forward 없이 fence한다. PHYSICAL의 Fresh 결과 묶음에
정상 앞부분과 잘못된 뒤 결과가 있으면 Fresh 전체를 Uncertain으로 남긴다. 이미 실행한 native
KV/sampler를 롤백했다는 뜻이 아니다. 정확한 전역 배포 폭 협상·placement는 별도 로드 계약이다.
이미 완성된 ID의 정확한 재전달은 frontier를 재전진시키지 않는다. 늦은 gap은 사전 거부하며,
그 거부가 자동 재정렬·재시도·무손실 전달을 구현한 것은 아니다.

whole-event Fresh/control 대조는 첫 native 효과 전에 끝낸다. 사전 거부는 owner·frontier·receipt·
출력 의도·native KV 모두 보존한다. delta는 touched slot만 들고 slot revision으로 stale/ABA를 거부한다.
native 응답의 row membership·outcome/proposal까지 확인한 뒤 commit하며, 성공 opcode 뒤 잘못된
응답이나 결과 불명은 fence다. 그 뒤의 오류를 사전 거부와 같은 rollback 보장으로 보고하지 않는다.
현재 worker는 native 호출 중 load/다른 상태 변경이 끼어들지 않는 직렬 소비자다. 이 전제가 바뀌면
reservation/commit 원자성을 다시 검증해야 한다. 이 원장도 restart 내구성·edge credit·전체 RSS 상한이 아니다.

### 명시적 UNLOAD와 실패 정리의 구분

정상 worker에 대한 UNLOAD는 현재 load의 **로컬 정지점에서만** native 소유권을 해제한다.
요청/수용 대기, 발행 준비/비행, pending SETTLE·RELEASE, Verify fence, 미발행 효과,
stage owner/frontier의 활성 KV, 수신 Running/Uncertain이 남으면 native 호출 전에 busy로 거부한다.
middle의 requests가 0인 것만으로 정지했다고 하지 않는다. 완료 receipt와 Released tombstone은
진행 중 KV가 아니며, 그것만 남은 idle UNLOAD를 영원히 막지 않는다.

busy는 호출자에게 상관 ID가 일치하는 명시 오류를 보내고 원장/KV/발행 효과를 보존한다.
기존 작업의 반환·정산·해제는 계속 처리할 수 있어야 한다. 성공은 local quiescence + native
cleanup 성공 뒤에만 UNLOADED로 보고한다. 이미 publish된 출력의 OUTER 도착이나 상대 stage의
정지까지 보증하지 않으며, 클러스터 전체 drain의 완료 증거로 사용할 수 없다.

이미 effects/native 결과 불명으로 fenced된 worker는 정상 busy 경로와 다르다. 현재 run의
실패 정리는 잔량·원래 오류를 보존하고 실패로 종료할 수 있으며 UNLOADED 성공을 내지 않는다.
정상 UNLOAD의 native cleanup 자체가 실패해도 복구 가능 busy로 바꾸지 않는다. 부분 종료된
엔진에 후속 요청/SESSION/재로드를 계속 승인하지 않고 실패 경계를 유지해야 한다.
강제 종료·요청 Cancel·분산 Drain은 별도 명령/상태 계약이며 UNLOAD의 성공 어휘에 섞지 않는다.

### 출력 포화 중 제어 진행 — 양보 가능한 effect pump의 목표 계약

**아래는 아직 구현 완료가 아닌 목표 계약이다.** 현재 `worker/emit.rs::Worker::wait_for_publication`은
Full에서 worker 스레드를 점유한다. capacity 통지를 추가하는 것과 worker가 다른 입력을 처리하는
것은 별개다. 실행 결과·단계 상태·적용 순서는 로드맵과 증거 기록이 소유한다.

- **고정 송신물**: 출력·관측·제어·오류·LOAD/SESSION/UNLOAD 응답을 동일한 보존 규칙으로 처리한다.
  Event의 ID/sequence·본문·수신자·상관 정보는 한 번만 배정한다. Full이 돌려준 Event를 그대로
  재시도하며 재직렬화·ID 재발급·native 재실행으로 대체하지 않는다. Forward 승인 뒤 고정한
  관측 시각도 후속 recipient 대기 때문에 다시 쓰지 않는다.
- **응답 표현 가능성과 상태 승인**: session 경로를 설치하기 전에 정확한 응답의 직렬화·ID 발급·
  수신 codec 표현 가능성을 확인한다. 개별 필드 길이 검사만으로 합산 envelope 검사를 대신하지
  않는다. 준비 실패는 권한/ID를 소비하지 않으며, 별도 진단 ID를 소비하는 handle 거부와 구분한다.
  SESSION은 권한 승인 전에 body와 최대 발급 가능 ID 폭의 envelope를 검증하되, 실제 번호는
  앞선 지연 관측까지 지나간 FIFO 선두에서만 배정한다. 준비 성공이 queue 공간 확보 또는 Closed 후
  분산 rollback을 의미하지 않는다. 일반 오류의 envelope도 표현 불가능하면 잘못된 wire를 보내지
  않고 진단 의도와 실패 원인을 보존한다. encode/decode의 일시 복제 비용은 전체 retained-byte 예산이 아니다.
  최대 ID 폭 검사는 수용 영역을 보수적으로 줄인다. 현재 짧은 번호로는 맞아도 향후20자리 번호에서
  codec 한도를 넘는 경계 입력은 승인하지 않는다. 이 차이는 단순 호출 위치 이관이 아니며, 같은 입력에
  대한 현재 번호의 codec 성공과 최대 번호의 거부를 모두 검사한다. 실제 번호 선소비로 우회하지 않는다.
- **예약과 양보**: 효과 개수와 보존 바이트의 합계 예산을 모두 검사한다. 하나의 TAIL/ACK가 만드는
  전체 효과를 검증·예약한 뒤 기존 whole-event 원자 commit을 유지한다. native 호출 전에는 그 결과와
  실패를 보존할 공간도 확보한다. 직렬화 wire 길이만 세면서 보존된 base/payload 복사 비용을 제외하면
  bounded RSS 증명이 아니다. 기존 큰 wire 한도를 임의로 줄여 통과시키지 않는다.
- **자원 선언과 한도의 종류**: `n_batch`/`n_seq_max`/mailbox 개수에서 adapter 전체 byte 예산을
  유도하지 않는다. OUTER가 정한 호스트 자원 정책을 composition root에서 adapter-local 설정으로
  전달한다. 보존 효과·보류 입력·native 임시 메모리·미래 반환/통지·실패 진단의 count/byte 영역을
  수치로 선언하고 합산한다. 이는 P4 코어에 llama 지식을 넣는 필드도 native wire 포맷 상한의 축소도
  아니다. 자원 정책의 기본값은 명시·검증해야 하며, 정상 한 발행의 최대 의무조차 예약할 수 없으면
  native 호출 전에 명시 보류/거부한다. `n_batch`를 몰래 낮추거나 결과를 받은 뒤 버리지 않는다.
- **미래 통지의 ID 여력**: RELEASE를 발행하기 전에 원본 제출별 receipt의 최악 count/bytes와
  미래 Event 발급 **개수**를 pending 권위에 예약한다. 실제 sequence 번호를 미리 배정하지 않는다.
  그렇지 않으면 뒤늦은 receipt가 이미 발행한 출력보다 작은 sequence를 갖는다. 일반 새 송신은
  남은 ID 공간에서 약속된 발급 개수를 제외한 몫만 쓰며, ACK는 검증된 whole-group 예약을 실제
  단조 ID와 고정 송신물로 전환한다. 합계 초과·overflow·잘못된 ACK는 예약/slot/원장을 소비하지 않는다.
  native 불확실 결과에서 예약을 반환하거나 ACK 도착 때 처음 일반 용량을 요구하지 않는다.
  현재 `worker/obligations.rs::Worker::ensure_event_id_obligations`는 queued 효과·활성 후속 관측·
  미래 해제 영수증·보류 진단의 **ID 발급 개수**를 대조한다. count/byte 공간 예약은 아니다.
  일반 발행은 prepare_issue와 native 호출 전에 검사하며, native 시도 뒤 결과 불명은 사전
  거부로 되돌리지 않고 Uncertain으로 유지한다.
- **보존 수명**: 예산 원장·고정 outbox·ID 여력은 load가 아니라 Worker 수명에 속한다.
  LOAD/UNLOAD가 미전송 구세대 응답을 `clear`로 지우면 실패다. UNLOAD의 성공 응답은 native 정리
  전에 준비·예약하고, 성공 뒤에도 원래 원인의 불변 송신물로 남긴다. 동적 HELLO를 읽어야 만드는
  LOADED는 bootstrap/native 임시 공간과 성공·실패 응답의 상한을 먼저 확보한다. 실제 frame 수신·
  파싱의 일시 복사도 예약 대상이며, 발행이 Uncertain이면 의무가 사라진 것으로 회계하지 않는다.
  비용에는 큐에서 꺼내 실행 중인 effect도 포함한다. VecDeque 길이가 줄었다고 그 메모리를 반환하지 않는다.
- **압력의 위치**: 미전송 효과가 남으면 새 native issue를 계속 쌓지 않는다. 입력 보류 자체도 유한한
  count/byte 예산에 포함한다. 반복 SESSION·오류·PREFILL을 별도 무상한 큐로 옮기지 않는다. 이미
  승인된 작업의 반환/ACK용 용량과 일반 새 입력의 용량을 구분하고, 그 예약 권한은 L1/L2가 소유한다.
  모든 큐가 찬 순환망의 진행은 이 국소 pump가 아니라 end-to-end credit/제어 용량 계약까지 필요하다.
  단일 입력 FIFO 앞의 새 요청이 Full로 보류된 상황은 이미 adapter에 수용된 ACK와 다르다.
  유한 parked queue만으로 무제한 새 입력 뒤 ACK의 진행을 증명하지 않는다. 반환 입력의 예약된
  수용 경로와 broker/edge credit를 함께 검증해야 end-to-end 포화 해소라고 부를 수 있다.
  예약은 수신 측 실제 공간에 근거하고 어댑터가 원인 작업·필수 반환 의무를 결속한다. 중립 전달층은
  opaque 권한의 대상·세대·소유·count/bytes만 이행하며 llama content-type을 해석하지 않는다.
  현재 broker의 `(source, correlation)` 순서를 유지한다. 후속 ACK가 앞선 동일 순서 영역의
  송신물을 추월하게 하거나 sender가 쓴 EventClass::Control을 예약 권한으로 인정하지 않는다.
  반환 credit은 수용 공간의 책임 이전이지 KV 정산 증거가 아니다. 이는 목표 계약의 제약이며
  새 grant API/wire가 구현됐다는 뜻이 아니다.
- **제어 실행 권위**: head의 pending Release/Settlement는 등록만으로 완료 권한이 되지 않는다.
  `Queued → LocalApplied → ForwardAccepted`에 해당하는 단조 상태를 가지며, 정상 ACK는 정확한
  load/session/key/slot/incarnation/operation과 마지막 상태를 모두 만족해야 적용한다. local native
  응답 검증·owner/frontier commit 뒤에만 LocalApplied, 다음 stage로 보낼 정확한 Event의 mailbox
  수용 뒤에만 ForwardAccepted다. 한 command의 모든 구성원을 먼저 대조한 뒤 함께 갱신한다.
  이 mailbox 승인은 전송 단계 증거이지 원격 KV 완료가 아니다. 슬롯 반환은 전 stage 적용을
  체인으로 결속한 꼬리의 단일 ACK가 별도로 성립한 뒤다. 오래된 effect callback이 새 incarnation이나
  다른 operation을 승격하면 실패다.
- **replay와 역할**: native control receipt는 LocalApplied의 근거만 제공하고 ForwardAccepted를
  대신하지 않는다. 정확한 native replay는 native 호출 0회로 상태를 유지하며 이전 단계로 낮추지 않는다.
  head pending 상태를 중간/꼬리에도 억지로 만들지 않는다. 중간/꼬리는 자신의 native receipt와
  미전송 Forward를 보존한다. 퇴역 후 ACK의 현행 거부를 이 변경에 끼워 멱등 성공으로 바꾸지 않는다.
  SETTLED는 꼬리의 정상 proposal이 추가될 수 있으므로 송신 SETTLE body와 바이트 동일성을 요구하지
  않는다. 기존 identity/retain/replay 대조와 Proposal/Replay 의미 검증 위에 전송 단계를 추가한다.
- **native 원자 구간**: 현행 `StageOwners::validate_control_batch`는 예약이 아닌 읽기 검사다.
  그 검사와 같은 command의 local native loop 사이에는 다른 command를 끼워 넣지 않는다. 첫 pump는
  외부 전송 대기에서만 양보한다. native loop도 turn quantum으로 분할하려면 receipt 예산 예약과
  candidate revision 계약을 먼저 구현·검증한다. 전송 단계 필드만으로 그 예약이 생기지 않는다.
- **검증 ticket의 수명**: 현재 head의 local/forward ticket은 서로 다른 private 타입이며,
  동일 worker의 동기 prepare→effect→commit 구간에서만 유효하다. ticket을 Full 너머 보관하는
  예약으로 사용하지 않는다. 양보 후 재송신은 현재 load/session·원래 제어 구성원·route를 다시
  검증해야 하며, 퇴역한 key나 새 incarnation에 과거 ticket을 적용하지 않는다.
- **대기와 실패**: 입력 도착·출력 공간·shutdown을 함께 관찰하며 등록과 재검사 사이의 wake 유실을
  막는다. 공간 통지는 예약도 전송 성공도 아니므로 실제 offer 결과를 다시 확인한다. Full은 Pending,
  Closed/ID 고갈/불명 native 결과는 각각 명시 실패다. native 실행 금지와 남은 진단 송신 수명을
  분리해 ERROR를 큐에 넣자마자 worker 종료로 버리는 회귀를 금지한다. 종료 deadline 뒤 미전송물은
  완료가 아니라 명시 abandonment로 기록한다. 내구 재연결/분산 Drain을 구현했다고 하지 않는다.

중립 mailbox는 opaque Event의 소유와 공간/종료 통지만 제공한다. 효과 예산·제어 실행 단계·KV
정산은 어댑터 안에 남긴다. 시험은 검증 규약 T22~T26을 따르며, 정상 ACK 진행만 고치면서 조기 ACK를
허용하거나 모든 입력을 막아 메모리 상한만 통과하는 구현도 실패해야 한다.

#### 고정 송신물과 미할당 ID 의무 — 제한된 구현 계약

`worker/effects.rs::Worker::flush_effects`의 committed FIFO는 현재 선두만 직렬화·checked ID
발급한 뒤 `Publication { event, after }`로 바꾼다. 실제 구현의 실행 여부는 로드맵/증거가 소유한다.
이 구분은 내부 표현 계약이며 wire 버전이나 순서 영역을 바꾸지 않는다.

| 실패/전이 지점 | 보존할 것 | 미할당 ID 의무 |
| --- | --- | --- |
| 직렬화·ID·사전 head 권한 검사 실패 | 원 DTO/본문·기존 FIFO·native 상태 | 원래 개수 유지, ID 미소비 |
| Event 생성 성공 | 정확한 envelope·ID·sequence·payload와 after-action | 자신의 ID만 소모, 후속 몫 유지 |
| Full | 같은 Event로 재시도, 현재 head 권한 재검증 | 추가 ID 미소비 |
| Closed·영구 초과·종료 중 Full | 같은 Event를 FIFO 선두에 반환하고 fence | 이미 발급한 ID는 되돌리지 않음 |
| ForwardObserved 수용 | forward는 제거, 한 번 고정한 시각의 관측을 원 순서로 배치 | 아직 생성하지 않은 관측 N개 |

따라서 `next_event + 미할당 의무`는 Event 생성으로 변하지 않는다. 일반 Publication의 미할당 몫은
0이고 Observed Publication은 N이다. `active_effect_ids`에는 materialize 이후의 N을 모두 넣어
Full 중 ACK/진단이 그 몫을 쓰지 못하게 한다. 이는 **공간 예약 수나 Event 보관 수가 아니다.**
원래 intent에 남아 있던 관측의 recipient/provenance/payload 전체와 queued suffix를 보존한다.
실패 후 자동 fence 해제·native 재실행·재연결 replay는 허가하지 않는다. 시험의 수동 재접속은
같은 Event의 보존 여부를 확인할 뿐 운영 복구 API가 아니다.

LOAD·SESSION·UNLOAD·일반 오류도 미번호 직접 응답 의도를 같은 FIFO에 넣고, 선두에서 고정
Publication으로 만든다. 앞선 ForwardObserved의 후속 관측은 뒤에 들어온 응답보다 먼저 번호를
얻는다. batch 오류는 첫 전달 실패 때문에 나머지 참여 요청의 진단 의도 자체를 잃지 않아야 한다.
표현 불가능한 진단 의도는 발행 가능한 Event인 것처럼 취급하지 않고 비발행 실패로 보존한다.

native fence 뒤의 진단에는 두 경우가 있다. **기존 효과 prefix가 없는** 상태에서 생긴 종료 진단
하나만 보내는 것은 새 native 실행이 아니며 기존 fence를 해제하지 않는다. 기존 미전달/불확실 효과가
있으면 진단은 그 뒤에 보존한다. 그 효과를 재실행하거나 고정 sequence를 가진 송신물을 추월하지 않는다.
기존 동기 Full 대기와 raw EventNode 소비는 남아 있으므로 이 보존 표현을 비동기 pump·end-to-end
예약으로 읽지 않는다. LOAD/UNLOAD native 이전 결과 공간 확보도 이 표현 변경만으로 완료되지 않는다.
broker의 실제 destination `try_reserve`는 즉시 dispatch 한 번의 슬롯이며 미래/native/remote
grant가 아니다. raw 실패의 원본 반환은
[중립 event 계약](event-protocol-v2.md#local-refusal-ownership--limited-implementation-boundary)이 소유한다.
성공 경로의 큐/중복 원장 복사·owned claim 이전은 별도 미완이다.

#### 로컬 완료 저장소 예약 — 범위가 제한된 구현 계약

이 절은 `node_adapter/mailbox.rs`의 로컬 저장소 API만 소유한다. 현재 구현/실행 여부와 다음 순서는
로드맵·증거 기록을 따른다. 분산 grant, native 결과 예산 또는 B3 완료 계약으로 확대 해석하지 않는다.

- 비용은 `retained_event_bytes`가 Event의 inline 크기와 모든 독립 String/Vec의 **capacity**를 합산한다.
  wire 길이·len·같은 문자열의 중복 제거로 바꾸지 않는다. entry 부기는 claim에 추가하고 실제 사전
  할당된 큐 backing은 snapshot에서 따로 보고한다. allocator/waker/native 임시 메모리·RSS는 이 수치 밖이다.
- 예약·일반 publication·큐 보관·owned dequeue는 실제 같은 저장소의 count/byte 원장을 쓴다.
  `try_reserve`는 Event 한 개, `try_reserve_group`은 알려진 필수 결과의 footprint 목록 전체를
  한 임계구역에서 확보한다. 마지막 항목 실패나 최종 대조 전 경합/Closed는 부분 claim을 남기지 않는다.
  그룹은 move-only 항목을 입력 순서로 넘기며 꺼낸 항목은 그룹 취소와 독립된 수명을 갖는다.
- `completion_mailbox_with_limits`는 **전달 queue 슬롯과 retained count/bytes를 분리**한다.
  다중 결과의 보존 공간을 먼저 확보해도 queue 한 칸을 통해 순차 전달할 수 있다. 기존 생성자는
  queue와 retained count가 같은 호환 계약을 유지한다. 실제 composition root의 예산 선정/연결과는 별개다.
- 그룹의 실제 VecDeque capacity에 해당하는 backing bytes도 별도 claim으로 과금한다. 항목을 모두
  꺼내도 배열 파기 전까지 반환하지 않는다. 따라서 그룹 API는 명시 byte limit가 없는 count-only
  저장소에서 거부한다. 성공한 보존 공간의 상한이며 commit 전 동시 준비 배열·allocator·RSS 상한은 아니다.
- count-only 기존 생성자는 byte 상한을 선언하지 않는다. byte 예산 생성자를 일부 시험에서 썼다는
  이유로 composition root나 제품 Worker의 byte 제한이 활성화됐다고 하지 않는다.

| 전이 | Event 소유자 | 저장소 claim | 실패/취소 규칙 |
| --- | --- | --- | --- |
| reserve | 아직 생산 전일 수 있음 | move-only 예약이 실제 공간을 점유 | Full은 일시 부족, 전체 한도 초과/비용 overflow는 영구 거부; 취소는 정확한 몫 반환 |
| publish_reserved | 큐로 이동 | 원래 예약을 큐가 인수 | queue Full·잘못된 저장소·예약보다 큰 값·Closed는 원본 Event와 예약을 모두 반환 |
| take_owned | RetainedCompletion | 큐에서 빠져도 계속 점유 | raw Event 추출로 회계 책임을 지울 수 없음 |
| transfer | 새 실제 저장소로 이동 | 새 저장소 수용 성공 뒤에만 옛 claim 반환 | 실패하면 원본 Event·옛 claim·새 예약을 모두 보존 |
| retire | Event 폐기 | Event를 먼저 파기한 뒤 반환 | transport 수용/KV 정산/내구 완료와 다른 전이 |

예약된 front를 기존 raw `try_take/poll_take`가 소비하거나 건너뛰지 않는다. **예약 생산자와 owned
소비자·책임 이전을 함께 연결해야** 하며, 생산자만 먼저 운영에 켜면 정지한다. 기존 raw 소비는 큐에서
꺼낼 때 queue claim만 반환하므로 이후 EventNode의 보관 비용까지 추적하는 API가 아니다.
이 권한은 로컬 실제 저장소에 귀속되며 sender의 EventClass, source 인증, peer generation 또는 wire 권한을 대신하지 않는다.

전달 슬롯 반환은 저장소 claim 퇴역과 다르다. 분리된 한도에서 owned dequeue가 큐를 비우면 queue
waiter에게 통지하되 claim은 유지한다. 일반 발행의 queue Full은 임시 claim을 만들었다 취소하는
자기 wake를 발생시키지 않는다. 한 Event의 영구 byte 초과는 queue Full보다 먼저 거부한다.
모든 정상 capacity callback은 저장소/예산/등록부 잠금 밖이다. 그룹 취소는 미사용 claim과 실제
배열 회계를 모두 반환한 뒤 한 번 통지한다. 최초 caller panic은 전파하고 unwind 중 claim 해제는
회계만 반환하며 추가 capacity callback은 생략한다. panic 이후의 전달·진행이나 임의 RawWaker
destructor의 안전까지 보증하지 않는다. callback은 여전히 비차단·비panic 계약을 지켜야 한다.

`try_publish_deferred`·`publish_reserved_deferred`·`transfer_to_deferred`는 같은 실제 enqueue를
수행한 뒤 move-only `DeferredCompletionNotification`을 반환한다. 기존 즉시 알림 API도 같은
enqueue 뒤 이 반환물의 `notify`를 소비한다. 반환물은 호출자가 모든 잠금을 놓은 뒤 한 번 소비하며,
transfer의 옛 source 회계를 먼저 반환한 다음 reader/capacity callback을 호출한다. Drop은 source
회계만 조용히 반환하고 enqueue를 취소하거나 알림을 대신 실행하지 않는다. `notify` 누락은 메모리
누수가 아니라 진행 계약 위반이다. 반환물은 Waker 자체가 아니라 약한 등록 슬롯 참조를 보관한다.

**알림 지연은 가시성 장벽이 아니다.** 이미 실행 중인 reader는 notify 전에도 수용된 Event를 읽을
수 있다. callback panic을 enqueue 실패나 재전송 허가로 해석하지 않는다. 현재 raw broker는 아직
이 deferred 경계를 소비하지 않으므로, 이 API 변경만으로 broker 원장 잠금 밖 알림이 성립했다고
하지 않는다. 실제 broker/owned receiver의 제품 구성 이관이 남아 있다. 소유형 `RetainedEventBroker`는
별도 명시 경로로 deferred enqueue와 원장 밖 알림을 사용한다. raw 제품 경로의 동작 변경으로 세지 않는다.

단일 native 작업이 여러 필수 Event를 만들면 completion capacity=1에 그 전체 슬롯을 미리 요구하는
것만으로는 정상 진행할 수 없다. 후속 효과 보존 공간과 전달 큐 슬롯을 구분하고 실제 수신 측의 책임
이전까지 연결해야 한다. 이 한계를 큐 증설·SESSION 금지·기존 cap1 정상 입력 축소로 숨기지 않는다.
RELEASE의 미리보기 Event ID를 나중에 일반 Forward에서 다시 발급하는 것도 금지한다. 앞선 효과가
먼저 ID를 소비할 수 있으므로, 고정 Event의 한 번 발급은 실제 순서가 확정된 outbox 전이에서 이행한다.

#### broker 책임 이전과 정확한 중복 보관 — 연결 시 지켜야 할 목표 계약

현재 `event_broker::EventBroker::dispatch`는 성공하면 destination과 중복 원장에 Event를 각각
보관한다. 따라서 producer claim을 원장에 옮겨 넣는 것은 비용 분리가 아니다. 정확한 중복 원장의
퇴역까지 producer 공간이 묶여 새로운 순환 대기가 생긴다. 다음 조건은 **전체 연결 목표 계약**이다.

- destination의 실제 저장 claim과 exact Event 중복 사본의 독립 비용을 성공 commit 전에 확보한다.
  모든 실패에서 원 Event/producer claim을 반환하고 원장은 그대로 둔다. 원본의 책임 이전은 양쪽
  수용 뒤에만 끝난다. receiver가 dequeue한 뒤도 보관 중이라면 destination claim을 유지한다.
- 기존 비교는 Event 전체 equality이며 순서 영역은 `(source, correlation)`이다. hash-only 비교나
  byte 부족 시 조기 eviction으로 바꾸지 않는다. count-window의 정상 eviction을 반영한 최종 비용이
  한도를 넘으면 destination dequeue를 기다리는 일시 Full이 아니다. 저장 정책/명시 한도의 영구
  거부로 구분한다. 이것을 해제할 수 없는 capacity waiter를 등록하지 않는다.
- source claim의 retirement/capacity callback은 broker ledger 잠금 **밖**이어야 한다. 단지
  `transfer_to` 호출을 ledger 잠금 안에 감싸면 성공 뒤 claim Drop의 재진입으로 잠길 수 있다.
- 일반 `EventSender/Receiver`뿐 아니라 connection writer, EventNode 보류물, adapter input과 worker
  보류물까지 책임이 이어져야 한다. 중간에서 raw Event를 꺼내 claim을 바로 버리는 다리는 큐 한도만
  증명한다. remote write 성공은 receiver 수용 증거가 아니며 별도 grant/acceptance 계약이 필요하다.

실제 소유형 이관의 필수 연결 범위는 아래와 같다. **완료 목록이 아니라 연결 누락을 판정하는 표**다.
한 줄만 opt-in으로 바꾸고 다음 소비자가 raw 복사본을 장기 보관하게 해서는 승인하지 않는다.

| 제품 경계 | 같은 수명 안에서 반드시 보존할 대상 |
| --- | --- |
| `event_runtime::{run,control::create}` → broker queue/ledger | 실제 목적지 예산, 원본과 독립 중복 사본, 거부·eviction의 비용 |
| `EventNode` → `NodeAdapter` → `WorkerInput` | 양방향 held 값, offer 거부, 완료 dequeue, terminal 반환의 owner |
| worker → `RequestState`·`PendingRelease`·후속 effect | 장기 원문/파싱 결과의 비용, 후보 복사의 공유 범위, terminal 뒤 해제 출처 |
| `effects`/`emit`/`ack_service` → completion | 고정 Event·후속 관측·진단이 실제 저장소에 수용되기 전까지의 claim |
| control reply·`NodeOwner` 수명 | broker 거부, 완료 task 결과, DELETE/Drop의 보존 또는 명시적 종료 판정 |
| transport 수신 → broker → connection writer | 프레임/디코드 임시 공간, Full, 송신 실패·결과 불명, 원격 수용 전 책임 |

과거 Frame runtime을 이 표의 Event 경로로 세지 않는다. 같은 Event wire를 읽는 OUTER가 존재한다는
것도 원격 acceptance 증거가 아니다. 이관 상태와 실행 순서는 로드맵이 단독 소유한다.

**소유형 국소 전달 경계:** `RetainedEventBroker`는 raw broker와 등록/세대/전체 Event 중복 원장을
공유하는 타입 변형이다. `RetainedEventNode`는 필수 `RetainedNodeAdapter` 계약을 소비하며 raw
fallback이 없다. 새 경로에서 source claim은 실제 목적지의 queue slot·retained count/bytes를
함께 확보하고 enqueue한 뒤에만 반환한다. held input/output·terminal 반환은 claim을 유지한다.
독립 front는 envelope와 실제 capacity 비용을 읽어 즉시 목적지 예약 후 조건부 dequeue한다.
목적지 Full은 front를 그대로 두며 같은 `(source, correlation)`은 앞지르지 않는다.
ticket은 await/native 호출/원격 전송을 넘어 보관하지 않는다. route 등록은 enqueue까지 재검사하고
읽기 잠금으로 유지한다. source 퇴역·예약 취소·receiver 알림은 원장/등록 잠금 밖에서 실행한다.
전체 중복 사본과 count-window는 유지하지만 **중복 사본의 byte 상한은 아직 연결하지 않았다**.
이 타입 경계의 소비 시험은 runtime composition root·connection writer 이관 완료가 아니다.

**llama.cpp의 소유형 소비:** `RetainedLlamaNodeAdapter`는 같은 실제 Worker loop를 사용한다.
`WorkerInput::Retained`의 claim은 bounded std 입력 큐·현재 handle/native 호출·Full 중 비ACK 보류·
지연 ACK 오류의 원문과 함께 유지된다. 원문을 빌려 처리하며 raw Event clone으로 바꾸는 다리가 없다.
`try_publish_owned`는 기존 completion 저장소에서 현재 Event의 bytes/count를 수용하고 raw dequeue를
막는다. 미래 native 결과나 아직 materialize하지 않은 effect의 사전 예약을 의미하지 않는다.
중단 때 원인 입력·보류 입력·ACK 원문·미처리 receiver·state/effect를 adapter owner에 보존한다.
owner Drop은 명시적 국소 폐기이며 성공 drain/replay 허가/원격 수용이 아니다. 입력 원문과 별개로
파싱/후속 효과/중복 사본의 독립 byte 예약, runtime control·writer의 책임 연결은 남아 있다.

#### 불변 수용 입력과 가변 진행 후보

**수용 입력 저장 예산 (2026-09-11 구현):** `node/request_budget.rs`는 head의 pending/active 요청과
후속 효과가 공유 보관하는 입력을 같은 계정으로 제한한다. 기본 한도는 4,096개 입력,
512MiB 보관 footprint, 입력 토큰 합계 16Mi, `max_tokens` 합계 16Mi다. 슬롯 수와 별도이며
수용 거부가 request/session-key/incarnation/free-slot/기존 요청의 예약을 변경하면 안 된다.
footprint는 원본 Event와 정규화 command의 독립 allocation capacity, reply 및 보수적인 입력/부기
비용을 포함한다. tokenizer의 일시 메모리·KV·발행 row/capsule·미래 출력 바이트·broker receipt는
이 계정 밖이다. 출력 토큰 예약은 미래 출력 **바이트** 예약이 아니다. B2/B3 전체 완료를 뜻하지 않는다.

예약은 모든 수용 검증과 read-only tokenization 뒤, 첫 admission write 전에 확보한다.
입력은 `Arc<RequestInput>`로 공유하며, 예약도 한 실제 입력에 한 번 과금된다. request map에서
제거돼도 공유 입력이 살아 있으면 반환하지 않고, 마지막 input의 데이터가 파기된 뒤 반환한다.
`P4_STAGED_TRACE_REQUEST_STORAGE`는 수용 시 해당 node/load의 입력 계정 값을 기록한다.
정상 완료·전폭 해제·slot 재사용의 계정 퇴역은 실제 Worker loop 시험으로 검사한다.

수용 정규화가 끝난 command(tokens/options 포함), 원본 Event, reply 출처는 이후 후보 전이의
변경 대상이 아니다. `RequestState` 복사는 이 입력을 공유하고 prompt/ready/outstanding 등 진행
상태만 독립적으로 복사한다. 생산 경로에는 공유 입력의 가변 접근자·copy-on-write를 제공하지 않는다.
발행 중 관측/오류 보고가 원 요청보다 오래 살아야 하면 읽기 전용 공유 소유자를 유지한다. 정당성은
원본 요청에 있으며 현재 PHYSICAL/TAIL/ACK의 출처로 바꾸지 않는다.

공유 allocation은 복제 가능한 **데이터 소유권**이지 선형 transport claim 또는 공간 예약이 아니다.
후속 owned 이관에서 이 둘을 같은 Arc에 숨겨 회계를 생략하지 않는다. wire 원문과 파싱한 tokens가
처음 생성될 때의 별도 비용, ready/continuation/RowOwner 및 native 결과 비용도 남는다. 공유 후
복사가 줄었다는 사실로 byte admission·RSS 상한·배치 처리량을 승인하지 않는다.

#### 선택적 파이프라인 요청 묶음 (2026-09-11)

`P4_STAGED_PIPELINE_BATCHING=1`은 ordinary attention에서 생성 우선 배정과 독립 prefill 집단을 사용한다.

이 정책의 `min_batch_rows`는 실제 준비된 계획이 decode-only일 때만 적용한다. 같은 SESSION의
eligible decode 수가 임계값보다 작고 기존 flight가 있을 때, 최초 대기 결정에서 단조시계2ms를
부여한다. 추가 input은 이 시각을 연장하지 않는다. Worker의 `recv_timeout`이 새 input 없이도
다시 발행 조건을 검사하게 한다. OS 스케줄링·native/제어 실행시간을 포함한2ms 응답 보장은 아니다.
prefill을 포함하는 계획은 기존 row/member/quantum 한도 안에서 즉시 진행한다. legacy·atomic/equal
경로에는 이 변경을 적용하지 않는다. 빈 계획·준비할 작업 없음은 대기를 지우고, full flight·fence 등
다른 차단 사유는 timer를 disarm한다. 만료로 native/KV/flight/저장 공간 권한이 생기지 않는다.
`SchedulingSnapshot.pipeline.decode_coalesce_max_ms`가 적용 상한을 기록하며 과거 관측에는 없을 수 있다.
prefill 참여 상한은 수용된 미완료 prompt(ready/inflight/waiting)를 기존 창에 나눈 값이다.
pending admission과 마지막 prompt 반환만 기다리는 요청은 이 집단에 넣지 않는다. 순간 빈 flight 수로
집단을 합치지 않는다. decode는 빈 flight 수로 나누지 않고 먼저 행을 예약하며 명시적인
`OrdinaryLimits.decode_members`와 실제 행 용량은 지킨다. pure-prefill의 전체 행 예산은 유지하므로
16개 긴 입력·창8·batch512는 2요청×256행을 발행하고 나머지 요청으로 다음 배치를 준비할 수 있다.
노드 수나 GPU 수를 실행비용 대신 사용하지 않으며 이 산술을 처리량 최적값으로 보증하지 않는다.

같은 session에서 decode가 진행 중이거나 마지막 prefill의 반환을 기다리면, 현재 eligible decode가 없어도
`P4_STAGED_MIXED_PREFILL_ROWS`(실험 초기값128)의 prefill 행 예산을 적용한다. 마지막 decode가
끝나면 pure-prefill 예산으로 돌아간다. native 호출은 비선점이며 이 행 예산은 시간/SLO 보장이 아니다.
참여 요청 선택의 회전과 prepare/validate/commit fairness는 기존 scheduler 권위를 유지한다.
선택 결과는 `SchedulingSnapshot.pipeline`에 window/open/decoding_active/effective_limits로 기록한다.

명시적 `max_open_batches > 0`, `mixed_prefill_rows > 0`, fragment1을 요구한다. 잘못된 조합은
tokenize·admission·KV 전에 요청 오류로 돌려준다. Verify/Replay·등폭 recurrent 경로에는 적용하지 않는다.
기본 비활성이다. 이 요청 묶음 자체는 창 증가·다중 fragment·시간 비용 모델·전 구간 B2/B3·성능 승격을 포함하지 않는다.

#### 선택적 pipeline RPC 서비스 예산 (2026-09-12)

`P4_STAGED_PREFILL_SERVICE_MS`를 양의 정수로 설정하면 위 정책에 pipeline RPC 완료시간 예측과
prefill 청크 재선택을 추가한다. 이전 stage별 backlog 정책과 같은 숫자가 같은 지연 목표를 뜻하지 않는다.
기본 비활성이며 ordinary attention·fragment1·pipeline policy·open1–128·stage2–64만 허용한다.
같은 실행에 참여한 모든 agent에서 켠다. stage는 실제 Frame 호출 전후의 단조시계 시간과 정확한
load/session/execution 목록·phase별 행 수·요청 수·최대 입력 position을 head로 보낸다.
head는 선언된 stage 출처와 수용된 membership을 검사한다. exact duplicate는 학습하지 않고,
같은 execution/stage의 충돌은 상태 변경 전에 거부한다. 이 정보는 flight/KV/edge를 퇴역시키지 않는다.

decode-only도 피드백을 보내며 open 작업의 비용에 포함한다. 동일 stage/load/session/행 수/요청 수와
최대 position의2진 구간은 최근8표본의 최댓값을 사용한다. 같은 구간의 미측정 폭은 행/요청 증가율로
보수적으로 확대 예측한다. 두 폭에서 각각2회 이상 관측됐고 prefill 폭이2배 이상·비용이 증가했으며
decode/요청 수가 같으면 고정비를 반복 곱하지 않는 affine 증가 비용을 사용한다. 서로 다른 stage와
context 구간은 합치지 않는다. 실제 backend `n_kv`·mask·kernel 비용을 안다는 뜻이 아니며 hard 상한이 아니다.
history128·profile256으로 제한하고 완료된 history만 교체한다. UNLOAD는 이 비용 이력을 지운다.
늦은 이전 load의 비용 피드백은 무시하며 완료한 load를 다시 열거나 출력 권한을 만들지 않는다.

생성 서비스가 필요하면 open 배치와 후보를 실제 발행 순서로 모든 stage에 투영한다. 각 stage의 다음
예상 완료는 `max(앞 stage 도착, 앞 배치 완료)+RPC 비용`이다. 실제 완료 표본이 온 stage와 그 앞 단계의
실행은 다시 청구하지 않는다. 이는 비용 추정만 바꾸며 정산/출력 권한을 반환하지 않는다.
후보의 마지막 stage 완료 예측이 예산을 넘거나 비용을 모르면 prefill 행을 절반씩 줄여 새 계획을 만든다.
최대 탐색 횟수는 행 수의 bit 폭이며, 준비/거부한 후보는 fairness·요청·flight를 변경하지 않는다.

모르는 비용은0이 아니다. 미완료 prefill이나 정체불명 open 작업이 있으면 `calibration_wait` 또는
`defer_prefill`로 생성만 다시 계획한다. 단, 큰 prefill의 반환을 기다려야만 작은 shape를 학습할 수 있는
초기화 장벽을 피하기 위해, 미측정 후보는 기존 창 안에 최대1개의1행 calibration probe를 허용한다.
이미 prefill1행 작업이 open이거나 open identity를 모르면 새 cold probe는 보내지 않는다.
이전 prefill이 모두 정산되면 최소1행을 `cold`/`progress_probe`로 측정해 영구적인 prefill 기아를 피한다.
거부된 원래 큰 quantum을 그대로 재허용하지 않는다.
prefill0은 `OrdinaryLimits`의0(무제한)으로 표현하지 않고 prefill 수요를 제외해 계획한다.
생성 서비스가 아직 필요 없는 pure-prefill은 기존 전체 폭을 유지한다.

`SchedulingSnapshot.service_budget`은 판정·예산·기존 stage backlog 집계·known stage 수 외에
`predicted_tail_rpc_us`, `examined_prefill_rows`, `selected_prefill_rows`를 기록한다.
기존 wire에서 생략되면 None이며 기본 비활성 출력 형식은 그대로다. decode-only로 대체한 발행도
원래 후보의 `defer_prefill` 판정을 보존한다. decode가 ready가 아니라 발행 자체를 미룬 경우는 다음 실제
input/피드백/정산에서 재검사한다. 현재 이 대기의 전체 시간·이유는 생산 관측에 별도 집계하지 않는다.

이는 **전체 pipeline RPC의 soft 예산**이다. 아직 확인되지 않은 실행에는 전체 예상 비용을 보수적으로
청구하며 실제 시작 후 경과시간을 빼지 않는다. 전송·dispatch·sampler 반환까지 포함한 client 완료시각,
요청별 deadline/시간 deficit/aging, 전체 반환 선예약은 아직 아니다. 일반 attention만 대상으로 하며,
혼합하지 못하는 equal-width hybrid에 같은 물리 모양을 강제하지 않는다.
작은 예산은 GPU 공급도 줄일 수 있다. c6bd6c597의 MI250 대조6회에서 전량 decode 합류는 독립
flight와 생성 성능을 줄였고, 시간250ms는 긴 prefill 중 ITL을 줄이는 대신 prompt 완료와 전체 처리량을
악화시켰다. 현재 동작의 설명을 권장 정책으로 읽지 않는다. 기본 비활성을 유지하고 성능 승격은 거부한다.
다음 변경 계약은 [로드맵](distributed-batching-roadmap.md#v11-plan), 측정/반례는
[6arm 판정](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-11-v1.1-inflight-diagnosis.md#generation-service-screen)이 소유한다.

시험 fixture의 불변 입력 변조는 명시적인 test-only COW로만 허용한다. 후보의 원본 공유/진행 격리와
거부 후 원상보존을 allocation 동일성과 값 대조로 함께 검사한다. 마지막 읽기 소유자가 남아 있는 동안
입력이 유효해야 하고, 마지막 소유자가 사라지면 퇴역해야 한다. 실제 소비와 실행 지위는 증거가 소유한다.

## 측정: 배치 폭 대 파이프라인 깊이 (2026-08-31, 2026-09-01 재측정)

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

**2026-08-31 관측 (RTX 4080 + 3090, 비대칭 배치): 처리량은 손해였다.**

| 시나리오 | 정책 | rows/batch | ms/batch | 생성 TPS | 의미 판정 |
| --- | --- | --- | --- | --- | --- |
| 연속 도착 | 기본 | 2.84 | 24.5 | **108.9** | 40/40 |
| 연속 도착 | 임계값 8 | 10.54 | 102.3 | 96.9 | 40/40 |
| 연속 도착 | 전량 병합 | 10.56 | 103.2 | 96.2 | 40/40 |
| 웨이브 | 기본 | 13.57 | 68.2 | **195.4** | 40/40 |
| 웨이브 | 임계값 40 | 30.41 | 213.5 | 139.8 | 40/40 |

행이 3.7배 늘 때 스텝 시간이 4.2배(연속), 2.2배 늘 때 3.1배(웨이브) 늘었다.
**이 구성에서 스텝 비용은 고정비가 아니라 행 수에 지배되었다.**

**2026-09-02 재측정 (RTX 3090 x2, pin `0eadefebd`, 동일 launcher, fence 검증)**

앞선 측정들은 launcher·pin·배치 정책이 실행마다 달랐고 증거가 stale 기록을
통과시킬 수 있었다. 이 표는 **하나의 launcher와 하나의 pin**에서, 실행마다
fence로 잘라낸 자기 기록만으로 판정한 4회다.

| 시나리오 | 정책 | rows/batch | ms/batch | 물리 배치 | 생성 TPS | 혼합 배치 |
| --- | --- | --- | --- | --- | --- | --- |
| 연속 도착 | 기본 | 6.38 | 117.3 | 1,994 | 51.15 | 5 |
| 연속 도착 | 임계값 24 | 16.78 | 264.2 | 758 | **59.73** | 19 |
| 웨이브 | 기본 | 18.41 | 164.3 | 2,212 | 109.92 | 0 |
| 웨이브 | 임계값 40 | 34.19 | 304.3 | 1,191 | 110.27 | 2 |

연속 도착에서 행이 2.6배 늘 때 스텝이 2.25배 — **선형 미만**, 처리량 +17%.
웨이브에서는 행이 1.86배 늘 때 스텝이 1.85배 — **정확히 선형**, 처리량 동률
(109.92 대 110.27, 0.3% 차이).

**그리고 기준선 자체가 이봉이다.** 같은 pin·같은 정책의 웨이브 기준선이 어떤
실행에서는 폭 13.57·3,000배치·93.7 tok/s로, 다른 실행에서는 폭 18.41·2,212배치·
109.9 tok/s로 나온다. 무리가 합쳐지느냐가 타이밍에 달렸다는 뜻이고, **단일
표본으로는 어느 정책도 특징지을 수 없다**. 앞서 이 문서가 "임계값 40이 94를
111.8로 회복시켰다"고 적은 것은 파편화된 기준선 1회와 임계값 1회를 비교한
것이었다 — 회복은 실재하지만 그 크기는 기준선이 어느 봉에 떨어지느냐에 달렸다.

**계약**: 임계값의 부호는 파편화 유무가 정하고, 파편화 자체가 타이밍 의존이다.
따라서 이것은 상시 정책도, 실험으로 한 번 정해 둘 값도 아니다 — 노드가 자기
배치 폭을 텔레메트리로 보고하고 그에 반응해야 한다(P1a). 기본값은 꺼짐인데,
파편화가 없을 때 스텝 지연만 1.85배로 만들고 아무것도 얻지 못하기 때문이다.

**측정의 한계 (2026-09-02, 정정)**: 이 문서는 앞서 "clean commit에서 기준선 6표본"이라고
적었다. **틀렸다.** 각 실행의 evidence를 확인하니 `repo_commit`은 모두 `ed846d920`이지만
clean인 것은 50.40 하나뿐이고 나머지 다섯은 서로 다른 dirty diff 4종에서 나왔다.
커밋이 같다고 소스가 같은 것이 아니다 — 또 산출물을 열지 않고 쓴 문장이다.

같은 소스 상태(dirty `f63266ff`)에서 나온 네 건만이 짝 비교로 쓸 수 있다.

| 정책 | 생성 TPS | 소스 상태 |
| --- | --- | --- |
| 연속 도착 기본 | 50.45, 52.82 | `f63266ff` |
| 연속 도착 임계값 24 | 58.74, 58.93 | `f63266ff` |

이 짝에서 임계값은 +14~17%다. 나머지 기준선 표본(50.08 / 50.40 / 53.41 / 54.96)은
**서로 다른 소스 상태**이므로 한 모집단으로 묶을 수 없고, 리팩터링 전후 비교에도
쓸 수 없다.

**비악화는 여전히 판정 불가다.** 이유는 둘이다 — 기준선의 실행 간 산포가 약 10%로
리팩터링이 낼 만한 변화를 덮고, 비교하려던 표본들의 소스 상태가 섞였다. 판정하려면
**동일 소스 상태에서 표본을 크게 늘리거나** 산포가 낮은 지표가 필요하다. 파편화가
산포의 원인으로 보이므로 노드가 자기 배치 폭을 보고하면(P1a) 폭을 조건으로 나눈
비교가 가능해진다.

**출처 없는 수치의 지위**: 8월 31일 표의 실행들과 9월 1일 오전 웨이브 실행
두 건(110.6·110.1)은 provenance를 남기지 않는 스테이지 바이너리로 측정됐고,
그 바이너리는 지금 없다. 기준선이 아니라 **식별 불가능한 빌드의 관측**으로
읽어야 하며, 이것이 HELLO에 `patch_set`을 넣고 실행이 기대 pin과 대조하게
만든 이유다.

**과거 비용 해석 (현재의 보편 결론이 아님)**: 이 문단은 앞서 "왕복 지연을 줄이는 것,
즉 홉당 cut-set 전송 비용(D7/P6)"이라고 적었다. **전송이 아니다.** 스테이지별
span을 재 보니 노드 2→3 홉은 꼬리가 비어 있을 때 **2 ms**(p50)이고 바쁠 때
**128 ms**이며, 배치의 63%가 바쁜 꼬리를 만난다. 130 ms 평균은 전송이 아니라
꼬리 앞 대기가 크게 섞여 있었다. 이 특정 링크/모델 관측으로 다중 머신 전송이 항상 싸다고 결론내리지 않는다.

**(2026-09-04 정정) 그 다음 문단이 틀렸다.** 아래 상관은 폭이 *결과*인 실행들에서
잰 것이다 — 첫 노드는 준비된 것으로 계획하므로, 빠른 실행일수록 준비 집합이 빨리
비고 배치가 좁아진다. 폭을 정책으로 고정해 *원인*으로 만들자 부호가 뒤집혔다:
발행 폭 상한(`P4_STAGED_MAX_ISSUE_ROWS`) 교차 8회에서 **폭 +0.898, 동시 계산
−0.060**이다. 상한 12에서 동시 계산 95.4%·GPU 사용률 최고·혼합 배치 3,698건을
달성하고 총 처리량은 544 → 198 rows/s로 떨어졌다.

이 실험에서 유력한 후보는 **배치당 고정비**였다. 꼬리(22층) 스텝시간은 폭에 대해
**34.2 ms/배치 + 1.051 ms/행**이고, 폭 8에서 0.208 행/ms, 폭 98에서 0.705 행/ms로
**넓은 배치가 행당 3.4배 효율적**이다. 곡선은 폭 98에서도 아직 오르는 중이다.
바쁜 스테이지는 그 고정비를 반복해서 내느라 바빴을 뿐이다.

아래는 정정 전 기록이며 반증된 문단으로 남긴다.

~~실제 레버는 **동시에 계산 중인 스테이지 수**다. 동일 시나리오 10회에서 생성~~
~~TPS와의 상관은 2개 이상 동시 계산 비율 **+0.923**, 꼬리 가동률 +0.684, 배치 수~~
~~+0.213, 열린 깊이 +0.121, 그리고 **배치 폭 −0.211**이다.~~

그리고 그 동시성을 막는 것은 스케줄이 아니라 균형으로 보인다. 스테이지 비용은
**배치당 고정 54.7 ms + 층당 3.04 ms**로, 22층 꼬리가 121 ms이고 4층 스테이지가
63 ms다. 컷 5/4/4/22는 KV 공유(13~34층) 때문에 고정이지만 배치는 아니다 —
기본 배치는 꼬리와 노드 2를 같은 카드에 두어 겹쳐야 할 둘이 경합한다.

**스텝 비용의 정체 (2026-09-04)**: 스테이지 서버 안을 네 구간으로 계측하니
파싱·소유자 매칭·응답 인코딩은 합쳐 3 ms 미만이고, 비용은 `llama_decode`와
**샘플링** 둘뿐이다. 꼬리에서는 샘플링이 더 크다 — 22개 레이어를 도는 데 행당
0.11 ms, 토큰을 고르는 데 **행당 0.29 ms**로 샘플러가 트랜스포머의 2.7배다.
어휘가 249,157 이상이고 후보 배열을 행마다 단일 스레드로 만들기 때문이다.

**과거 병렬 sampler 실험이며 운영 승격 증거가 아니다.** 공유 llama_context의
synchronize/output reorder 안전성이 미검증이므로 현재 기본 직렬을 유지한다.
당시 교차 8회(블록 순서 반전)에서
생성 TPS 194.06±6.54 → **213.45±8.09 (+10.0%)**, 폭을 맞춘 비교에서 샘플링
−28%~−49%(9~128행 구간). **분포가 겹치지 않는다** — 최저 병렬 204.7이 최고 직렬
202.3보다 높다. 8회 모두 192/192 통과. 이 기록에서 교차 검증을 견딘 첫 변경이다.

`P4_STAGED_SAMPLE_THREADS=1`이 직렬 동작을 정확히 복원하며 그것이 대조군이다.

**배치(placement)도 판정 불가다.** 꼬리에게 카드를 통째로 주면(3+1) 꼬리 스텝이
12.7% 빨라지고 가동률이 10.3% 오르지만(짝 4회 모두), 처리량은 +2.7%로 산포 안이다
— 최고의 기본 실행이 네 꼬리단독 중 셋을 이긴다. 처음 두 쌍만 보면 +4.8%였다.

**그리고 세션 자체가 표류한다.** 오늘 18회에서 실행 순번과 꼬리 가동률의 상관이
+0.696, 배치 폭과는 −0.700이다. 뒤에 돌린 실행일수록 꼬리가 바쁘고 배치가 좁다 —
정책과 무관하게. 순차 스윕은 이 기울기를 그대로 물려받으므로 **증거가 아니다.**
이후의 모든 정책 비교는 교차 실행이어야 한다.
**발행 정책은 판정 불가로 남는다.** 꼬리 인지 발행 상한
(`P4_STAGED_MAX_OPEN_BATCHES`)을 0/2/3/4/8로 시험했을 때 순서대로 돌린 스윕은
+21%로 단조 증가했으나 **순서를 섞자 사라졌다** — 대조군 자체가 170.8~216.9로
27% 벌어진다. 이 산포를 줄이기 전에는 어떤 발행 정책도 판정할 수 없다.

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
세션 상태는 참여 노드 N개에 조각나므로(`stage_id`, `operation_id`,
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

현재 구현은 staged adapter의 `src/v2/`에 있다. 기전과 정책을 실제로 공유하고 검증한 뒤
별도 크레이트가 의존성 경계를 개선하는지 결정한다. 크레이트 생성·골든 재생만으로 공유 전이 완료를 주장하지 않는다.
## 실패 이력의 층별 귀속

| 관측된 실패 | 귀속 층 |
| --- | --- |
| 40요청 position 불연속 (슬롯 재사용) | L1이 불변식 위반을 검출; 원인 규명·수정은 계획 P1b |
| 혼합 배치 0건 | 합류/shape/도착/정책 관측; 깊이와 실제 device overlap은 별도 계측 |
| gemma-4 로드 거부 후 5GB 로드 낭비 | L1 단가표 + 로드 전 preflight |
| Qwen3.6 VRAM 초과 사후 발견 | L1 단가표 (로드 전 계산) |
| cut-set 81전송 고정비 | L5 + cost_model |
| compute buffer 과할당(점유 3.5%) | 계획 시점 n_ubatch (OUTER 입력) |
| 셀 고갈 시 무작위 세션 실패 | L2 — unified에 격리가 없으므로 정책이 격리 |

## 도입 순서

단계·순서·수용 기준은 [분산 배치 로드맵](distributed-batching-roadmap.md)이
단독 소유한다. 이 문서는 L0~L5 계약만 소유하며 순서를 재서술하지 않는다.
