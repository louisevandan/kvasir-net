# 초대형 모델 분산 배치 — 현재 상태와 실행 로드맵

문서 기준일: 2026-09-06. 코드 감사 기준: `a9e1967fc59dffa6c2e458f1b91f916b1df826c1`.
이 파일은 **현재 목표·상태·작업 순서·단계 승격의 단독 소유자**다.
시험 상세와 실기 판정은 [검증 규약](distributed-batching-verification.md), 계층별 책임/업데이트 격리는
[격리 계약](layer-isolation-contract.md), 기존 문서의 역할은
[문서 안내도](document-map.md)가 소유한다. 최초 문서 이관과 후속 구현을 구분한다.
기준 커밋의 감사는 §3, **후속 구현·체크포인트와 다음 행동은 마지막 진행 기록**을 따른다.
최신 진행 기록은 [실제 전달 거부의 원본 소유권](#2026-09-07-후속-구현--실제-전달-거부의-원본-소유권)이다.
이전 진행 기록 안의 “다음”은 당시의 순서이며 현재 지시가 아니다. 현재 반례와 단계 상태는 최신 기록을 우선한다.

## 1. 완료해야 할 제품 목표

**초대형 모델을 여러 물리 컴퓨터에 분산된 노드에서 실행하고, 강한 요청 웨이브를
연속으로 받아 정상적인 프롬프트·응답을 유지하면서 유효 생성 TPS와 GPU 활용을 최대화한다.**

- 한 호스트의 GPU 두 장에 프로세스 네 개를 띄우는 것은 다중 컴퓨터 증명이 아니다.
- 작은 모델과 35B는 개발·교란 분리·회귀 기준선이다. 그것만으로 초대형 모델 목표를 완료할 수 없다.
- 노드 수는 모델 가중치·KV 수용량·합법적 컷·머신/장치 배치의 제약이다. 필요한 노드를 줄여
  성능을 높이는 것은 고정 토폴로지 배치 전략의 개선 증거가 아니다. 같은 장치의 여러 노드도 지원 대상이다.
- 노드를 늘려도 가중치 중복·공유 KV·가장 빡빡한 스테이지 때문에 수용량이 선형 증가한다고 가정하지 않는다.
- “최적”은 선언한 모델·하드웨어·SLO·워크로드·탐색 범위에서 검증한 최선이다. 전역 최적이나 GPU 100%를 약속하지 않는다.
- 유효 생성 처리량과 유용한 GPU 계산을 함께 개선한다. 사용률을 올리기 위해 작은 배치·재계산·polling을 늘려 TPS를 낮추지 않는다.
- **최종 성과 증거는 정상 프롬프트와 응답 전문을 보존한 다중 컴퓨터 실기 강한 웨이브 실행뿐이다.**
  결정론적 시험과 mock은 그 실행에 들어가기 위한 필수 안전성 게이트이지 최종 성과 증명이 아니다.

### 현재 승인된 자원과 실기 확장 순서 (2026-09-07)

사용자가 지정한 모델 후보는 **`S:\models` 전체**, 실기 GPU 예산은 **RTX 3090 두 장**이다.
RAM 오프로딩도 허용하며, **VRAM-only에서 충분한 검증을 마친 뒤 RAM 오프로딩이 필요한 더 큰 모델**로
확장한다. 이 순서는 B6/B7 안의 자원별 검증 순서이지 B1~B5의 안전성 게이트를 건너뛰는 허가가 아니다.

1. 모델 디렉터리 전체를 inventory로 만든다. split GGUF는 한 모델로 묶고 mmproj/LoRA/embedding 등
   보조·비생성 artifact를 구분한다. 각 논리 모델/variant에 감사 상태·필요 자원·선택 또는 제외 이유를 남긴다.
   파일 크기나 이름만으로 지원/실행 가능을 승인하지 않으며, 지원하지 않는 memory family를 성능 시험 때문에 열지 않는다.
2. **VRAM-only 기준선**: 감사된 모델 중 실제 가중치·KV·compute/전송 buffer·상주 동시성을 두 GPU 안에
   수용하는 모델로 검증 규약 H0의 자원 단계 게이트를 통과한다. 작은 모델만으로 큰 모델 배치 적합성을 판정하지 않는다.
3. **RAM 오프로딩 확장**: 앞 게이트를 통과한 뒤 GPU+RAM의 실제 예산 안에서 더 큰 후보를 평가한다.
   CPU 계산, host-resident 가중치/KV, staging/pinned buffer, 전송을 구분하고 placement를 봉인한다.
   각 후보의 load/정상 응답/웨이브/메모리/성능 판정을 따로 기록한다. 정상 거부·자원 부족·미감사는 통과가 아니다.
4. 각 자원 단계의 정책 A/B는 모델·quant·placement·context·resident·워크로드를 고정한다.
   다른 모델의 VRAM-only TPS와 RAM 오프로딩 TPS 차이를 배치 정책 효과로 계산하지 않는다.

읽기 전용 확인에서 두 3090은 **M42-SERVER2 한 물리 호스트**에 있었다. 이 fleet에서의 성과는
단일 호스트/두 GPU 검증이다. 다중 컴퓨터라는 장기 목표와 H6은 별도이며, 다른 호스트를 임의로
추가하거나 RAM 사용을 두 번째 컴퓨터의 증거로 세지 않는다. H6 미충족 때문에 승인된 로컬 안전성·
현재 자원 내 실기 검증까지 중단하지도 않는다. 최종 목표 전체 완료와 현재 자원 내 완료를 구분한다.

`S:\models`는 실행 계정에서 접근을 확인한다. SSH 비대화형 세션에서 S:가 보이지 않는 사실만으로
모델 부재를 선언하거나 경로를 임의 변경하지 않는다. 접속 정보 문서는 저장소 밖에 두고 암호/토큰은
명세·로그에 복사하지 않는다. 모델 선택과 예산은 검증 규약 H0의 명세에 고정한다.

2026-09-07의 현재 로컬 실행 계정에서는 S:를 읽을 수 있었다. GGUF 경로/크기/mtime 예비 목록은
`target/model-file-inventory-20260907-01.json`에 보존했다(156파일, 파일명으로 묶은63그룹).
파일명 기반 임시 분류는 모델 후보40/embedding1/projector22이며 shard 번호 누락은 없었다.
이는 GGUF 헤더·전체 content hash·family 감사·메모리 계획·원격 계정 접근·load 성공의 증명이 아니고,
non-GGUF 전체 목록도 아니다. H0의 논리 모델/variant inventory를 완료했다고 읽지 않는다.

## 2. 새 세션의 첫 30분

1. 저장소 루트에서 `git status --short --branch`, `git log -5 --oneline`으로 기준을 확인한다.
   기준 커밋 이후 변경은 §4의 소스 경로에서 감사하고, 다른 사람이 남긴 dirty 변경은 보존한다.
2. 이 문서와 검증 규약·격리 계약을 읽는다. 모든 역사 문서를 처음부터 읽고 과거 순서를 복원하지 않는다.
3. `entrypoints/agent/src/main.rs::main`을 확인한다. 기본은 `event_runtime`이고
   `P4_AGENT_SERVICE_RUNTIME`은 과거 Chain/Hop 경로다. 후자의 fairness/queue 시험을 현재 경로 증명으로 세지 않는다.
4. §3의 기준 커밋 감사와 마지막 진행 기록을 대조한다. 이미 고정한 반례를 다시 발견했다고 하지 말고,
   현재 남은 반례를 실제 worker/native 소비 경로에서 먼저 고정한다.
5. 검증 규약의 현재 실행 명령을 수행하고 실제 실행/제외/실패를 기록한다. 숫자가 이전 기록과 다르면 이유를 설명한다.
6. 첫 미통과 단계 B0의 증거를 확인한 뒤 마지막 진행 기록의 B1/B2 잔여부터 계속한다.
   후속 구현을 무시하고 수정 전 테스트로 회귀시키거나, B1 전체가 끝났다고 전제하지 않는다.

모델 후보 경로와 현재 GPU 범위는 §1의 사용자 지정을 따른다. 아직 확정하지 못한 입력은
실행 계정의 파일 접근, 후보별 전체 artifact identity/메모리 계획, 추가 다중 호스트 자원이다.
기존 하네스의 경로·IP·계정은 예시가 아니라 과거 구성값이다. 사용 가능성과 권한을 다시 확인한다.
입력이 없다고 작은 모델이나 한 호스트를 최종 대상으로 자동 대체하지 않는다.

## 3. 감사된 현재 상태

아래 `확인`은 기준 커밋에 한정한다. 과거 GPU 수치는 이번 HEAD의 재측정이 아니다.

| 영역 | 판정 | 근거와 남은 일 |
| --- | --- | --- |
| 현재 실행 경로 | 확인 | `main.rs::main` → `event_runtime`; 실제 event worker 사용 |
| 배치 선택기 | 부분 구현 | `scheduler.rs::plan_equal_ordinary`: cohort별 sequence ID 회전, patience; `plan_ordinary`: 일반 행 배분. 전체 분산 스케줄러가 아님 |
| 요청별 기아 반례 | 해당 반례 해소 | 17 prefill·decode 1·용량 8·900회 계획에서 모두 진행. 최대 간격 끝 구간 포함; 실시간 TTFT 보장은 아님 |
| 정산 공유 | 일부 확인 | `node/state.rs::RequestState::settle_fragment`를 worker와 simulator가 호출. outstanding·prompt cursor·ready 일부만 공유 |
| 정산 전체 원자성 | **미해결 R-A** | `worker/release.rs::Worker::tail`: 검증 전 open execution 제거, 여러 요청 중 일부 먼저 변경. 오류 발행 후 worker가 계속될 수 있음 |
| 출력 효과의 승인 경계 | **미해결 R-D** | `worker/drive.rs::Worker::emit_tail_results`: 꼬리가 OUTER 출력 후 head 반환을 발행. head 원자화만으로 malformed 반환의 외부 부작용 0을 증명할 수 없음 |
| 발행-반환 동일성/멱등 | **미해결 R-B** | 이전 execution 반환이 다음 fragment를 소비; outcome 없는 부분 prefill은 다른 sequence와 위치 구간도 수용 가능 |
| 공유 경로 회귀 시험 | **미해결 R-C** | `simulator_tests.rs::the_worker_and_this_model_settle_through_one_transition`은 직접 함수 시험. 실제 Simulation 호출을 별도 부기로 바꿔도 11개 통과한 검수 반례 |
| simulator | 고정 지연 완료 모델 | 실제 selector 사용. admission·RPC·분할·credit·release/shutdown을 모두 통과하지 않음 |
| worker 시험 | tail 지점만 | 인코딩된 CapsuleSet의 정상/과다행/미발행 거부 3건. 가짜 stage와 전체 worker loop는 아직 없음 |
| 열린 배치 원장 | 일부 | 논리 배치별 execution 집합 존재. 요청 정산과 한 transaction으로 결속되지 않음 |
| backpressure | 일부 | 양방향 보존/Full·Closed 구분 존재. capacity notification·완전 drain·취소 검증은 남음 |
| admission/credit | 미완 | 슬롯 외 KV 셀 예약, bounded pending, 다중 노드 예약과 edge row/byte credit 완결 필요 |
| native 호환 경계 | 부분 | `src/` 격리, pin/patch 큐 존재. `common/` 잔여, 제품 identity 강제·실제 placement 결속·backend conformance 남음 |
| 영속 KV/스냅샷 | 기반 구현 + 목표 계약 | 파일/영수증/코디네이터가 존재. 새 namespace·정체성·수렴·스냅샷 계약이 전부 구현된 것은 아님 |
| 실기 하네스 | 개발 기반 | `test/benchmarks/p4-4node/` 추적. 현 remote 구성은 원격 한 호스트 안에 stage들을 배치; 임의 다중 호스트 최종 runner는 보강 필요 |
| 최종 초대형 모델 웨이브 | **미증명** | 정상 응답·다중 호스트·봉인된 반복 비교를 동시에 만족하는 완료 증거 없음 |

위 표는 기준 커밋의 초기 감사다. 이후 수정 전 실패 반례를 RED로 봉인하고, 다음 구현 slice에서
발행 권위·반환/효과 전이를 수정했다. **초기 표 또는 RED 집계를 현재 상태로 재사용하지 않는다.**
반례별 실제 도달 범위와 최신 전체 집계·명령은
[2026-09-06 감사 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.
C++/실기 승격과 CPU-only Rust 검증은 별개다. 외부 fixture 기능은 기본 집계에 포함되지 않는다.

### 받아들이지 않는 과거 결론

- “4스테이지보다 2스테이지가 빠르므로 GPU 수까지만 노드를 만든다”: 특정 배치 실험의 일반화다. 용량 요구를 무시한 제품 규칙으로 사용 금지.
- “RPC가 겹치므로 여러 GPU가 동시에 계산했다”: host service span과 device kernel span은 다르다.
- “빈 꼬리의 홉이 2 ms이므로 전송은 항상 싸다”: 특정 모델·한 호스트/링크 관측이다. 초대형 모델·다중 머신에 일반화 금지.
- “30행과 1,232행의 TTFT가 비슷하므로 admission이 원인이다”: 도착·대기·tokenize·prefill·첫 출력 시점을 분해하기 전 인과 확정 금지.
- “혼합 배치 0개이면 깊이 1이다”, “출력에 U+FFFD가 없으면 의미가 정상이다”: 둘 다 충분조건이 아니다.
- “단위 시험이 초록이면 현재 실기 품질도 초록이다”: 코드 안전성, 수치 회귀, 자연어 품질, 성능의 게이트는 별개다.

## 4. 책임 경계와 코드 지도

| 책임 | 현재 진입점/소유 | 목표 |
| --- | --- | --- |
| 요청·도착 웨이브·SLO·토폴로지·스냅샷 트리거 | `tools/event-drive/`, `test/benchmarks/p4-4node/` (OUTER) | 제품 요구와 실제 placement 명세 |
| 전달·순서·mailbox·노드 lifecycle | `layers/agent/`, `layers/adapters/adapter/`, `entrypoints/agent/src/event_runtime/mod.rs` | backend 중립 보존, 수용 통지·drain |
| runnable/배치 선택 | `layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs` | 순수·결정론적 정책; 독립 상태 전이와 결합 |
| 요청·비행·예약 원장 | `v2/node/state.rs`, `v2/node/worker/drive.rs`, `release.rs`, `settlement.rs` | 발행 기록과 원자적·멱등 정산의 단일 소유 |
| 분할·캡슐 | `v2/capsule.rs`, `v2/capsule/`, `v2/logical.rs` | 행 소유와 실제 physical membership 대조 |
| 완료 모델 | `v2/simulator.rs` | 가짜 시간/engine만 대체; 상태 변경 로직 복제 금지 |
| native 실행 | `staged/server/src/runtime/`, `staged/server/src/compat/`, `server/CMakeLists.txt` | public 타입/facade 경계, upstream 편의 기능은 opaque plan으로 보존 |
| llama.cpp와 구상 backend | `layers/adapters/llamacpp/upstream/`, `staged/compat/` | pin별 prepare·CPU·선언 production backend 감사 |

위 `v2/`의 기준 루트는 `layers/adapters/llamacpp/staged/adapter/src/v2/`다.
정책/원장 → P4 소유 capability → native compat → llama.cpp 추상층 → ggml/backend(CUDA·CPU·Metal 등)의 경계를 지킨다.
P4 코어에 모델별 batch·KV 규칙을 넣지 않는다. `common_params`를 필드 몇 개로 복제해 upstream 간접 옵션을 잃지 않는다.
신규 크레이트 분리는 공유가 실제 성립한 뒤 선택하는 포장 변경이지 B1의 목표가 아니다.
각 층의 변경 권한·public 타입·CMake/Rust 의존 허용 범위·pin 갱신 과정은 [격리 계약](layer-isolation-contract.md)을 따른다.
특히 adapter-owned 의미 DTO를 generic P4 protocol로 올리지 않는다. 격리 계약의 인터페이스 대장과
의존 manifest를 구현 산출물로 만들고, L1의 상태 확정과 L5의 native/출력 효과 실행을 분리한다.

## 5. 목표 상태 전이 — 구현해야 할 계약

현재 함수 시그니처가 아래 계약을 이미 구현했다고 해석하지 않는다.

### 발행과 정산

- `plan`: 상태 snapshot과 budget을 읽고 후보를 만든다. 후보 생성만으로 발행량·credit·KV를 소비하지 않는다.
- `reserve`: 참여 스테이지 KV/shape와 edge budget을 전부 확보한다. 부분 실패는 안전하게 해제/수렴한다.
- `issue`: 실제 받아들여진 발행을 fragment 기록에 결속한다. 전송 성공이 불확실하면 미발행으로 돌리지 말고 `Uncertain`으로 reconciliation한다.
- `validate_return`: generation/session/sequence/fragment/execution/phase/position range/row membership/outcome을 **발행 기록과** 대조한다.
- `commit_settlement`: 반환 이벤트가 건드리는 요청·비행 원장·예약·credit·후속 출력 의도를 함께 반영한다.
  기본 계약은 이벤트 전체 사전 검증 후 원자 반영이다. 부분 수용으로 바꾸려면 별도 receipt와 재시도 계약이 먼저다.
- `drain/release`: transport ACK, compute 완료, KV 정지점, sequence release attest를 별도 사건으로 취급한다.

fragment 기록에는 generation, session/sequence 세대, 논리 dispatch·fragment ID, physical execution 집합,
phase, `[start,end)`, membership digest, 예약/credit ticket, 상태와 정산 증거가 필요하다.
정확한 wire encoding은 구현 slice에서 버전화하고 기존 content-type 소유를 지킨다. raw counter를 식별자의 대용으로 쓰지 않는다.
같은 ID·같은 완료는 no-op/기존 receipt 재응답, 같은 ID·다른 내용은 conflict 거부한다.
미등록 ID, 이전 세대, 다른 시퀀스·구간·phase, 중복 행은 상태 변경 전에 거부한다.
발행되지 않은 위치로 진행하거나, 이전 반환이 다음 비행을 소비하거나, 정산 전 sequence를 재사용하면 실패다.

엔진/가짜 엔진은 모두 정규화된 결과를 반환한다. token 값을 얻는 과정만 다르고 generated count,
다음 입력 위치, stop/length, verify/replay continuation을 적용하는 의미론은 공유한다.
출력 publisher가 Full이면 완료 의도를 보존하며, 재시도는 같은 논리 결과를 두 번 발행하지 않는다.

### 배치 계획의 계약

- KV 상주 용량, resident sequence 한도, 이번 decode 폭, 논리 batch 한도, physical ubatch 한도,
  edge credit, 장치별 실행 슬롯은 서로 다른 축이다. 하나의 `parallel` 또는 bool로 합치지 않는다.
- 일반 attention은 합법적 decode와 chunked prefill을 row budget 안에서 배분한다.
- equal-width 계열은 cohort를 전체 eligible 수요에서 선택하고, cohort별 회전으로 **개별 요청** 진행을 보장한다.
- decode 다음 토큰은 선행 결과 없이 발행하지 않는다. prefill 다중 fragment는 KV 순서·행/byte credit을 증명한 뒤 허용한다.
- verify/replay와 speculative 창의 원자성은 throughput 때문에 완화하지 않는다.
- ready인데 미발행한 행은 `shape/credit/KV/cohort/in_flight/deadline` 등 이유로 분류한다.
  “남긴 행 0”은 목표 자체가 아니다. 의존성 때문에 보낼 수 없는 행을 runnable로 부풀리지 않는다.
- 공정성은 cohort 횟수뿐 아니라 요청별 첫 선택·중간 간격·마지막 대기 구간과 실시간 queue age로 확인한다.

## 6. 실행 단계와 승격 조건

상태 표기: `TODO`, `IN_PROGRESS`, `PASS`, `BLOCKED`. 필요한 증거가 없으면 PASS가 아니다.
각 단계는 [검증 규약](distributed-batching-verification.md)의 시험 ID와 연결한다.
B0의 반례 봉인은 **수정 전 실패를 확인·보존**하는 작업이다. 이를 통과 구현으로 보고하지 않는다.
해당 반례가 정상/부정 경로 및 mutation까지 통과하는 것은 B1의 종료 조건이다.
기존 suite가 통과한다는 이유로 반례를 생략하거나, 예상 실패를 숨겨 B0/B1을 동시에 PASS로 만들지 않는다.

| 단계 | 현재 | 산출물 / 종료 조건 |
| --- | --- | --- |
| B0 기준과 반례 봉인 | IN_PROGRESS | 기준 HEAD/실행 경로 확인, R-A/B/C 정식 failing tests, 문서·시험 inventory, target 후보/자원/권한 목록. T00~T04 |
| B1 발행 원장 + 원자적 정산 | IN_PROGRESS | 헤드 권위·후보 정산·효과 의도, 후속 incarnation/제어 receipt·승인 후 fairness commit 구현. 물리 실행 전 홉 멱등, 정상화 전이 전체 공유와 실패 수렴이 남음. T10~T19 |
| B2 실제 event worker 통합 | IN_PROGRESS | ordinary actual run 2/4/8-stage, speculative 2/4-stage·busy UNLOAD·head OUTPUT/발행 증거·소유 관측의 현재-run 소비 회귀와 별도 broker 포화 검증. 취소·drain·capacity notification·통합 순환망·재시작 freshness는 남음. T20~T28 |
| B3 수용·KV 예약·edge credit | TODO | bounded pending와 byte/token 예산, deadline·명시적 거절, 다중 노드 all-or-none 예약, row/byte credit, leak/over-admit 0. T30~T38 |
| B4 continuous batching 정책 | TODO | 전체 runnable 재선택, 일반/등폭/atomic 전략, 요청별 fairness, batch/ubatch 분리, 공유 전이를 쓰는 simulator/reference와 worker 대조. T40~T47 |
| B5 실행 신원·native·다중 호스트 하네스 | IN_PROGRESS | B1에 필요한 versioned 실행 identity와 제품 LOAD bind·native guard부터 보강. 실제 layout/model/build/ABI 결속·full/relink 격리·선언 backend·다중 호스트 runner는 남음. I00~I09/T50~T58 |
| B6 초대형 모델 웨이브 기준선 | TODO | §1의 자원 단계와 H0~H4: 정상 요청/응답 전문, 강한 겹치는 웨이브, 유효 TPS/GPU 요약, 재시작 없이 반복. 현재 fleet 성과와 2개 이상 물리 컴퓨터 최종 승격을 별도 기록 |
| B7 배치 최적화와 반증 | TODO | 토폴로지 고정 paired A/B + holdout, 단계별 cost 분해·credit-aware issue·prefill chunk/폭 선택; H5/H6. 승인된 유효 TPS/GPU Pareto 후보 |
| B8 지속 운영·최종 인수인계 | TODO | H7 soak/fault, 선언 backend/upstream 회귀, 재현 가능한 증거 bundle, 모든 필수 gate PASS와 남은 비필수 범위 공개 |

승격 의존 관계: B0 후 B1, B1 후 B2, B1/B2 후 B3, B2/B3 후 B4.
B1의 실제 소비 경로 검증을 위해 B2의 최소 fake-stage 연결을 먼저 작성할 수 있다. 이는 B2 승격이나
실기 최적화 선행의 허가가 아니며, 양쪽의 남은 시험을 생략하지 않는다.
B5의 환경 발견·신원 설계는 B1과 병행 가능하나 실기 승격은 B1~B5의 관련 gate를 모두 요구한다.
B6 후 B7, B7 후 B8이다. 작은 native smoke는 기존 감사 조합의 회귀 진단용으로 허용되지만 B6를 대체하지 않는다.
GPU 실험을 기다리며 같은 arm의 checkout을 수정하지 않는다.
계층 격리는 B5만의 마지막 청소가 아니다. B1~B4의 매 변경부터 I00/I03의 pure 경계를 지키고,
B5에서 I 전체를 완성하며, B8 및 이후 모든 채택 pin에서 반복한다.

### B1의 첫 작업을 구체적으로 고정

1. 열린 배치를 등록한 과다 반환, A 정상+B 잘못된 혼합 반환, 새 이벤트에 담긴 옛 execution,
   outcome 없는 wrong sequence/range를 `Worker::tail`/`handle`로 재현한다.
2. `close_execution`을 단순히 뒤로 옮기는 것으로 끝내지 않는다. 요청 전체 검증과 outcome 오류·출력 효과까지 사전 검증한다.
   꼬리 선출력을 제거하거나 승인 receipt로 차단하여, head가 거부한 반환의 출력이 먼저 나가지 않게 한다.
3. 발행 원장의 expected range/membership와 반환을 대조하는 validated settlement를 만들고 한 번만 적용한다.
4. Simulation의 실제 도착 경로에도 동일 malformed fragment를 주입한다. 직접 RequestState 함수 시험만으로 대체하지 않는다.
5. 새 원장과 기존 원장을 병행할 때 shadow mismatch는 숨기지 않는다. 두 곳이 독립적으로 상태를 갱신하는 전환기는 금지한다.

### B5 격리 구현의 종료 산출물

격리 계약의 경계별 대장을 코드 심볼/target과 연결한다. 기존 큰 `p4_llama_compat.cpp` 파일을 키우는 것 자체는 목표가 아니다.

1. 현재 dependency/include/link/type/codec 표면을 inventory로 봉인하고 I00~I02의 정상·침범 fixture를 먼저 만든다.
2. common 파싱·옵션·grammar 연산과 내부 단언을 compat 구현/전용 시험 타깃으로 옮긴다.
   문법을 재발명하거나 필드 getter 복제로 white-box 시험을 약화하지 않는다.
3. public/internal header 경로, `Impl` 접근, direct/transitive link를 정리한다. full build와 imported relink 모두 검증한다.
4. native shell와 engine bridge의 실제 허용 API·수명·실패 상태를 확정하고, capsule codec의 안정 코드표 또는
   opaque codec 협상을 구현한다. 기존 raw 정수 필드를 자동으로 중립 ABI라 부르지 않는다.
5. engine/common 변화와 ggml/backend 변화 각각에 합성 반증·실제 채택 pin 회귀를 연결한다.
   제품 LOAD identity 강제와 선언 backend conformance를 통과한 뒤만 실기 승격한다.

각 slice는 격리 계약의 최소 manifest 레코드와 검증 규약의 I 하위 사례를 함께 납품한다.
특히 Rust/native의 실행 권한 이중 구현 대조, 실제 적재 라이브러리·plugin 신원, opaque handle 수명,
model-free/model-required 시험 분리는 뒤의 성능 수치로 면제할 수 없다. 허용 module의 변경으로
흡수한 것과 상위 계약 변경이 필요한 것을 구분해 보고하며, getter나 새 폴더 개수를 성과로 세지 않는다.

이 단계는 B1의 안전성 수정과 별개로 병행 준비할 수 있다. B1~B4에서도 새 상태 권한 누출이나
native 의존을 추가하면 해당 slice를 승인하지 않는다. 자세한 책임/허용 의존은 격리 계약을 단독 소유로 유지한다.

### B7에서 비교할 정책과 금지할 접근

먼저 row 폭·prefill chunk·decode 예약 비중·age bound·edge issue budget을 작고 사전 선언한 후보군으로 비교한다.
호스트/장치별 queue, tokenize, compute, sample, encode/copy, network, tail wait를 나눈 cost model을 사용한다.
cohort를 합치려고 pipeline 전체가 비기를 기다리거나, sampler 안전성 없이 병렬화하거나,
로컬 실측 한 번으로 전송/노드 수를 원인으로 단정하지 않는다.
컷/placement를 변경하는 연구는 별도 arm이며 KV 수용량·모델·품질·연산량 차이를 함께 보고한다.
1GPU/replica/stock llama-server는 맞는 조건에서 진단 대조군이다. 분산 용량 목표의 대체 제품은 아니다.

### B4 정책 구현의 구체적인 출발점

고정한 upstream의 `tools/server/server-context.cpp::update_slots`, `can_batch_with`, prompt 추가/분할 경로를
직접 읽어 재사용할 의미론과 분산 때문에 달라지는 계약의 대응표를 먼저 작성한다. HTTP/slot/server_context
구현을 가져오거나 llama.cpp가 이미 분산 정산·credit을 보장한다고 가정하지 않는다.

1. 호출자 상태를 `resident/eligible/blocked`로 정규화한다. token 의존성, cache 명령 정지점,
   model/LoRA/shape 호환성은 단순 ready 개수와 별개다.
2. 합법적 shape와 최대 row는 **참여 stage 전체 capability의 교집합**에서 구한다. 헤드에서만 맞는 batch는 거부한다.
3. 일반 attention은 decode와 chunked prefill을 함께 검토한다. equal-width·verify/replay는 그 제약을 명시하는 별도 전략을 쓴다.
4. deterministic tie-break, 요청별 회전·age bound, row/byte/KV budget을 명시한다.
   계획 결과와 발행 확정은 분리하고 issue 거부/부분 성공/Uncertain 경로가 cursor와 fairness를 잘못 소비하지 않게 한다.
5. 작은 상태 공간의 독립 reference allocator와 전수 대조한다. oracle은 한 시점의 명시 목적/제약에 대한 기준이지
   장래 GPU 비용을 모르는 전역 최적 oracle이 아니다. 실제 비용의 후보 선택은 B7에서 한다.
6. capability/실측 cost profile을 입력 데이터로 버전화한다. 환경변수 임계값을 늘려 의미론의 빈칸을 메우지 않는다.

현재 실험 손잡이(min rows/open batches/issue rows/prefill fragments)는 검증된 새 정책에 연결되기 전까지
기본 비활성 또는 기존 단일 fragment 동작을 유지한다. 남길 손잡이는 적용 경로·예산·안전성·성능 반증을 모두 갖춰야 한다.

## 7. 기존 U/P 시리즈와의 연결 — 버리지 않되 순서는 교체

구체적인 저장 계약·결함 배경은 [구 계획](adapter-restructure-plan.md)에 남긴다. 오래된 완료/미착수 표현은 당시 상태다.

| 구 항목 | 새 소유/처리 |
| --- | --- |
| U0 | B5 + B8 per-pin 회귀. 모든 compat 작업이 B1의 pure correctness 작업을 막지는 않음 |
| P-1 | B0/B5 identity·재현성, 아래 K 저장 분기의 record identity |
| P0 | K 분기: namespace·receipt·bundle·CONTROL |
| P1a/P1b | B1/B2/B3: 실행 원장·동적 셀·슬롯 재사용 반례. 관찰만으로 수정 완료 금지 |
| P2 | K 분기 fault/복원 행렬. 사용하지 않는 영속 기능을 batch correctness 선행 조건으로 묶지 않음 |
| P2.5 | B7의 정적 batch/ubatch 보정. 저장 정체성 변화는 K 게이트 적용 |
| P3 | B3 수용/셀 예약과 K 스냅샷 정책 이행으로 분리 |
| P4 | B1/B4 기전-정책 분리. 크레이트 생성과 골든 일치만으로 완료 금지 |
| P4.5 | B3 credit; 정산 동일성과 원자성은 B1부터 필요 |
| P5 | B2/B4/B6/B7. 깊이 존재 여부가 아니라 안전하고 유효한 overlap/서비스 증명 |
| P6 | B5/B7: 실제 다중 머신 전송·고정비 프로파일 후 최적화. 병목을 미리 확정하지 않음 |
| P7 | B5/B8 backend/model 승격과 K의 청크 persist. 선언 외 조합은 계속 거부 |

### K: 영속·스냅샷/확장 분기

K0 namespace/CONTROL/불변 bundle → K1 Persist/Restore/Discard 장애 수렴 → K2 Checkpoint/Fork/RestoreInto/List,
LCP/TrimTo와 stage별 정지점 → K3 큰 상태 청크/교차 backend·ABI 행렬 순으로 다룬다.
세부 계약은 [저장 규약](kv-state-store-convention.md)이 소유한다. 기존 Committing·epoch·read-pin·quota 등 열린 결정을 버리지 않는다.
자동 TTL 축출·prefix 재사용·KV 복원을 켜면 관련 K 게이트가 필수다. B6 웨이브가 resident-only 예산에 들어가면
해당 기능을 꺼 둔 채 핵심 분산 배치 목표를 먼저 검증할 수 있다. 최종 보고는 꺼 둔 기능을 완료로 세지 않는다.
미감사 모델을 최종 대상으로 선택했다면 해당 memory/backend 감사는 선택 사항이 아니라 B5의 차단 게이트다.

### 기존 결함·열린 결정의 누락 방지 색인

아래는 **소유권 이관**이지 결함이 현재 재현되거나 해소됐다는 판정이 아니다.
기준 커밋 이후 다시 확인하고, 닫힌 과거 결함도 회귀 시험을 유지한다.
세부 배경/원문은 구 계획과 분야 규약, 실행 판정은 검증 규약의 T/K/H ID를 따른다.

| 구 ID | 새 소유 / 확인할 게이트 |
| --- | --- |
| D1 | B1~B4/B6: 실제 issue/settle/credit/overlap; T14/T20/T21/T38/T44, H2/H4. 과거 깊이 1 주장을 현재로 복사하지 않음 |
| D2/D3/D6/D13~D18 | K0~K2: namespace·identity·영수증·번들·세션 직렬화·장애 수렴; K00~K07 |
| D4 | B1/B2: slot/sequence 재사용 및 전달 손실을 분리 감사; T12/T18/T24/T25/T58. O13의 수리로 모든 position 결함을 닫지 않음 |
| D5/D12 | B3/B5: KV 예약/점유와 wire telemetry; T31/T32/T37/T57 |
| D7 | B5/B7: 실제 cut-set/copy/network 프로파일; H4~H6. 다중 호스트에서 비용 재측정 |
| D8/D9 | B3/B5/B7: compute/SWA 실제 메모리 회계와 batch/ubatch; T31/T37/T44/T53, H5 |
| D10/D11 | K2/K3: 큰 상태/프리픽스 재사용; K07/K09 |
| D19~D22 | B5/B8: pin·패치·include·실행 identity·EOL; T50~T53 |
| O1 | B3/B5: reserved/used/last-access의 telemetry 버전·시점·소유; T31/T57 |
| O2 | B5: stage ABI 실제 함수/타입·upstream 의미 변경 감사; T50/T52/T53 |
| O3 | B5/K1: 계열별 소형 모델/골든 state 자산의 실제 존재·재생; T53/K03/K04 |
| O4 | B3/K0: 예약/lease 전순서, TTL·Commit 경합·Prepared 회계; T32/K01/K09 |
| O5/O13 | 기존 해소 보고의 회귀 유지: session key 전달과 cancel-safe wire; T00/T24/T54/T58 |
| O6/O8 | K2: 정확한 snapshot 동사/ID 계약과 OUTER 원장 복구; K06 |
| O7/O11 | B3/K3: resident/disk/RAM capability·예산·ENOSPC; T31/K09 |
| O9 | B1/B2/K2: transport credit와 quiescence 분리; T25/T35/K05 |
| O10 | K2: read-pin/storage domain/Discard; K08 |
| O12 | B2/B6/B8: 동일 agent의 반복 수용과 연결 수명; T28, H2/H7 |

새 결함은 재현 경로·소유 단계·실행할 시험 ID를 함께 등록한다. “이미 등재됨”은 발견 이력의 분류일 뿐,
현재 차단 결함을 무시하거나 승격해도 된다는 뜻이 아니다.

## 8. 상태 기록과 중단 규칙

단계별 기록은 아래 형식을 이 파일 마지막에 누적한다. 증거 수치/응답 전문은 evidence 파일의 링크로만 참조한다.

```text
단계 / 상태 / 날짜:
검증한 source commit + dirty 여부:
계약 변경 및 소유 문서:
통과한 test IDs / 실행 명령 / exit code / 결과 파일:
수정 제거·오류 주입 시 실패한 ID:
실기 run IDs / binary+model+workload digests / machine identities:
미실행·제외·실패·BLOCKED와 이유:
다음 세션이 가장 먼저 실행할 반례/작업:
```

counterexample 미보존, malformed 반환 수용, credit/RSS 초과, 정상 응답 실패, 기록 손상,
arm 정체성 불일치는 해당 승격을 즉시 중단한다. 범위를 줄이거나 실패를 제외해서 통과시키지 않는다.
필수 시험이 없는 단계는 미완이며 “테스트 작성 예정”을 PASS로 기록하지 않는다.

### 2026-09-06 초기 인수인계 기록 — 아래 후속 기록 이전 상태

- B0 IN_PROGRESS: 코드 감사·문서 역할 재정리 및 R-A/B/C의 실패 반례 추가. actual handle 후속 지속,
  뒤 permutation/identity table의 아직 미도달 입력과 전체 worker/effect 경로는 추가 검증 필요.
- B1~B8 TODO. 위 표의 기존 부분 구현을 재사용하되, 해당 단계의 완료 시험을 생략하지 않는다.
- 계층 격리 보강: 권한/API/의존 대장과 upstream 변경별 실패 계약을 명시했으나 I gate 전체 구현은 아님.
  common 타입/Impl·간접 link·codec 격리와 R-D 출력 승인 문제가 남아 있다.
- K 분기는 목표 계약/기반 구현 상태이며 승격 전 재감사 필요.
- 다음 첫 행동: 보존한 T10~T13/T17 실패를 재실행하고, 실제 발행 expected membership 등록 및
  whole-event 검증→원장/효과 의도 commit을 연결한다. 부분 수리로 suite를 덮지 말고 handle·출력 경로까지 확장한다.

### 2026-09-06 후속 구현 — B1/B2 IN_PROGRESS

- 소스: HEAD `a9e1967fc` + 미커밋 어댑터/시험/문서 변경. commit/push/native 빌드/배포/GPU 실행 없음.
- `node/flight.rs::FlightLedger`: issued invocation/owners, 논리 fragment별 physical 집합,
  부분·역순 반환 buffering, bounded terminal receipts, 기존 ID 재사용 거부. **헤드의 원장**이지 전 홉 exactly-once가 아니다.
- `node/state.rs::RequestState::issue_fragment` 및 `settle_fragment`: worker와 Simulation의 실제 발행/도착에서 공유.
  Prepared/AwaitingNative/Uncertain을 구분하고 응답 유실을 미발행으로 되돌리지 않는다.
- `worker/release.rs::Worker::tail`와 `worker/outcome.rs`: 전체 후보 검증→request/flight/후속 intent commit.
  tail의 OUTER 선출력을 제거하고 head 승인 뒤 공개. pending KV ack와 물리 outstanding을 분리했다.
- `worker/effects.rs`: 승인된 output/forward/native 의도를 실행. Full은 기존 대기 경로로 보존,
  Closed/결과 불명은 남은 의도를 보존하고 fence. **메모리 내 보존**이며 재시작 내구 수렴을 구현한 것은 아니다.
- `process/core.rs::ServerControl`을 Box로 주입한다. fake는 native Frame만 만들고 selector/원장/정산을 흉내 내지 않는다.
  실제 handle/drive·native 호출 수·split·마지막 반환 장벽·사후 오류 fence를 검사한다.
  LOAD parser, `Worker::run` 수신 큐, 실제 N-stage broker/transport는 이 최소 seam의 범위 밖이다.
- 오류 반례·변이·최종 전체 집계는 [후속 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의 최신 절만 사용한다.
  원래의 R-A~D 반례가 보강됐다고 T10~T28 전체를 PASS로 만들지 않는다.

**다음 세션의 첫 행동과 남은 순서**:

1. 현재 소스/시험을 확인하고 **동일 load·session·request ID·slot 재사용 후 옛 RELEASE/RELEASED/SETTLE 도착** 반례를
   실제 fake 중간·꼬리 worker에 고정한다. `pending_releases[key]=slot`만으로 이전 요청과 새 요청을 구별하지 못한다.
   요청 incarnation과 연산/receipt 식별을 physical issue부터 모든 홉의 native 효과·ack까지 결속한다.
   head nonce만 추가하거나 동일 request ID 재사용 금지로 우회하지 않는다. adapter wire 버전과 native conformance가 필요하다.
2. 중간 노드의 동일 PHYSICAL 재전달이 native KV/sampler를 두 번 만지는 반례를 고정한다.
   edge 수신 원장의 accepted/running/completed/uncertain 및 재연결/이전 세대 처리를 구현한다.
   head의 중복 terminal no-op로 downstream 중복 계산이 해결됐다고 하지 않는다.
3. 실제 `Worker::run`을 N-stage 가짜 네트워크에 연결한다. 입력 큐가 계속 차도 발행 기회를 주는 bounded servicing,
   capacity wake, cancel/reload/drain을 검증한다. 지금의 `try_recv` 무제한 drain은 selector fairness와 별개다.
4. 공유를 작은 counter 함수 둘에서 끝내지 않는다. engine 결과 생성만 대체한 reference가 동일 발행/정산 의미론과
   outcome/stop/position 전이를 사용하도록 확장한다. 구상 native 타입은 순수 원장/정책에 넣지 않는다.
   `Scheduler::plan`이 먼저 바꾸는 cohort resume/decode_runs도 후보 fairness delta로 분리한다.
   실제 drive의 발행 거부→재계획에서 서비스 순번이 소비되지 않는 반례를 먼저 고정한다.
5. 고성능 기준선 전에 불변 요청 입력/라우팅과 작은 진행 후보를 분리한다. 현재 `RequestState::clone`은 긴 prompt와
   원본 Event를 매 발행·반환에 복사하며, effect clone은 cut-set bytes를 재복사할 수 있다.
   발행 등록 시 member index를 만들고 touched request/후속 fragment만 갱신한다. 전체 대조는 독립 시험으로 유지한다.
   CPU 시간·할당/복사 byte가 prompt 길이 또는 무관한 열린 배치에 비례해 증가하지 않는지 검사한다.
6. 그 뒤 B3/B4의 KV/row/byte credit·continuous batching 정책과 B5의 native/제품 신원 격리를 완결하고,
   B6~B8의 **초대형 모델·여러 물리 컴퓨터·강한 웨이브·정상 응답**을 실행한다. 로컬 green은 이 목표를 대신하지 않는다.

B0의 최종 모델/호스트/권한/예산 입력과 B5~B8 실기 증거는 여전히 미확정이다.
현재 반환 receipt는 제한된 메모리 window에서만 동일 결과를 no-op 처리하며, 만료된 ID는 fail-closed다.
active flight/queue/단계간 tensor 전체 메모리 상한은 B3의 별도 gate이고, receipt 상한으로 증명되지 않는다.

### 2026-09-06 후속 기록 2 — 실행 소유권과 정책 후보, B1/B2/B5 IN_PROGRESS

- 동일 load/session/request key/slot 재사용 반례를 actual head/middle/tail 소비 경로에 고정했다.
  늦은 제어/ack가 새 incarnation의 KV·슬롯·pending barrier를 소비하지 않도록 adapter와 native에 결속했다.
  wire·BindLoad·수명/범위의 단독 계약은 [배치 계약](adapter-batching-layers.md)의 실행 소유권 절이다.
- 중간/꼬리의 정확한 SETTLE/RELEASE 재전달은 native 추가 실행 없이 같은 receipt를 반환한다.
  같은 ID의 다른 body/kind, 이전 operation은 효과 없이 거부한다. 여러 control의 합계 예산도
  첫 native 효과 전에 검사하며, 뒤 receipt 축소를 앞의 여유로 미리 계산하지 않는다.
- `Scheduler::prepare_plan_with_physical_capacity`/`commit_plan`을 actual drive와 Simulation에 연결했다.
  후보 거부가 회전·cohort 순번을 바꾸지 않고 승인한 발행만 전진한다. 이 보강은 정책 최적화 결과가 아니다.
- native 소유권/UTF-8 helper는 llama/ggml 타입 없이 빌드한다. 제품 LOAD는 실행 identity를 bind하지만
  모든 build/model/state ABI/actual placement를 강제하는 완성된 B5 경계는 아니다.
- 실제 consumer·RED/GREEN·변이·동결 소스/전체 집계는 [최신 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 있다.
  이전 진행 기록의 미구현 표현은 그 시점의 이력이며 이 기록의 완료 범위에 한해 갱신된다.

**다음 세션의 첫 행동**: 동일 PHYSICAL을 새 event ID로 middle과 tail에 재전달하는 실패 반례부터
실행한다. 현재 제어 receipt와 head terminal receipt는 physical KV/sampler 재계산을 막는 원장이 아니다.
그 뒤의 순서는 다음과 같다.

1. PHYSICAL 수신/실행의 accepted/running/completed/uncertain 및 stage별 prefix 순서·이전 load/run 차단을
   구현한다. 중복 반환 no-op와 중복 native 실행 no-op를 구분하며 T18/T24를 완결한다.
2. actual `Worker::run` + N-stage 가짜 transport의 지속 입력·출력 포화·취소·재접속·종료를 검사한다.
   메서드 직접 호출 시험을 이 단계의 완료로 치환하지 않는다. 무제한 input drain의 starvation도 여기서 닫는다.
3. reference와 운영의 outcome/stop/position까지 동일 전이로 결속하고, 큰 불변 입력/작은 후보 delta를
   분리한다. full history 스캔/복사 비용을 줄이되 독립 전체 대조·거부 반례·변이를 유지한다.
4. 이후 B3/B4의 실제 KV·row/byte credit와 continuous batching 정책, B5의 나머지 격리·실행 신원·하네스를
   완결하고 B6~B8의 최종 다중 컴퓨터 실기로 간다. 영속 기능을 켤 때만 해당 K 분기 게이트도 선행한다.

실행 소유권 wire가 바뀌었으므로 이전 바이너리/배포를 그대로 사용할 수 없다. 모든 stage가 새 계약을
협상해야 하며 legacy mutation 우회나 mixed-version 자동 허용은 금지다. 실제 모델 선택·다중 물리 호스트·
접근 권한은 여전히 H0 미확정이고, 임의의 작은 모델/한 호스트를 최종 목표로 대체하지 않는다.
native CTest의 exit 0 안에 모델 부재 SKIP 분기가 있으므로 “13개 종료 성공”을 모델 conformance PASS로
읽지 않는다. GPU/실기 웨이브/성능 비회귀는 이번 slice의 완료 주장에 포함하지 않는다.

### 2026-09-07 문서 보강 — 계층 격리는 전 단계의 제약

- 사용자 요구에 따라 P4 공통, adapter L0~L5, native shell/engine bridge/common compat,
  llama.cpp 모델 실행 추상층과 ggml/backend를 구분하는 [격리 계약](layer-isolation-contract.md)을 보강했다.
  새 layer를 추가한 것이 아니라 의존·권한·의미의 세 검사를 명시하고 구현 manifest의 필수 필드를 정했다.
- 기존 코드/CMake의 common signature·Impl 우회·간접 링크·imported 바이너리 신원·handle 수명 경로를
  다시 읽었다. 발견/재확인한 잔여는 격리 계약과 I/T의 하위 시험 요구로 연결했다. 코드 수리나 gate PASS가 아니다.
- 동일 의미의 pin 변경은 허용 compat 모듈 안에서 흡수하고, 정규화 입력/trace와 위층 소스 변경 범위를
  독립 대조한다. 실제 의미·ABI·backend 제약 변화는 거부 또는 명시 계약 변경으로 처리한다.
- **구현 상태는 바로 위 후속 기록 2와 같다.** 이번 보강으로 B1/B2/B5를 완료로 바꾸지 않는다.
  다음 첫 행동은 동일 PHYSICAL의 새 event ID 재전달 반례이며, 이후 순서는 위 기록과 §6을 따른다.
  선언한 최종 모델·호스트·권한이 없는 상태에서 실기나 초대형 모델 성과를 주장하지 않는다.
  현재 load highwater는 같은 Worker 수명 안의 보호다. 같은 agent 내 Worker 재생성도 포함한
  새 load/run 신선성·재연결은 T18/T24 잔여이며, 프로세스만 살아 있으면 보존된다고 해석하지 않는다.
- 문서 검증: `npm run test:docs-lint` 12 passed / 0 failed, `npm run docs-lint` 추적 73파일,
  `node tools/scripts/docs-lint.mjs --all` 전체 79파일 clean, `git diff --check` 오류 없음.
  이번 문서 보강에서는 Rust 전체/C++/GPU 시험을 새로 실행하지 않았다. 이 수치는 I/T/H 구현 통과가 아니다.

### 2026-09-07 후속 구현 — PHYSICAL 재전달과 실기 자원 확정

상태는 **B1/B2/B5 IN_PROGRESS**다. HEAD는 `a9e1967fc`이고 미커밋 변경을 검증했다.
중간/꼬리의 실제 `Worker::handle`→PHYSICAL→native Frame 경로에 수신 원장을 연결했다.
정확한 입력 재전달은 보존 결과를 재생하고, cached+Fresh 혼합은 Fresh만 계산한다. 전체 사전 거부,
native 결과 불명의 fence, 다른 head의 번호 충돌 방지, 해제/슬롯 재사용 후 옛 결과의 비재계산을 검사했다.
정체성/보존 상한/만료의 단독 정의는 [배치 계약](adapter-batching-layers.md)의 PHYSICAL 수신 절을 따른다.
이 구현은 멱등 원장 기반이며 배치 정책의 TPS 개선이나 T24 전체 완료가 아니다.

opaque plan의 move 이후 참조 결함도 시험용 shared preparation에서 고쳤다. 실제 options E2E의
소유권 이전 순서와 모델 없는 lifetime 시험이 같은 helper를 통과하지만, **모델을 적재한 options E2E는
아직 실행하지 않았다**. native 빌드의 종료 성공과 생략된 모델 경로를 별도 집계한다.
최신 시험/RED/변이/소스 식별은 [증거 기록](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의 마지막 절에 있다.

사용자가 모델 경로·GPU 범위를 지정하고 RAM 오프로딩을 허용했다. 현재 자원과 확장 순서는 §1,
판정 조건은 검증 규약 H0가 소유한다. 목록/SSH 읽기만 수행했으며 새 binary 배포·GPU 모델 load·
실기 웨이브는 실행하지 않았다. 기존 하네스는 GPU 적재·한 ingress를 고정하므로 RAM/다중 호스트
manifest를 이미 실행하는 runner로 취급하지 않는다. B5에서 해당 경로와 부정 시험을 구현해야 한다.

**다음 첫 행동**은 새 execution ID를 붙인 동일 incarnation의 지난 위치·미래 gap·잘못된 phase를
actual middle/tail에 보내는 반례다. 정확한 old ID 재전달과 정상 연속 prefill/decode를 함께 검사한다.
이후 진행은 다음과 같다.

1. ordinary/equal/Verify/Replay와 SETTLE·RELEASE의 stage별 KV frontier를 명시적으로 결속한다.
   지금 소유권 검사는 active owner만 확인하며 이 순서를 증명하지 않는다. 새 Worker/native Session
   생성의 freshness와 credit/retry 수명도 별도로 보강한다. 기존 결과 cache만으로 완료하지 않는다.
2. actual `Worker::run`+N-stage transport에서 발행·부분 반환·지속 입력/출력 포화·취소·drain을 검증한다.
   메서드 시험 14개와 pure 원장 시험을 이 full-loop gate의 대체로 쓰지 않는다.
3. B1 후보의 큰 불변 입력/응답 복사와 `O(seen)` 전체 index 복제를 touched delta로 줄인다.
   독립 전체 원장 대조·원자 거부·변이를 유지한다. cache byte 상한을 active 메모리/RSS 상한으로 확대하지 않는다.
4. B3/B4의 예약·credit·배치 정책과 B5의 native/placement/runner를 완결한 뒤 §1의 자원 단계로 실기에 들어간다.
   부재 자원 때문에 실기 범위가 제한돼도 로컬 정합성 구현을 건너뛰거나 작은 모델로 최종 목표를 닫지 않는다.

### 2026-09-07 후속 구현 — 새 ID의 KV 위치 우회 방어

직전 기록의 첫 행동을 실제 middle/tail에서 실행했다. 정상 경로 1건은 통과했지만 새 ID의
지난 위치·gap·phase 회귀와 혼합 이벤트 사전 거부 8건은 native KV/sampler를 변경하며 실패했다.
이후 순수 stage frontier를 head 발행·중간/꼬리 PHYSICAL·SETTLE/RELEASE에 연결했다.
위치와 phase의 단독 계약은 [배치 계약](adapter-batching-layers.md)의 stage KV frontier 절을 따른다.
정상 Verify 전량 수용·부분 수용·checkpoint Replay도 실제 worker 메서드에서 검사했다.
순수 시험 10개, 실제 PHYSICAL 워커 시험 28개가 통과했고 독립 복사본 변이 6종을 검출했다.
전체 1012 passed / 0 failed / 7 ignored이며 자세한 소스 봉인·원문·변이 범위는 증거 기록의 마지막 절을 따른다.

**B1/B2/B5는 여전히 IN_PROGRESS**다. full Worker::run·native 직접 호출의 위치 자기검증·restart
신선성·credit·VRAM-only/RAM 오프로딩 실기는 완료하지 않았다. 현재 조회한 하드웨어 범위와
VRAM-only 이후 RAM 확장 순서는 §1을 유지하며, 이번 Rust 시험은 실기 성과의 대체가 아니다.

**다음 첫 행동은 새로 재현된 P1 proposal 폭 누락 두 경로를 닫는 것**이다.
`target/proposal-cap-red-20260907-01`의 독립 복사본은 `physical_capacity=2`에서 native의 정상 형식
proposal 3개를 PHYSICAL과 SETTLE 양쪽이 승인하는 반례를 보존한다. 토큰 예산 안이라는 것과
물리 atomic 폭 안이라는 것은 다르다. 이 반례를 원본의 실제 소비 시험에 이관하고, 응답 승인 전에
cap을 대조해 결과 불명 fence·후속 native 0을 확인한다. head의 `SETTLED`가 나중에 거부하는 것으로 닫지 않는다.

그 뒤에는 앞 기록의 full-loop·touched-cost·B3/B4/B5 항목으로 계속한다. 이미 구현된 receipt와
frontier를 다시 처음부터 만들지 않는다. 동결 소스 밖의 새 반례는 whole-suite GREEN에 포함됐다고
보고하지 말고, 해당 원문과 실패 수를 별도로 유지한다. 이번에는 원본 suite가 GREEN이어도 독립 P1 두 건은 RED다.

### 2026-09-07 후속 구현 — continuation 폭과 실제 루프의 첫 검증

직전 P1 두 경로와 정상/잘못된 Fresh 혼합을 원본 시험으로 이관했다. native continuation 폭을
PHYSICAL·SETTLE·head 반환 승인 전에 확인한다. 정상 폭 1/2의 후속 진행과 사후 실패 fence를 함께
검사했으며, 계약의 단독 정의는 배치 계약의 stage KV frontier 절이다.

실제 `Worker::run` 스레드 2/4/8개와 독립 native fake를 연결했다. ordinary 웨이브 합류·개별
token/position·전 stage release·완료 큐 Full 복구·max-open 부분 반환 장벽을 검사한다.
별도 actual EventNode/broker 시험은 outbound Full을 기다리느라 inbound를 못 읽는 두 노드
정지를 재현했다. 중립 코어의 pump는 방향별 이벤트 한 개를 보존하며 반대 방향을 계속 처리한다.
토큰/배치/모델 지식은 코어에 넣지 않았다. 시험 범위와 변이/소스/집계는 최신 증거 기록을 따른다.

**B1/B2/B5는 IN_PROGRESS**다. 위 run-loop fixture는 LOAD/subprocess·실제 모델과 network를
지나지 않으며 ordinary만 다룬다. 별도 broker 시험과 함께 통과해도 통합된 전체 순환망의 credit,
지속 입력 기아, speculative SETTLE/Replay, 취소·graceful drain을 완료한 것이 아니다.
원래 Worker::run의 무상한 입력 drain과 drive 루프는 아직 바꾸지 않았다.

동결 뒤 독립 copy에서 지속 유입 반례를 재현했다. runnable 요청 앞의 Tokenize 연쇄가 0/16/256이면
첫 Logical 이전의 Tokenize 수가 그대로 0/16/256이다. 연쇄 종료 뒤 정상 완주하는 것과 유한 처리
기회를 보장하는 것은 다르다. 이 관측 probe는 원본 whole-suite GREEN에 포함하지 않았다.

**다음 첫 행동:** 위 반례를 원본 실제 run-loop 회귀로 이관하고, 입력 처리와 발행을 유한 기회씩
교대하는 명시 계약을 구현한다. 입력과 발행 양쪽 방향의 기아 및 tail/control의 기회도 검사한다.
새 호출 예산을 제거하면 해당 회귀가 실패해야 하며, 조절 상수를 TPS 최적값으로 발표하지 않는다.
그 뒤 실제 run-loop의 speculative 제어·취소·종료, B1 touched-cost, B3/B4 credit/수용/정책,
B5 native/placement/runner를 진행한다. 하드웨어 자원과 VRAM-only → RAM 오프로딩 순서는 §1을 유지한다.

### 2026-09-07 후속 구현 — 유한 입력/발행 기회와 종료 분류

이 절은 앞의 “무상한 루프 미변경” 기록 이후의 구현 상태다. 실제 `Worker::run`은 한 turn에
입력을 최대 32개 처리한 뒤 head의 자발적 논리 배치를 최대 한 개 발행한다. 발행 성공이면 입력이
없어도 다음 turn에서 재선택하고, gate/무수요이면 입력을 기다린다. 숫자 32는 조절할 TPS 손잡이가
아니라 actor 처리 기회 상한이다. 하나의 PHYSICAL/control 이벤트 안의 작업량·동기 native 호출 시간은 별도다.

관측한 중지/입력 EOF 뒤 새 native 발행을 막는다. 종료 전 로컬 요청·정산·비행·KV/효과 잔량을
남기고, 정상 local empty와 abandoned/stopped/failure를 구분한다. 완료 history는 미완 작업으로
세지 않되 Stopped KV와 Uncertain은 남긴다. cleanup 전 성공 종료를 발표하지 않으며 cleanup 실패는
최상위 실패로 기록한다. 이는 **종료 증거의 보존이지 요청 취소 통보·네트워크 drain의 구현이 아니다.**

지속 Tokenize 연쇄, 입력 없는 추가 발행, 발행 중 정상 SESSION, 중지/EOF/cleanup 실패를 actual
run 회귀로 고정한다. 조회 함수와 실제 종료 소비 시험은 별도로 두고, 구현 제거의 독립 copy 변이·
최종 소스/실행 집계는 날짜별 증거에 남긴다. 기존 GPU 실측을 이 변경의 성능 증거로 재사용하지 않는다.

**B1/B2/B5는 계속 IN_PROGRESS**다. **다음 첫 행동**은 actual run fixture를 speculative
SETTLE/Replay의 정상 전량/부분 수용과 정확한 후속 위치·출력·전 stage release까지 확장하는 것이다.
기존 메서드 직접 호출 시험과 독립 token/KV 모델을 재사용하되 production 전이를 fake 안에 복제하지 않는다.
그 뒤 취소·정상 drain·재시작 없는 반복 실행과 capacity 통지를 검증한다. B1 touched-cost,
B3의 bounded admission/edge row·byte credit, B4의 여러 session을 포함한 공정성/정책,
B5의 native 직접 호출·실제 placement/runner도 남았다. 각 단계의 전체 종료 조건은 §6을 유지하며
이 slice의 성공을 전체 완료로 바꾸지 않는다. 현재 자원에서 VRAM-only의 정상 강한 웨이브를 먼저
증명한 뒤 RAM 오프로딩 모델로 확대한다. 다중 물리 컴퓨터 최종 증명은 별도다.

다음 slice의 시작 파일은 `worker/loop_tests.rs`다. node-target 라우팅 pump는 그대로 사용하고,
ordinary oracle를 약화시키지 않은 별도 speculative oracle를 추가한다. 현재 fake의 `compute`는
Prefill/Decode만 허용하고 `PhysicalSettle`을 구현하지 않았으며, 단순 chunk 분할과 위치당 1회
append 검사는 speculative에 그대로 쓸 수 없다. **전량 수용(SETTLE 없음) / 직접 부분 수용 /
checkpoint 복구 후 Replay** 세 literal 응답 스크립트를 먼저 고정한다. atomic 그룹 보존,
SETTLED를 보류한 동안 새 native 0, append→trim/restore→재append 기록, 정확한 출력 토큰·위치·
상한·종료, 전 stage RELEASE를 독립 대조한다. 특히 Replay의 output flag와 실제 생성 결과를
혼동하지 않으며 rollback 위치는 native 경로와 대조한다. 이것은 다음 시험의 설계 지침이고
현재 1039개에 그 speculative full-loop 시험이 포함됐다는 뜻이 아니다.

### 2026-09-07 후속 구현 — speculative full-loop와 native Replay 경계

앞 절의 첫 행동을 actual Worker::run에서 수행했다. 전량 수용·직접 부분 수용·checkpoint Replay
각각 2/4스테이지, 정산 체인 보류·별도 runnable 요청·정확한 출력·KV 변경 이력·전 stage release를
검사한다. 기존 ordinary 5개를 유지했고 3개를 더했으며 생산 소비 변이 3종이 실패한다.
시험의 단독 판정 조건은 검증 규약 T20/T24, 원문/소스/집계는 최신 정산 증거를 따른다.

fake 통과와 별개로 native 배치가 Replay의 logical output=false를 logits 요청에도 사용하면서
그 logits를 곧바로 읽는 오류를 찾았다. 요청 mask의 native 번역을 logical wire 의미와 분리하고,
실제 배치 생성 본문을 통과하는 모델 없는 소비 시험을 둔다. 이는 실제 llama 샘플링·checkpoint
복원·모델 수치/성능을 아직 증명하지 않는다. FIRST의 서버 capsule mask 복원도 별도 소비 범위다.
또한 no-llama 빌드의 무조건 compat include를 조건부로 고쳤으며 의존 include/link 권한을 넓히지 않았다.

**B1/B2/B5는 IN_PROGRESS**다. **다음 첫 행동은 실행 중 UNLOAD 반례를 닫는 것**이다.
독립 copy actual run은 tail 반환을 보류한 채 기존 UNLOAD를 보내면 native shutdown 1,
거부 0, UNLOADED 1, held tail 1, 출력 0, snapshot unloaded를 기록했다. 이 RED는 원본 전체
1042 GREEN과 별개다. 정지점 밖의 UNLOAD가 미완 요청·KV를 성공으로 지우지 않도록 아래
목표 계약을 실제 경로에 고정한다. 구체적 반례/미실행 양성 대조는 증거 기록에 남긴다.

1. UNLOAD를 정지한 load의 해제로 한정하는 안전 계약부터 구현한다. pending/요청/flight뿐 아니라
   pending SETTLE·RELEASE, Verify fence, middle의 active owner/frontier, Uncertain/효과 잔량도 본다.
   busy 거부는 native 호출·원장 삭제·UNLOADED 성공 효과가 없어야 한다. 이것을 즉시 강제 취소로 부르지 않는다.
2. 원본 ordinary run에 tail 보류 반례를 이관하고, 원래 요청 정상 완주 뒤 idle UNLOAD 성공을
   양성 대조로 실행한다. speculative SETTLED 보류/중간 stage active KV도 추가한다. requests/flight만
   검사하는 잘못된 guard와 무조건 UNLOAD 거부가 둘 다 실패해야 한다.
3. 그 뒤 명시적 Cancel/Drain 상태·권위를 설계한다. 현재 event 어휘에는 둘 다 없고 과거 Agent/deployment의
   Cancel은 다른 경로다. 신규 수용 중단과 기존 반환·SETTLE·RELEASE·출력 전달을 구분하고, 보낸 token을
   되돌리지 않는다. input EOF/Drop·로컬 잔량 0·native unload 성공을 global drain으로 승격하지 않는다.
4. actual EventNode/adapter/transport의 완료 ACK·capacity 통지·종료/join·반복 실행을 결속한다.
   같은 loaded Worker의 slot 재사용, 같은 Worker unload/reload, 새 Worker/agent 재시작의 freshness를
   별도로 검증한다. 현재 load highwater가 새 Worker에도 보존된다고 가정하지 않는다.

이후 B1 touched-cost, B3 bounded admission/edge row·byte credit, B4 다중 session 정책,
B5 native 직접 호출 권위·실제 placement·실기 runner를 진행한다. 미검증 MTP/native model 경로를
ordinary GREEN으로 열지 않는다. 현재 승인된 자원과 **VRAM-only 충분성 검증 → 더 큰 RAM 오프로딩
모델 검증** 순서는 §1과 H0를 유지한다. 단일 호스트 3090×2 성과와 다중 물리 컴퓨터 최종 목표를 구분한다.

### 2026-09-07 후속 구현 — 안전한 UNLOAD와 native 종료 실패 경계

앞 기록의 UNLOAD RED를 원본 actual Worker::run에 이관했다. ordinary tail 보류와 중간 KV,
speculative SETTLED 보류와 중간 Verify KV의 네 회귀가 기존 요청 완주·idle UNLOAD 성공까지
통과한다. 원래 ordinary/speculative oracle는 유지했다. 요청 수만 보는 guard와 무조건 거부도
실패하는 독립 변이를 남긴다. 단독 의미 계약은 배치 계약의 명시적 UNLOAD 절, 판정은 T25가 소유한다.

추가로 idle UNLOAD의 native cleanup 실패 뒤 새 SESSION이 ACK되는 반례를 actual run에서 찾았다.
native 실패 뒤에는 worker를 fence하고 원래 오류를 유지해 종료한다. 정상 busy 거부와 치명적 실패
정리는 구분하며 이 변경은 Cancel/Drain, 실제 OS process 정리 또는 출력 전달 ACK 구현이 아니다.
소스·원문·전체 집계와 변이는 최신 정산 증거에 기록한다. HEAD는 여전히 `a9e1967fc` + 미커밋 변경이다.

**B1/B2/B5는 IN_PROGRESS**다. 다음 소비 경계 감사에서 actual head가 승인한 OUTPUT을
`tools/event-drive/src/run/inference_identity.rs::InferenceIdentity::output`이 꼬리 source만 허용해
거부하는 코드 불일치를 확인했다. 워커 fake 통과를 현재 실기 drive 통과라고 하지 않는다.
독립 copy의 실제 producer 출력 15개(ordinary 5, checkpoint Replay 10)를 그대로 actual consumer에
넣어 모두 거부됨을 재현했다. source만 tail로 바꾼 인과 대조군은 전부 통과했다. 이 변경을 수리로
허용한 것은 아니며 원본 1047 GREEN 밖의 소비자 RED 두 건이다. `target/head-output-consumer-red-20260907-01/verification.md`
및 최신 정산 증거에 실제 캡처/명령/소스 봉인을 남겼다.
**다음 첫 행동은 이 교차 경계 반례를 원본에 이관하고 head 승인 출력 계약에 결속하는 것**이다.
tail/head 양쪽 무조건 허용으로 거부 검사를 완화하지 않는다. 실제 생산 출력 fixture와 strict identity의
load/session/request/route/position 부정 시험을 함께 유지한다. 이는 P4 중립 transport를 바꿀 일이 아니다.

그 뒤 앞 절의 Cancel/Drain 권위·출력 전달/ACK·capacity 통지·종료/join·재시작 없는 반복 실행을
이어간다. cancel 수용·발행 금지·기존 native 정산·전 stage release·OUTER terminal 전달은 서로
다른 증거다. 현재 v2 어휘에 없는 명령을 legacy Agent Cancel로 대체하지 않는다. B1 touched-cost,
B3 admission/edge credit, B4 정책, B5 native/placement/runner와 §1의 실기 순서는 그대로 남았다.

Cancel/Drain의 다음 구현에는 아래 코드상 제약부터 실제 소비 시험으로 닫는다. 현재 명령 구현 사실이 아니다.

- `worker/emit.rs::publish_or_wait`가 완료 Full에서 worker thread 자체를 기다리게 한다. Control class나
  수신 waker만 추가해서 뒤의 취소를 처리할 수 있다고 하지 않는다. bounded effect pump/공간 통지와
  control 처리 기회·용량을 함께 설계하고 T22/T23/T26에서 포화 중 도달을 증명한다.
- admission 전 request-attempt 권위와 admission 후 slot/incarnation·issued membership을 구분한다.
  durable session_key를 취소 ID로 쓰지 않으며 같은 request 이름 재사용 후 늦은 취소가 새 작업을
  건드리지 않아야 한다. 아직 비행 중인 요청을 원장에서 먼저 삭제하지 않는다.
- `event_runtime/transport.rs::deliver_outer`의 enqueue 또는 `write_loop`의 쓰기 성공을 OUTER 소비 ACK로
  쓰지 않는다. 현재 OUTPUT은 incarnation을, RELEASED는 개별 request 완료 watermark를 운반하지 않는다.
  취소 완료/Drain 영수증을 정할 때 이를 명시하며 P4 코어는 모델 의미 없는 전달 계약만 소유한다.

### 2026-09-07 후속 구현 — head 승인 OUTPUT과 실제 OUTER 소비 경계

앞 절의 출력 거부 반례를 원본 기본 시험으로 이관했다. actual Worker::run의 ordinary 및 checkpoint
Replay 출력 15개를 공용 wire fixture로 보존하고, 현재 producer의 의미 대조와 실제 InferenceIdentity/
inference::drive 소비를 각각 연결했다. consumer는 configured head 전체 endpoint만 허용하며 tail이나
모든 node를 같이 허용하지 않는다. 생산자·소비자 변이와 정확한 실행/제외 범위는 최신 정산 증거를 따른다.
승인된 출력 계약은 배치 계약, 판정은 검증 규약 T20 단독 소유다. P4 transport와 native ABI는 이 slice에서
바뀌지 않았다. HEAD는 여전히 `a9e1967fc` + 미커밋 변경이다.

**B1/B2/B5는 IN_PROGRESS**다. source를 고친 actual drive를 독립 copy에서 추가 감사하자 아래
세 거부 반례를 여전히 승인했다. 정상 대조 1 PASS / 거부 규약 3 RED이며, 원본 전체 GREEN과 별개다.

- 같은 요청의 fresh-ID RELEASED 두 개로 전체 해제 수를 채움. 실제 peer의 해제 집합에는 다른 요청이 없음.
- 제출 max_tokens=1인데 연속 위치 OUTPUT 두 개와 length 종료를 승인함.
- 서로 다른 프롬프트의 경계 [4,7]에서 두 번째 요청의 전체 출력 위치를 +1 이동해도 승인함.

**다음 첫 행동은 이 세 반례를 기본 실제 소비 경로 회귀로 이관하고 완료 판정의 증거를 닫는 것**이다.
시작 파일은 `tools/event-drive/src/run/inference.rs`, `inference_identity.rs`, `acceptance.rs`와
`worker/release.rs`다. 재사용할 독립 실행/원본 probe는 정산 증거에서 찾는다. T20의 완료 조건대로:

1. 상한·terminal과 요청별 프리필 경계의 양성/음성 소비 시험을 먼저 고정한다. 기존 response/judge를
   약화하지 않는다. BatchObservation의 도착 순서·유실과 실제 승인된 프리필 끝 위치의 관계를 확인하고,
   선택적 공통 `expected_prefill_rows`를 필수 per-request 증거로 오인하지 않는다.
2. 기존 scalar RELEASED는 요청 집합을 표현하지 못한다. 어댑터 소유 versioned 완료 영수증에 필요한
   membership/operation·incarnation 증거와 실제 producer/consumer 변경을 함께 설계한다. 현재 run 안의
   중복 방지와 재시작 후 freshness를 구분한다. 여러 요청을 한 통지로 해제하는 정상 경로를 유지하며,
   상위의 성공 판정을 단순 count 또는 correlation 중복 제거로 대체하지 않는다.
3. 각 반례가 원본에서 실패→수정 후 통과하고 독립 변이에서 다시 실패해야 한다. producer가 실제로 이
   손상을 만들었다는 주장과, 손상된 peer 응답을 consumer가 막는다는 증명을 구분한다.

그 다음 앞 절의 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker
포화·종료/join·반복 실행으로 이어간다. B1 touched-cost, B3 admission/edge credit, B4 정책,
B5 native/placement/runner는 그대로 남는다. source 승인만으로 GPU 실기나 정상 응답 전체를 승인하지
않으며, 현재 자원의 **VRAM-only 충분성 → RAM 오프로딩 확장**은 §1/H0를 그대로 적용한다.

### 2026-09-07 후속 구현 — OUTPUT 예산과 요청별 fresh-prefill 경계

앞 절의 세 소비자 RED 중 sampled 출력 상한과 요청별 첫 위치를 기본 실제 drive 시험으로 닫았다.
예산 검사는 OUTPUT을 적용하기 전에 수행하고, 요청별 관측 대조는 전체 terminal/해제 경계에서 수행한다.
유효한 OUTPUT 뒤에 관측이 도착하는 순서는 허용하되 그 최종 경계까지 관측이 없거나 어긋나면 거부한다.
빈 EOS도 sampled 예산에는 포함하며, 비어 있는 응답을 정상 품질로 승인하지 않는다. 자세한 의미는
배치 계약의 OUTER 예산 절, 시험은 T20, 실행 원문·독립 변이·봉인·집계는 최신 정산 증거가 소유한다.

이는 fresh position 0 제출의 head 관측과 OUTPUT 대조다. 실제 tokenizer/KV를 독립 관측한 증명이나
Restore/LCP의 위치 계약이 아니다. 공용 actual producer OUTPUT 15개는 그대로 유지했고, producer가
발행한 실제 관측의 prefill 합계를 독립 workload 상수와 비교했다. consumer fixture에 추가한 관측과
해제 통지는 synthetic이며 actual producer 캡처로 부르지 않는다. 전체 Rust **1076/0/7 ignored**,
하네스/build wiring **63/0**는 해당 소스 봉인에만 귀속된다. C++/GPU 실기는 이번 slice에서 실행하지 않았다.

**B1/B2/B5는 IN_PROGRESS**다. **다음 첫 행동은 해제 집합의 실제 생산·소비 계약을 닫는 것**이다.
scalar RELEASED로 A를 두 번 세어 A+B 해제로 승인하는 반례는 아직 미해결이다. 아래 경계들을 같이
다루며, 수신 영수증이 자기 기대 신원을 정하게 하는 순환 검증을 만들지 않는다.
후속 독립 actual run은 서로 다른 OUTER의 A/B가 한 physical capsule에서 정상 종료해도 해제 통지가
A에 2/B에 0으로 가는 RED를 확인했다. 별도 broker→실제 handle 시험에서는 tail이 아닌 middle/외부 Node의
ACK도 슬롯 반환과 대기 요청 재수용을 일으킨다. 두 결과는 원본 1076 GREEN에 포함되지 않은 copy 전용
반례이며 아직 수리하지 않았다. **해제 ACK의 독립 SESSION 권위를 먼저 고정하고**, 아래 외부 완료 계약을
함께 이관한다. 3-stage의 next는 middle이므로 next만 source로 허용하는 임시 수리도 금지한다.

1. 요청 시도·slot/incarnation·release operation의 기대 권위를 해제 통지보다 먼저 확정한다. 기존
   OUTPUT/RELEASED content-type은 이 증거를 충분히 운반하지 않으므로 versioned 어댑터 계약과
   producer/consumer를 함께 바꾼다. 현재 run 중 중복 방지와 새 OUTER/Worker 재시작 freshness는
   별도다. Sender의 sequence는 새 인스턴스에서 1부터 시작하므로 event_id만으로 재시작 신원을 증명하지 않는다.
2. 한 physical batch가 여러 OUTER 소유자를 담는 actual run 반례부터 고정한다. OUTPUT의 소유자별
   ReplySpec처럼 pending release에도 요청 소유 경로를 보존해야 한다. base batch 또는 돌아온 ACK의
   단일 return_route로 여러 소유자의 통지를 보내지 않는다. 내부 multi-request RELEASE batching은 유지한다.
3. 전 stage ACK 검증 뒤 slot 반환·pending admission과 외부 통지의 관계를 명확히 한다. 통지 실패로
   이미 정산된 KV를 다시 해제하지 않으며, 남은 notification intent가 effects에 보존돼야 한다.
   A 유효/B 무효의 원자 거부, 중복/누락/오래된 해제, 다중 OUTER 경로, 첫/후속 emit 실패를 실제 경로로 검사한다.
4. 같은 다중 OUTER actual run의 관측도 소비자까지 연결한다. 현재 전체 batch requests를 모든 ReplySpec에
   보내는 생산과 자기 제출만 허용하는 소비가 불일치한다. 해제 라우팅만 수리한 뒤 다중 OUTER 전체를
   완료로 부르지 않는다. 배치 계약의 소유자별 관측/전체 통계 구분과 T20의 양·음성 시험을 함께 적용한다.
   이미 실제 producer 관측을 사후 수정 없이 실제 InferenceIdentity에 재생하여 A/B 양쪽의 unknown-request
   거부를 확인했다. copy 전용 **1 PASS/1 RED**이며 전체 drive나 원본 기본 시험 완료로 세지 않는다.

이후 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker 포화·종료/join·
재기동 없는 반복 실행을 잇는다. B1 touched-cost, B3 admission/edge credit, B4 정책과 B5 native 권위/
placement/runner도 남아 있다. 모델/GPU 성과는 여전히 §1/H0의 **VRAM-only 충분성 → RAM 오프로딩 확장**
순서에서만 승인한다. 두 GPU의 단일 호스트 검증을 다중 물리 컴퓨터 완료로 바꾸지 않는다.

### 2026-09-07 후속 구현 — SESSION 권위와 해제 ACK 발신자 경계

앞 절의 ACK source 반례를 원본 실제 broker/worker 회귀로 옮기고 SESSION의 독립 토폴로지 선언으로
수리했다. 구 wire를 암묵 변환하지 않으며 OUTER 실제 생산 경로도 함께 이관했다. wire/역할 의미의
단독 정의는 배치 계약의 해제 권위 절, 시험 제약은 T20/T25, 실행·변이·봉인은 정산 증거가 소유한다.
이번 수정은 concrete adapter와 OUTER 안에 있고 P4 중립 broker/native/llama/backend에 지식을 추가하지 않았다.

actual 3-stage 반례는 중간 노드의 ACK로 9번째 대기 요청이 조기 발행되는 것을 구코드에서 확인했다.
수정 후에는 native 발행 상태를 보존하고, 정상 terminal ACK 재개 뒤 기존 출력/위치/KV oracle로
완주한다. 기존 ordinary 2/4/8 및 speculative 2/4 경로를 유지한다. 이는 post-LOAD fake stage의
실제 worker 루프이며 실제 모델/품질/GPU/TPS 증명이 아니다. 코드 기준은 여전히 같은 HEAD의 미커밋 트리다.

code-only 추가 감사에서 transport peer/source 인증·최초 설정자의 control 권한과 SESSION_READY의
전체 topology attest는 별도 미구현임을 확인했다. 계층 귀속은 격리 계약의 신뢰 경계를 따른다.
B5 제품/fleet 신원 게이트에서 신뢰망 제한과 실제 인증/합의 증명 여부를 분리하고, 필드 비교를
발신자 인증 또는 모든 노드의 동일 선언 승인으로 보고하지 않는다. 이 감사는 네트워크 침입 재현이 아니다.

**B1/B2/B5는 IN_PROGRESS**다. ACK 역할 검사는 닫았지만 전체 해제 완료는 닫지 않았다.
**다음 첫 행동은 pending release의 요청 소유 provenance와 versioned OUTER 완료 계약을 연결하는 것**이다.
시작 파일은 `worker/release.rs`, `worker/effects.rs`, `node/state.rs`, `commands.rs`, 그리고
`tools/event-drive/src/run/inference.rs`/`inference_identity.rs`다. 이미 봉인한 다중 OUTER actual run과
scalar 중복 승인 RED를 재사용한다. 소유 route 수정만으로 멤버십 증거·재시작 freshness를 완료로 부르지 않는다.

1. resident 요청 삭제 전에 작은 요청 시도/ReplySpec/slot·incarnation·operation 증거를 pending에 보존한다.
   제출·terminal 승인·해제 통지의 계약과 생산/소비를 함께 이관하며 수신 receipt가 기대값을 만들게 하지 않는다.
2. 전체 ACK 후보 검증 후 상태와 소유자별 notification intent를 함께 commit한다. 실제 Full/Closed/
   event-ID 고갈 반례에서 남은 통지를 보존하고 native release를 반복하지 않아야 한다.
3. A/B 실제 동시 batch의 소유자별 OUTPUT/해제/관측을 실제 OUTER 소비까지 연결한다. 현재 관측의
   foreign-request 거부와 B span 누락도 미해결이다. 전체 통계를 요청 소유 행으로 바꿔 통과시키지 않는다.

그 뒤 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker 포화·종료/join·
반복 실행, B1 touched-cost/B3 admission·credit/B4 정책/B5 native·placement·runner를 잇는다.
실기 자원 순서는 §1/H0 그대로이며 로컬 정합성 GREEN으로 VRAM-only/RAM 오프로딩 웨이브를 대체하지 않는다.

### 2026-09-07 후속 구현 — 요청별 해제 증명과 소유자 통지

정상 sampled 종료에 대해 이전 절의 1·2번을 구현하고 실제 생산/소비를 함께 이관했다. 소유자는 배치
계약의 해제 절이며, 새 wire의 필드·순서·제외 범위를 다른 문서에서 재정의하지 않는다. P4 중립 envelope/
broker와 native/llama/backend는 이번 변경 대상이 아니다. 여전히 HEAD a9e1967fc의 미커밋 작업 트리다.

실제 요청 제출과 terminal 승인으로 기대값을 먼저 고정하여 A의 중복 영수증이 B의 완료를 대신하지
못하게 했다. 실제 2/4-stage 혼합 terminal에서 각 OUTER의 route/correlation/deadline을 보존하고,
첫/후속 Full·Closed·ID 고갈 시 미발행 알림을 보존한다. 실제 원본 PREFILL·OUTPUT·영수증의 새 캡처는
구버전 토큰/text/position/stop 검사를 그대로 유지한다. 이는 post-LOAD fake native와 in-memory
EventWire 증명이지 모델 의미/토큰화·실제 네트워크·GPU 성능 증명이 아니다.

소스379 봉인과 전체 Rust **1122/0/7 ignored**, 확장 JS **75/0**, 독립 생산/소비 변이 및 실행별
범위·처음 실패한 시험은 [정산 증거의 요청별 해제 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.
기존 JS63은 선택된 하네스57+빌드 wiring6의 결과였다. 범위를 넓히면서 오래된 저장소 경로를 참조하던
설정 시험을 발견해 import만 실제 모듈로 고쳤다. 누락 시험을 runtime skip으로 녹색 처리하지 않았다.

**B1/B2/B5는 IN_PROGRESS**다. 현재 run의 정상 종료가 새 OUTER/Worker 재시작 freshness, 출력 없는
Cancel/실패, durable outbox·재연결 수렴 또는 전체 다중 OUTER 성공을 의미하지 않는다. 같은 request_id의
동시 복수 OUTER도 아직 지원하지 않는다. 실패 뒤 fence된 알림은 자동 재발행/회복했다고 보고하지 않는다.

**다음 첫 행동은 같은 실제 다중 OUTER batch의 관측 생산·소비를 맞추는 것**이다. 시작 지점은
`worker/observe.rs`, `commands.rs::BatchObservation`/`StageSpan`, OUTER의 `inference_identity.rs`,
`inference_evidence.rs`, report 집계다. 앞서 봉인한 unknown-request RED와 현재 소유자별 actual run을 재사용한다.

1. 원래 물리 폭·execution 전체 비용과 각 OUTER의 허가된 요청 행을 분리하는 versioned 계약을 고정한다.
   전체 requests를 route마다 복사하거나 unknown-request 거부를 꺼서 통과시키지 않는다. span 수신 대상을
   명시하고 동일 계산을 요청 수만큼 중복 계수하지 않는다. request_id만 있는 관측의 attempt 범위도 감사한다.
2. 실제로 생산된 OUTPUT/영수증/관측을 각 실제 OUTER 소비자에 연결한다. 타 요청 노출·B 관측 누락·
   모든 관측 삭제·전체 통계와 소유 행 혼동의 부정 및 같은 OUTER 여러 요청 양성을 함께 둔다.
   synthetic 관측으로 actual producer의 잘못된 라우팅을 보정해 통과시킨 실행은 이 단계 증명이 아니다.
3. 이후 Cancel/Drain·bounded effect pump·capacity notification·actual EventNode/broker 포화·종료/join·
   재기동 없는 반복 실행을 잇는다. B1 touched-cost, B3 admission/edge credit, B4 정책, B5 native 권위/
   actual placement/runner가 남는다. 과거 U/P 전체를 다시 직렬 선행 조건으로 만들지 않는다.

§1/H0의 **VRAM-only 충분성 → 더 큰 RAM 오프로딩 모델** 순서는 변하지 않는다. 현재 3090×2 한 물리
호스트의 자원 증명과 최종 다중 물리 컴퓨터 증명을 분리하며, 로컬 녹색 수치를 최종 목표 완료로 올리지 않는다.

### 2026-09-07 후속 구현 — 관측 완결 감사와 보고 지표 분리

앞 절의 관측 경로를 실제 코드에서 추가 감사했다. 현재 생산은 전체 요청 관측을 각 route에 복사하고
span은 첫 owner에만 보내며, 소비는 terminal/receipt 후 즉시 종료한다. 받은 execution에만 coverage를
요구하면 관측과 span을 한 묶음 통째로 잃은 경우까지는 발견하지 못한다. 이를 위한 목표 계약은
[배치 계약의 관측 완결 절](adapter-batching-layers.md), 반례는 검증 규약 T20/T25/T57/T58에 고정했다.
이는 **읽기 전용 코드 감사와 계약 보강**이며 새로운 producer/consumer wire 구현이나 GREEN이 아니다.

병행 가능한 보고서 수식 오류를 수정했다. 실제 `run.mjs::buildReport` 소비 경로에 지표 버전을 넣고
계산 행과 승인 출력 토큰을 분리했다. 상세 필드/이전 수치 이관/분모는
[하네스 README](../test/benchmarks/p4-4node/README.md)의 현 구현을 따른다. 새 report 시험11과 독립
변이를 포함한 실행 원문은 정산 증거에 보존한다. 기존 Rust379 봉인은 그대로이고 이번 Rust 전체 재실행은
**1122/0/7 ignored**, 확장 JS는 **86/0**다. 물리 span 집계·Rust 요청별 행 속도·H4 최종 유효 TPS는
이번 수식 수정으로 완료되지 않았다. 실제 모델/VRAM-only/RAM 오프로딩/다중 컴퓨터 실기는 실행하지 않았다.

사용자의 자원 확장 순서는 §1 그대로다. H0는 의도한 CPU 계산이어도 실제 모델 계산/가중치/KV가 host에
의존하면 VRAM-only로 승인하지 않도록 분류를 명확히 했다. 일반 제어/토크나이즈/CPU sampler/staging과
계산하지 않는 비소유 레이어는 구분한다. 기존 runner의 GPU 고정 plan은 RAM offload 지원 증거가 아니다.

**B1/B2/B5는 IN_PROGRESS, 다음 첫 행동은 여전히 실제 다중 OUTER 관측 생산·소비의 결속**이다.
이 보고서 수정은 그 선행 계약을 건너뛰어 GPU 임계값을 다시 튜닝할 허가가 아니다.

1. 배치 계약의 canonical 발행 증거 입력을 독립 literal vector로 먼저 고정하고 `accept_prepared_issue`
   후보 검증/commit과 정상 terminal의 새 OUTPUT 버전에 연결한다. 원장과 요청의 부분 commit·과거
   이력 clone을 금지하고, raw count만으로 동일량의 다른 발행을 승인하지 않는다.
2. 그 계약으로 소유자별 head 관측·fresh stage span과 effect fan-out을 연결하고 actual drive의
   Missing/Invalid/Complete 판정을 구현한다. 기존 실제 A/B 캡처와 구버전 출력 oracle를 보존한다.
   뒤늦은 관측, 묶음 전체 누락, 다른 attempt, 물리 전체/소유 부분 혼동을 실제 경로에서 검사한다.
3. 다음으로 Cancel/Drain·bounded effect pump·capacity notification·EventNode/broker 통합 포화·반복
   실행을 잇는다. B1 touched-cost/B3 admission·credit/B4 정책/B5 placement·runner도 남는다.

현재 Rust/JS 녹색은 정해진 로컬 코드 범위만 증명한다. 모든 신규 목표 시험이 실행됐다는 주장이나
VRAM-only 충분성, 이후 더 큰 RAM 오프로딩 모델의 성공으로 승격하지 않는다.

### 2026-09-07 후속 구현 — 내부 발행 증거와 제출 입구

이전 첫 행동의 **내부 발행 증거만** 실제 L1 승인 경로에 연결했다. canonical 입력·의존 위치·원자성은
[배치 계약의 내부 issued-work v1 절](adapter-batching-layers.md)을 따른다. primitive와 실제 L1 API,
2/4/8-stage actual Worker::run에서 독립 bytes/digest 및 기존 출력/KV/해제 oracle를 대조한다.
승인 기록 누락·조기 commit·execution 입력 누락을 독립 복사본의 재컴파일 변이로 검출했다.

추가 코드 감사에서 P4 envelope와 하위 승인 신원의 NUL 허용 범위가 다름을 발견했다. 실제 worker에서
6개 입력이 계산/KV 변경 뒤 Uncertain·종료로 이어지는 RED를 남겼다. 이를 PREFILL 입구에서 같은 신원
검사로 거부하도록 고쳤다. 정상 Unicode·별도 correlation과 거부 뒤 같은 worker의 정상 재제출을 유지한다.
이 수리는 backend별 코드나 P4 공통 문자열 규칙을 바꾸지 않는다. 범위와 최종 소스/집계/변이 원문은
[정산 증거의 내부 witness 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.

**B1/B2/B5는 IN_PROGRESS**다. OUTPUT v4/관측 wire/actual OUTER 완료 조건은 아직 이관하지 않았다.
현재 witness는 내부 RequestState에만 있고 정상 terminal에서 제거되므로, 이를 외부에서 확인 가능한
증거 또는 전체 관측 누락 검출로 보고하지 않는다. 전체 행 정보·소유 투영·late observation·effect
fan-out의 기존 공백도 그대로다. 수신 관측이 자기 기대 집합을 만들어 승인하는 우회를 금지한다.

**다음 첫 행동은 제출 입구의 남은 row-string 경계를 실제로 고정하는 것**이다. 코드 감사에서
serialized ReplySpec이 logical/capsule의 4096바이트 한도를 넘는 입력은 아직 토큰화/기록 뒤에야
거부될 수 있음을 확인했다. 이는 code-only 열린 표면이며 이번 NUL RED로 실행 재현했다고 하지 않는다.

1. `worker.rs::prefill`과 logical/capsule의 기존 문자열 상한을 비교한다. 원본 JSON 길이와 escape 후
   ReplySpec 바이트 길이를 구분하여 실제 prompt/tokens 입력으로 반례를 먼저 만든다. 정상 경계값·
   Unicode·독립 correlation 양성을 유지하고 토큰화/요청·세션키·slot·witness/다른 요청 효과 전 거부한다.
   기존 wire 상한을 늘리거나 늦은 worker 종료를 정상 요청 거부로 바꿔 보고하지 않는다.
2. 이어서 이미 고정한 witness를 **정상 terminal의 새 OUTPUT 버전**에 복사하고 producer/consumer를
   함께 이관한다. 기존 v4 캡처/출력 oracle는 유지한다. 발행 primitive/실제 L1/worker 변이와 별도로
   실제 OUTER가 최종 count/digest를 대조하는 반례를 추가한다.
3. 소유자별 head 관측·fresh stage span·effect fan-out 및 actual drive의 Missing/Invalid/Complete를
   연결한다. 마지막 관측 지연·중간 관측 묶음 전체 누락·동일량의 다른 execution/위치·타 소유자 누출·
   span 중복/전체와 부분 통계 혼동을 정상 다중 OUTER batch와 함께 검사한다.
4. 그 뒤 Cancel/Drain·bounded effect pump·capacity notification·EventNode/broker 통합 포화·반복 실행,
   B1 touched-cost/B3 admission·edge credit/B4 정책/B5 native·실제 placement·runner를 잇는다.
   SHA 재계산/현재 행 정렬과 기존 프롬프트 clone 비용이 공짜라고 가정하지 않는다.

정책 손잡이를 늘리거나 GPU 사용률 수치로 위 정확성 경계를 건너뛰지 않는다. 현재 자원과 실기 순서는
§1/H0의 **VRAM-only 충분성 → 더 큰 RAM 오프로딩 모델**을 유지한다. 이번에는 모델 적재·실기 웨이브·
원격 배포를 하지 않았으며, 한 호스트의 두 3090과 최종 다중 컴퓨터 증명을 구분한다.

### 2026-09-07 후속 구현 — 제출 문자열 경계와 관측 이관 준비

이전 첫 행동의 serialized ReplySpec/options 크기 경계를 실제 Worker::run에서 RED로 고정했다.
정상 한도 입력은 유지하고 초과 입력은 세션키 기록·Tokenize·요청 수용 전에 거부하도록 공유 wire 검사를
배치했다. 정확한 한도/소유는 [배치 계약](adapter-batching-layers.md), 반례/변이는 검증 규약을 따른다.
Rust와 모델 없는 C++의 기존 codec에도 독립 경계 회귀를 추가했다. 생산 C++/한도/CMake는 바꾸지 않았다.
실행 소스·RED/GREEN·변이·전체 집계는
[정산 증거의 제출 문자열 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.
기존 Tokenize 실패·context 초과 등 다른 수용 실패의 원자성을 이번 검사로 완료 처리하지 않는다.

**B1/B2/B5는 IN_PROGRESS**다. 내부 witness는 아직 OUTPUT에 없고 전체 관측 누락도 판정하지 못한다.
**다음 첫 행동은 새 OUTPUT와 소유자별 관측 DTO의 실제 생산·소비 이관**이다. 내부 hash만 공개하고
현재 소비자의 terminal/receipt 즉시 종료를 남기는 것으로 완료하지 않는다.

1. 배치 계약의 canonical witness를 재사용해 OUTPUT v5/BATCH_OBSERVATION v4/STAGE_SPAN v4의
   adapter-owned 타입·엄격 검증을 함께 만든다. terminal만 최종 증거를 보유하며 기존 wire를 조용히
   확장하지 않는다. request_issue_index를 쓰면 수신자가 만든 기대 총량이 아니라 head 승인 당시 count로
   결속한다. request가 없는 logical ordinal 간격·역순·정확 재전달을 증분 처리하는 시험이 먼저다.
2. `release.rs`의 RequestState 제거 전 승인 증거를 보존하고, head 관측은 승인된 split+원제출로 만든다.
   full OuterEndpoint별 소유 행과 물리 전체 통계를 분리하고 fresh span만 보존한다. 모든 recipient를
   사전 검사하며 forward 이후 시각은 한 번만 고정한다. 재시도마다 시각을 바꾸거나 native를 재실행하지 않는다.
3. actual drive는 자기 송신 권위·terminal witness·해제 집합·해당 execution의 전 stage coverage를 대조한다.
   Missing은 기존 overall deadline 안에서 기다리고 Invalid는 실패한다. 조기 완료·전체 관측 삭제·다른
   execution/중간 position 교체·다중 OUTER 누출·후순위 Full/Closed/ID 고갈을 실제 producer/consumer로 검사한다.
   기존 원문 캡처/토큰/KV oracle는 보존하며 새 버전은 실제 run에서 새로 캡처한다.
4. 관측을 기다리는 시간이 늘어도 기존 release-boundary 지표의 분모를 몰래 바꾸지 않는다. release 시각을
   별도 latch하고 관측 완결 시각을 분리하거나 summary 버전을 명시 이관한다. owner-visible 작업을 fleet
   전체 비용으로 합산하지 않는다. 정책·원장에 native/backend 타입을 추가하지 않는다.

이후 Cancel/Drain·bounded effect pump·capacity notification·EventNode/broker 통합, touched-cost·
admission/credit·배치 정책·native placement/runner가 남는다. 이번에는 모델·GPU/원격 웨이브를 실행하지
않았다. 현재 자원의 VRAM-only 충분성 이후 RAM 오프로딩 확장 및 최종 다중 물리 컴퓨터 증명은 §1/H0를 유지한다.

### 2026-09-07 후속 구현 — 발행 증거의 OUTPUT·관측 완결 이관

이전 첫 행동의 내부 발행 증거를 실제 생산·소비 경계로 이관했다. OUTPUT v5는 정상 terminal에서만
실제 승인된 witness를 보존하고, head OBS v4와 모든 stage SPAN v4는 full OUTER별 소유 내역을
물리 전체 통계와 분리한다. 버전·필드·정확한 의미의 단독 소유는 [배치 계약](adapter-batching-layers.md)이다.
중립 P4 envelope/core·native stage wire·llama/backend에는 이번 의미 타입이나 의존성을 추가하지 않았다.

actual drive는 실제 송신 권위와 terminal 증거에서 기대량을 얻고 관측의 issue chain/해제/선언 stage
coverage가 끝나기 전에는 성공하지 않는다. 출력·해제 뒤 늦은 관측은 허용하지만 원래 deadline은 늘리지
않는다. release 완료 시각과 관측 완결 시각을 분리해 기존 처리량 분모를 보존했다. 기존 v3/v4 원문과
token/text/position/stop·KV·해제 검사는 유지하고, 새 wire는 actual worker에서 따로 캡처했다.

검수 중 정상 span에 근거 없는 empty-owner 실행을 더해도 성공하던 반례를 실제 drive에서 재현했다.
이제 **수신된** global 실행은 소유 내역이 비어도 head 물리 크기로 확인될 때까지 Missing이다. A에게
아예 도착하지 않은 B-only stage span을 요구하는 규칙이 아니다. 정상적인 span-before-head도 유지한다.
정확한 실행 소스·수정 전 실패·독립 복사본 변이·전체 집계와 한계는
[정산 증거의 OUTPUT·관측 이관 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.

**B1/B2/B5는 IN_PROGRESS**다. 이번 검사는 현재 run의 소유·관측 완결이지 실제 llama tokenizer/품질,
GPU wavefront, 교차 호스트 시계, 전체 통계의 독립 장치 계측 또는 완전한 분산 drain의 증명이 아니다.
고정 native 응답을 쓴 actual worker와 캡처를 재생한 actual drive를 구분한다. offline acceptance는
온라인 원장 검사를 독립 재실행하지 않는다. target의 로컬 봉인은 영속 배포 bundle도 아니다.

**다음 첫 행동은 completion Full 중 실제 정상 정산 ACK가 멈추는 반례를 고정하는 것**이다.
`worker/effects.rs::flush_effects` → `worker/emit.rs::publish_or_wait`가 worker 스레드에서 대기하므로
기존 input/issue quantum도 제어 이벤트를 읽을 수 없다. 단순히 sleep을 waker로 교체하는 것만으로는
같은 actor의 제어 소비 기회가 생기지 않는다. 아래 순서로 진행한다.

1. 실제 worker에 A의 RELEASED를 보류하고 B의 완료로 큐를 포화시킨 뒤 A의 정확한 ACK를 보낸다.
   시험용 읽기 관측으로 실제 입력 수용·포화·미처리를 구분한다. 원인 재현 뒤 출력 공간을 다시 열어
   원래 OUTPUT/receipt/관측의 바이트·순서·한 번 전달과 전 stage 정산을 유지하는 양성도 실행한다.
   source 변화 없는 시간 경과만으로 포화/기아를 선언하지 않는다.
2. immutable committed intent를 보존하는 bounded effect pump와 capacity 통지를 설계한다. Full은
   미발행 작업 유지, Closed/ID 고갈은 명시 실패이고 native 재실행이 아니다. 효과 의존 순서와 정산
   authority를 지키면서 제어 소비 기회를 부여한다. 무상한 옆 큐·우회 슬롯 반환·ACK=KV완료 대체는 금지한다.
   공용 mailbox 변경은 mock/다른 adapter의 중립 계약과 lost-wakeup·close·실제 양방향 포화를 함께 검사한다.
3. 실제 EventNode/broker의 입력 종료와 이미 수용한 늦은 완료를 연결한다. 현재 local-close의 Ok를
   graceful drain으로 부르지 않는다. adapter-owned Cancel/Drain의 권위·수용 ACK·발행 금지·기존 native
   정산·전 stage release·OUTER 최종 전달을 나눠 계약/버전화하고, 출력 없는 취소와 전후 중복·재시작도
   검사한다. 코드에 없는 event 명령을 legacy service 명령으로 대신하지 않는다.
4. B1 touched-work/불변 입력 clone 비용, B3 bounded admission/KV 예약/row·byte credit, B4 정책,
   B5 실제 placement/ABI·격리/runner를 각각 해당 gate로 이어간다. 관측을 모두 메모리에 보존하는
   드라이브 원장을 bounded-RSS 증명으로 세지 않는다. 오류 시 partial artifact 저장도 아직 별도 작업이다.

이번 slice의 convenience `SubmissionLedger::approve_output`는 실제 drive가 아닌 시험만 사용하는
미사용 경고가 남는다. 증거를 위해 동결한 소스를 마지막에 몰래 청소하지 않았으며 후속 코드 편집에서
범위를 축소하고 재검증한다. 모델 적재·GPU/원격 배포·VRAM-only/RAM 오프로딩은 이번 slice에서 하지 않았다.
현재 fleet의 **VRAM-only 충분성 → 더 큰 RAM 오프로딩 모델** 순서와 최종 다중 컴퓨터 증명은 §1/H0 그대로다.

### 2026-09-07 후속 구현 — completion Full의 실제 반례와 공간 통지

직전 첫 행동을 actual Worker::run에서 재현했다. A의 native 해제 뒤 정확한 ACK를 보류하고 B의
OUTPUT으로 completion을 채우자 A ACK는 입력에 수용돼도 처리되지 않았다. 공간 복구 뒤 기존
출력·해제·관측은 모두 완결됐다. 새 필수 시험은 숨기거나 기대값을 낮추지 않고 **RED로 남긴다**.
원본 소스·실제 재컴파일·실행파일과 복구 양성은
[정산 증거의 completion Full 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)에 보존한다.

중립 mailbox의 공간 통지와 reader 종료 wake는 이 actor 문제의 선행 작업이다. 그 primitive의
구현·검증 결과는 같은 증거 절을 따른다. 이를 추가해도 현재 `publish_or_wait`의 blocking이나
EventNode의 입력 재시도·graceful drain이 사라지지 않는다. **B1/B2/B5는 IN_PROGRESS**이며 이
RED가 남아 있는 동안 이전 전체 green 집계를 현재 집계로 인용하거나 성능 단계로 승격하지 않는다.

**다음 첫 행동은 bounded effect pump를 실제 worker에 연결하여 이 RED를 통과시키는 것**이다.
정확한 목표 의미는 [배치 계약의 출력 포화 절](adapter-batching-layers.md), 시험은 검증 규약
T22~T26이 소유한다. 이미 재현된 ACK 기아를 새 발견처럼 다시 조사하거나 환경변수로 우회하지 않는다.

1. head 제어 의도의 적용/전송 단계를 먼저 명시화하고 조기 ACK 거부 반례를 추가한다. 기존 normal
   RELEASED·terminal proposal이 붙는 SETTLED·checkpoint replay를 함께 유지한다.
2. 직접 LOAD/SESSION/UNLOAD/오류 응답까지 동일한 고정 송신물·효과 예약 경계로 통합한다. count/byte
   예약은 전체 후보 검증 뒤 commit 전에 확보하고, native 결과를 보존할 공간은 호출 전에 확보한다.
   기존 wire 한도를 임의 축소하거나 다른 무상한 큐로 옮기는 것은 이행이 아니다.
3. 입력·capacity·shutdown을 함께 기다리는 actor loop를 연결한다. 정상 ACK는 진행하되 새 native
   발행으로 backlog를 키우지 않는다. 효과의 일부 native 성공 뒤 Pending/Closed/ID 고갈·종료에도
   원본 Event/순서·한 번 실행·불확실/abandonment 회계를 지킨다. capacity API 존재만으로 완료하지 않는다.
4. actual EventNode/broker 포화와 명시 Cancel/Drain을 이어 검증한 뒤, 남은 B1 touched-cost,
   B3 admission/예약/edge credit, B4 정책, B5 placement/ABI/격리/runner를 진행한다.

이 slice는 모델 적재·GPU/원격 배포·실기 웨이브를 수행하지 않는다. §1/H0의 사용자 지정 자원과
VRAM-only 이후 RAM 오프로딩 확장 순서는 그대로다. 한 물리 호스트의 두 GPU를 다중 컴퓨터로 세지 않는다.

### 2026-09-07 후속 구현 — head 제어의 적용·전송 권위

직전 첫 하위 작업인 head 제어 단계를 구현했다. pending 등록만으로 조기 ACK가 슬롯을 반환하거나
Verify/Replay를 재개하는 반례를 실제 codec→handle에서 수정 전 RED로 보존했다. local native 성공과
다음 stage 송신 수용을 별도 상태로 결속했고, 기존 꼬리 proposal/Replay와 whole-event 거부를 유지했다.
실제 효과 소비 시험은 native Frame 응답·receipt/frontier·송신 수용을 따로 통과한다. 초기 상태를
주입한 소비 시험과 실제 run-loop 진행 증거는 구분한다. 의미·ticket 수명은 [배치 계약](adapter-batching-layers.md),
시험 의무는 검증 규약 T23, 봉인·실행·변이는
[정산 증거의 head 제어 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**B1/B2/B5는 IN_PROGRESS**다. 실제 completion Full의 ACK 기아 시험은 여전히 필수 RED이며,
blocking publish·capacity API의 미연결·전체 outbox 예산·Cancel/Drain은 이번에 고치지 않았다.
phase 필드가 존재한다고 actor가 양보하거나 전송 credit이 KV 정산을 증명하는 것은 아니다.

**다음 첫 행동은 고정 송신물과 전체 효과 예약을 실제 소비 경계에 연결하는 것**이다.

착수 반례는 독립 복사본의 `SESSION` 응답 ID 고갈로 고정했다. `control.rs::Worker::session`이
`emit.rs::Worker::emit_bytes`의 ID 검사보다 먼저 sessions.insert를 수행해, 거부 후에도 route가
설치된다. 실제 session()/handle() 두 경로는 RED, 정상 응답 wire 양성은 통과했다. 원본 전체1225 집계에
이 복사본 시험을 합산하지 않는다. 우선 이 반례를 기본 회귀에 이관해 **SESSION 응답 준비·예약 전
상태 변경 금지**를 닫는다. 이 작은 소비 경계에서 시험한 ID/송신물 준비를 native 결과 예산이나
전체 outbox 예약 완성으로 부르지 않고 아래 전체 경로로 이어간다.

1. `worker/effects.rs`, `emit.rs`, `control.rs`, `drive.rs`의 모든 직접 송신을 포함해 보존 비용과
   예약 수명을 감사한다. 이미 발행한 작업이 나중에 만드는 OUTPUT/ACK receipt의 용량은 발행 전에
   확보해야 한다. ACK 도착 때 처음 예약하면 다른 OUTPUT이 예산을 채워 같은 기아가 재발한다.
2. 전체 후보의 count/retained-byte/ID를 상태 commit 전에 예약하고, 한 번 만든 Event를 그대로
   Full 재제출한다. 현재 native frame 상한과 결과·관측 fan-out·base/payload 복사본을 제외하지 않는다.
   결과를 받은 뒤 임의의 작은 cap으로 버리거나, 고정 크기 큐만으로 RSS 상한을 주장하지 않는다.
3. 그 예약을 소비하는 비동기 effect pump에서 입력/capacity/shutdown을 함께 기다린다. 동기 구간용
   ticket은 양보 뒤 재사용하지 않고 재검증한다. native command 내부 양보에는 별도 그룹 예약이 먼저다.
   실제 Full ACK RED와 정상 전달 양성을 모두 유지해 통과시킨다.
4. 이후 순서는 직전 진행 기록의 EventNode/broker·Cancel/Drain 및 B1/B3/B4/B5 잔여를 따른다.

소스는 기준 HEAD 위 미커밋 변경이다. 모델/GPU·C++/원격 실행·배포·커밋/push는 이 slice의 성과가
아니다. §1/H0의 **VRAM-only 충분성 검증 후 RAM 오프로딩 모델 확장**과 다중 컴퓨터 최종 증명은 유지한다.

### 2026-09-07 후속 구현 — SESSION 응답 준비의 소비 경계

직전 첫 반례를 기본 SESSION 회귀로 이관했다. 응답 직렬화·ID 후보·실제 Event 왕복 가능성을
상태 변경 전에 확인하고, 성공했을 때만 session 설치→ID commit→기존 동기 송신으로 진행한다.
입력은 wire-valid지만 원본 ID가 응답 ID/causation에 중복돼 응답 envelope가 커지는 추가 반례도
독립 복사본에서 재현했다. 단순 Event::validate 또는 encode 성공으로는 decoder 승인까지 보장되지
않는다. 기존 정상 wire와 Unicode metadata/body를 유지했다. 정확한 시험/봉인/집계·변이는
[정산 증거의 SESSION 응답 준비 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**B1/B2/B5는 IN_PROGRESS**다. 이 private 준비물은 같은 worker 동기 구간 전용이며 queue count/
retained-byte/미래 native 결과 예약이 아니다. 전체 wire 한도를 줄이거나 중립 protocol을 고친 것이
아니며, 일반 ERROR/LOAD/UNLOAD 응답에는 아직 이 준비 경계가 없다. 큰 원본 ID의 ERROR fallback도
wire-invalid일 수 있다. 준비 실패 시 SESSION 권한 보존과 정상 오류 응답 전달을 구분한다.
현재 completion Full ACK 기아는 필수 RED 그대로이며 Closed 뒤 session 롤백도 이번 범위가 아니다.

**다음 첫 행동은 효과의 보존 표현과 예산을 실제 발행/정산 의무에 결속하는 것**이다.

1. `effects.rs`에서 OUTPUT마다 전체 TAIL base Event를 복제하고 flush에서도 front를 clone하는
   비용부터 명시·축소한다. envelope만 필요한 곳은 본문 전체를 중복 보관하지 않도록 하되 실제
   OUTPUT/관측/제어 Event의 identity·본문·순서가 동일한 소비 회귀를 먼저 고정한다.
2. `release.rs`의 전체 TAIL 효과 및 **앞으로 올 RELEASED가 생성할 소유자 receipt**를 함께
   예약하고, `drive.rs`의 native 호출 전에 결과/forward/관측 의무 상한을 예약한다. planned 예약과
   실제 발행 witness는 분리한다. ACK 도착 후 일반 예산을 처음 요구하는 구조는 허용하지 않는다.
3. count뿐 아니라 보존 bytes·중첩 telemetry fan-out·파싱/Vec capacity·일시 복제를 계측한다.
   현재 frame의2GiB 한도를 RSS 한도로 사용하지 않는다. `capsule/decode.rs::read_capsule`의
   선언 count에 따른 선할당도 예산/유효 입력 길이와 대조해야 하며, 실제 대용량 할당으로 개발 호스트를
   고갈시키는 반증은 금지한다. 협상된 상한 또는 안전한 격리·계측을 사용한다.
4. 모든 직접 응답과 미래 의무가 같은 보존 경계에 들어간 뒤 capacity/input/shutdown actor pump를
   켜서 기존 Full ACK RED를 통과시킨다. 이후 EventNode/broker·Cancel/Drain과 나머지 B1/B3/B4/B5를
   잇는다. 일반 ERROR의 미이관을 잊고 정상 token 경로만으로 포화 해결을 선언하지 않는다.

실기 확장 순서는 §1/H0 그대로다. 이번 모델 파일 목록은 읽기 전용 경로/stat 조사이며 load·GPU·
VRAM-only/RAM 오프로딩 웨이브 실행은 하지 않았다. 소스는 미커밋 변경이며 자동 push/배포는 하지 않는다.

### 2026-09-07 후속 구현 — 효과 보존 표현과 할당 전 검사

직전 첫 작업인 효과 표현을 실제 생산·소비 경로에서 이관했다. OUTPUT뿐 아니라 Forward/관측/
해제 통지의 provenance도 Envelope만 소유하며, flush는 전체 effect를 clone하지 않고 소유권을
옮긴다. 실패하면 본문과 중첩 관측을 원본 의도로 복구한다. 캡슐의 잘못된 outcome/generated 선언이
본문도 없이 Vec를 선할당하던 경로는 최소 wire 크기 검사로 막았다. 정확한 코드 봉인·시험·변이는
[정산 증거의 효과 보존 표현 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**B1/B2/B5는 IN_PROGRESS**다. 실제 completion Full ACK 기아는 여전히 필수 RED다. 보존 표현의
복사 감소는 전체 RSS 상한·고정 Event 재개·처리량 개선·비동기 pump 완성의 증명이 아니다.
활성 입력/파싱 객체·일시 직렬화·실행 중 effect·미래 결과와 통지 의무는 별도로 회계해야 한다.

**다음 첫 행동은 Worker 수명의 ResourceBudget을 실제 응답/발행/반환 소비에 연결하는 것**이다.
상세 예약·ID·수명 계약은 [배치 계약의 출력 포화 절](adapter-batching-layers.md), 실패 반례는
검증 규약 T22~T26이 소유한다. 이미 완료한 표현/검사 작업을 재감사만 하며 반복하지 않는다.

1. composition root→adapter 생성 설정에 자원 선언을 전달하고, Worker 소유 예약 원장을 연결한다.
   일반 보존물·입력·native 임시 공간·미래 반환·실패 진단을 합산한다. queue 개수나 native frame
   한도로 RSS를 대체하지 않는다. 실제 response/native 후보의 예약 실패가 상태·ID·효과를 보존하는
   소비 시험을 함께 넣는다. 사용하지 않는 순수 budget 클래스만 만든 것으로 이 단계를 닫지 않는다.
2. TAIL 후보의 pending release에 미래 owner receipt와 ID 발급 개수 예약을 귀속한다. 원본 제출/
   operation별 권한을 검사하고, 정상 ACK가 일반 예산이 찼어도 자기 예약으로 정산하도록 한다.
   잘못된 ACK·전체 그룹 초과·ID 여력 경계·중복 전환·native 불확실 상태를 실제 consumer로 검증한다.
3. 직접 LOAD/UNLOAD/SESSION/ERROR와 native 결과/forward/관측까지 같은 보존 경계를 완성한다.
   그 뒤 실제 actor가 입력/capacity/shutdown을 함께 처리하게 하고 기존 Full ACK RED를 통과시킨다.
   단일 FIFO 앞단 포화와 이미 수용된 ACK 진행을 구분하며, native 그룹 내부에는 아직 양보하지 않는다.
4. 실제 EventNode/broker 포화·Cancel/Drain 및 B1/B3/B4/B5 잔여를 잇는다. 예약된 반환 입력 경로/
   edge credit 없이 전체 순환망 진행을 승인하지 않는다. 안전성 승인 후 B6/B7 실기 웨이브로 넘어간다.

§1/H0의 **VRAM-only 충분성 검증 뒤 RAM 오프로딩 모델 확장**은 유지한다. 이번 slice에는
모델 적재·GPU 웨이브·원격 배포·C++ 실행·커밋/push가 없다. 최종 다중 컴퓨터 증명도 아직 아니다.

### 2026-09-07 후속 구현 — ACK 진행 설계 재검수와 전체 체크포인트

사용자가 과도한 시간·토큰과 국소 수리의 반복, 장기간 무커밋을 지적했다. 전체 원인을 다시
대조한 결과, 직전 기록의 **전체 ResourceBudget을 현재 ACK 정체 수정의 직렬 선행으로 둔 결정은
과했다.** 전체 RSS·native 임시 공간·EventNode credit는 필요하지만 이 국소 반례의 선행 조건은 아니다.

실제 반례에서 mailbox를 점유한 것은 이미 전송된 B OUTPUT이며, worker가 대기하는 것은 그 뒤
B의 RELEASE forward다. A는 이미 ForwardAccepted이고 정상 ACK가 입력에 들어와 있다. 해결의
최소 단위는 다음 전이를 함께 보존하는 것이다: 동일 활성 Event 유지 → 접근 가능한 ACK의 순수
prepare/commit → 기존 FIFO 뒤 receipt/진단 보존 → 매 offer 직전 head 권위 재검증 → 성공 callback.
native 그룹 내부 선점·재귀 handle/flush·추가 native issue는 허용하지 않는다.

현재 작업 트리에 이 제한된 서비스와 queued/active-suffix/future-receipt ID 개수 대조를 통합했고,
전체 누적 소스·시험·문서를 **WIP 체크포인트**로 함께 커밋한다. 정확한 소스 봉인·전체 집계와 아직
미통과한 회귀는 [정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의
마지막 체크포인트 기록이 소유한다. 이전 전체1244/1/7을 새 수정의 결과로 재사용하지 않는다.

**B1/B2/B5는 IN_PROGRESS**다. 정상 ACK는 pending 권위의 수만큼만 새 receipt를 만들 수 있고,
추가 일반 보관은 FIFO 입력1개·진단1개로 제한한다. 이것은 추가 객체 수에 대한 구조적 한계이지
전체 byte/RSS 예약 또는 native 응답 보관 상한이 아니다. native frame 한도를 RAM 예산으로 쓰지 않는다.
non-ACK가 FIFO 앞을 막거나 두 번째 잘못된 ACK를 보관한 이후의 ACK 진행은 이 국소 보장의 범위 밖이다.
1ms 대기·capacity/input 통합 wake·완전한 고정 outbox·전 경로 byte 예산·Cancel/Drain도 완료하지 않았다.

다음 첫 행동은 **아래 하나의 수정 묶음을 증명하는 것**이다. 새 성능 손잡이나 일반 자원 모델을
먼저 추가하지 않는다. 실패에 맞춰 기존 정상/거부 기대값을 약화하지 않는다.

1. 기존 actual Worker Full ACK 반례, 조기 ACK/전체 그룹 거부, 정상 복구를 그대로 통과시킨다.
2. 같은 실제 하네스에서 잘못된 ACK1개 뒤 정상 ACK, non-ACK 보관/FIFO 복구, SETTLED의 직접
   proposal/Replay를 확인한다. 정상 ACK 정산이 새 native 실행을 일으키지 않는지도 동시에 단언한다.
3. 동일 송신물 재시도·매번 fresh head ticket·ID 미래 의무/queued suffix·overflow/고갈·종료 잔존을
   실제 소비 경로와 독립 복사본 변이로 검증한다. 컴파일 오류나 미실행은 변이 검출이 아니다.
4. 봉인된 최종 전체 시험·변이·실행 범위와 남은 한계를 기록하고 전체 변경을 다시 커밋한다.
   이후 capacity wake/반환 수용 경로·byte 예산·B3/B4/B5 및 승인된 VRAM-only→RAM 실기를 진행한다.

어떤 입력·장애에도 예외가 없다는 전역 보장은 하지 않는다. 각 보장은 상태·실패 모델·진입 경로와
경계 밖을 함께 명시한다. 중간 커밋 이후 비무시 변경0을 확인하며, GPU·배포·push는 이번 체크포인트의
검증이나 권한에 포함하지 않는다.

### 2026-09-07 후속 구현 — 제한된 ACK 진행 검증과 두 번째 전체 체크포인트

누적 변경196파일을 `2e9451a5c` WIP로 먼저 커밋하고 비무시 잔여0을 확인했다. 이어 그 체크포인트의
회귀9개를 수정하고 실제 소비 경로 시험8개를 추가했다. commit 전 의무 검사와 commit 후 전달 장애를
분리했으며 기존 후자 시험의 기대값은 유지했다. native 발행의 ID 거부도 prepared issue 설치 **전**이다.
전체399 Rust 입력 봉인에서 **1253 passed/0 failed/7 ignored**, 57 summary·cargo0이다.
수정5종을 독립 복사본에서 제거하면 모두 실행된 시험이 실패하고 복원하면 다시 통과한다.
정확한 입력·시험명·명령·변이·해시는
[정산 증거의 두 번째 체크포인트 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.

**이번에 닫힌 반례**는 동일 worker의 completion Full 때문에 FIFO에서 접근 가능한 정상 RELEASED/
SETTLED가 정산되지 않던 국소 기아다. 실제 worker에서 오류 ACK1개 뒤 정상 ACK, non-ACK의 원본 보관·
순서 복구, Direct/Checkpoint SETTLED의 공간 복구 전 정산과 native 재진입0을 검사했다. 별도 실제
flush 시험은 Full 도중 ACK로 권위가 은퇴한 제어 재전송을 재검증해 거부하고 원래 의도를 보존한다.
이 마지막 경우는 성공으로 흡수하지 않고 fenced 상태로 남긴다. 완료된 재전송의 투명한 회수는 미구현이다.

**B1/B2/B5는 여전히 IN_PROGRESS**다. 이 변경은 byte/RSS 예약, 모든 입력에서의 진전, 완전한 outbox,
전송/노드 순환망의 credit, graceful Cancel/Drain 또는 성능 개선을 완성하지 않는다. 두 번째 오류나
non-ACK 뒤의 ACK를 추월하지 않으며1ms 대기도 남아 있다. 이 경계를 숨기고 “포화 해결 완료”라고 하지 않는다.

다음 세션의 첫 작업은 **현재 정상인 원장/정산을 다시 만드는 것이 아니라 반환 수용 경로를 검증하는 것**이다.

1. 실제 EventNode→adapter 입력→completion→broker의 소유·용량·wake 흐름을 한 표로 고정한다.
   non-ACK 앞단과 출력 Full이 겹친 최소 순환 대기 반례를 먼저 만든다. 아직 관측하지 않은 전역
   deadlock을 이번 국소 반례에서 추론해 확정하지 않는다. ACK 전용 수용/예약 경로와 FIFO 계약을 함께 결정한다.
2. 입력/capacity/shutdown 통합 wake와 반환 경로를 그 반례에 연결한다. 임의 sleep/threshold 추가,
   ACK의 native 재진입, 일반 요청 무제한 drain으로 해결하지 않는다. 거부 보존·중복·재연결·종료와
   정상 혼합 요청 완주를 같은 실행에서 검사하고 독립 변이로 고정한다.
3. 활성 Event·보류 입력·효과·미래 반환/영수증·native 임시 공간을 포함한 byte 예산을 실제 경로에
   연결한다. 현재 ID 개수 검사를 byte 예약으로 이름만 바꾸지 않는다. B3/B4/B5 잔여를 검증 규약에
   따라 통과한 뒤 B6/B7의 승인된 **VRAM-only→RAM 오프로딩 강한 웨이브**로 넘어간다.
4. 각 응집된 변경과 검증 결과를 전체 중간 커밋으로 남긴다. 비무시 잔여0을 확인하고, 실패한
   체크포인트도 실패 그대로 표시한다. build/모델/원문 실행물은 ignore, 소스·시험·문서는 누락하지 않는다.

이번 두 번째 체크포인트도 C++·실제 모델·GPU·원격 배포·push를 실행한 것은 아니다. 초대형 모델의
정상 프롬프트/응답 전문과 유효 TPS·GPU 활용의 최종 실기 목표는 §1/H0/H6 그대로 남아 있다.

### 2026-09-07 후속 감수 — 외부 감수 대조와 작업 범위 재점검

외부 감수의 11:43~11:45 스냅샷과 **1236/9/7**은 첫 WIP 시점의 결과다. 이후 전체 체크포인트
`96c90f99e`는 **1253/0/7**이다. 9개 회귀를 기대값 완화로 숨기거나 ResourceBudget 완료로
기록하지 않는다. 현재는 접근 가능한 ACK의 국소 진행과 ID 개수 의무 검사이며, B1/B2/B5는
IN_PROGRESS다. 정확한 실행과 이번 작은 API 정리의 결과는
[정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)가 소유한다.

이번 범위 심사에서는 생성 원자료 묶음과 특정 커밋 전용 보관 도구의 Git 포함을 **철회**했다.
이미 커밋된 소스의 반복 해시 목록을 추가해도 다른 머신에서의 실행 재현은 성립하지 않는다.
삭제하지 않고 무시 경로에 보존하며, 장기 증거 보존/재실행 조건은 미충족으로 남긴다.
보관 도구 개발로 배치 작업을 확대하지 않는다. 전체 커밋은 무시 대상 외의 소스·시험·문서를
빠짐없이 포함한다는 뜻이며 생성물까지 무차별로 넣는다는 뜻이 아니다.

**다음 첫 행동은 앞 기록의 반환 수용 경로 검증 그대로**다. 실제 EventNode·broker·Worker를
잇는 최소 반례와 정상 진행 oracle부터 고정한다. 코드상 후보는 정상 추론과 허용된 SESSION
재전달이 각 입력/출력 큐를 함께 채우는 순환 대기다. 아직 실행된 RED가 아니며 순수 PREFILL
웨이브만의 결함이라고 확정하지 않는다. 기존 duplex 시험의 어댑터는 항상 입력을 받거나
시험이 외부에서 여유를 주므로 이 후보를 증명하지 않는다.

시험 전에 각 보류 Event의 소유자·큐 상한·wake·대상과 유한 도달 순서를 기록한다. 재현되기 전
새 ACK lane/예약 정책/임계값을 구현하지 않는다. 재현 후에도 정책/원장과 llama/backend 경계를
유지하며, 정상·포화·종료의 수용 조건을 먼저 정한다. 단계 전환 시
[검증 규약의 시행착오 점검](distributed-batching-verification.md)을 적용한다. 현재의 국소 수정을
반복 재작성하거나 GPU 실험으로 이 정합성 설계를 찾지 않는다.

### 2026-09-07 후속 검증 — 실제 actor 순환 반례와 수정 경계

기준은 `f13e2560b`다. 운영 변경 없이 실제 EventBroker→EventNode→LlamaNodeAdapter→Worker::run의
시험 두 개를 추가했다. native 계산/유한 지연과 post-LOAD 초기 설정만 fake이며 정상 SESSION·
추론·캡슐·해제 상태는 실제 경로에서 만든다. 이전 감수의 실행은 보강 전 소스이므로 현재 소스의
결과로 재사용하지 않는다. 정확한 실행·실패·보류 소유자 표는
[정산 증거의 actor 순환 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)이 소유한다.
봉인400입력의 실행13은1254/1/7 ignored·cargo101이며 cap1 마지막 진행 단언만 실패하고 cap8은
통과했다. 운영 변경 없는 수정 전 RED 체크포인트다. 이전1253/0/7을 현재 상태로 읽지 않는다.

이번 판정은 B2/B3의 **수용 경로와 원인 작업의 후속 공간 보장**이다. 제한된 ack_service를 넓히거나
SESSION을 금지하고, 큐/임계값을 늘리거나, GPU A/B로 정체 원인을 다시 찾는 작업이 아니다.
capacity wake는 비워진 공간을 알릴 뿐 이미 닫힌 대기 고리에 공간을 만들지 못한다. 같은 source/
correlation의 순서와 필수 결과의 공간을 함께 지켜야 하므로 ACK 우선 lane 하나로 완료할 수 없다.
의미 계약은 [배치 계약의 출력 포화 절](adapter-batching-layers.md), 중립 경계는 격리 계약이 소유한다.

**B1/B2/B5는 IN_PROGRESS, B3 end-to-end 예약은 미완**이다. 다음 첫 행동은 봉인 반례를 별도
체크포인트로 남긴 뒤 다음 구현 범위를 닫힌 상태 전이 표로 확정하는 것이다.

1. 원인 작업의 승인 전에 필수 결과·반환·앞선 동일 순서 송신물의 count/retained bytes를 선확보한다.
   adapter는 flight/KV 의미를, 중립 전달층은 공간/소유를 책임진다. 일반 입력이 예약된 반환의
   공간을 소비하지 못하게 하고, 모든 일을 거부하는 구현도 동일 정상 입력 대조에서 실패시킨다.
2. completion→EventNode→broker→worker의 책임 이전과 Full/Closed/중복/취소/결과 불명 전이를
   실제 소비 경로에 함께 연결한다. ACK 처리 뒤의 필수 효과도 확보하며 ID 여력을 byte 예산으로
   오독하지 않는다. 용량 통지는 등록→조건 재검사→대기의 lost-wake 검증과 함께 연결한다.
3. remote `transport.rs::serve`의 Full→연결 종료와 공용 outbound pump의 목적지 간 대기도 같은
   경계에서 다룬다. local actor GREEN을 remote 진행 증명으로 승격하지 않는다. grant를 envelope에
   싣는 설계라면 wire 버전·producer/consumer·구 peer 거부와 llama를 모르는 중립 responder가 필요하다.
4. 수정이 포화 형성 자체를 예방하면 시험 설치 과정에 정상 진행 분기를 둔다. 제출14개·결과6개·
   native KV/해제·순서 oracle는 유지하며 잘못된 포화 상태를 강제로 만들도록 구현을 왜곡하지 않는다.

세 라운드 제한을 새 slice 이름으로 초기화하지 않는다. 요청 이후 앞 기록의 추가 실행12가 첫
라운드이고, 이 고정 반례/정상 대조 및 전체 회귀 묶음13은 둘째로 기록한다. 마지막 라운드를
설계가 덜 정해진 후보 탐색에 쓰지 않는다. 기존 국소 수정으로 전체 순환 문제가 닫힌다고 한
전제가 성립하지 않으면 재설계 필요라고 보고하며, 세 번 안에 전체 B1~B8 완료를 보장하지 않는다.

### 2026-09-07 후속 구현 — 로컬 저장소 예약 기반과 실행 전 체크포인트

수정 전 RED는 `393a6c23e`에 별도로 보존했다. 이번에는 실행 결과를 보고 구현을 고르는 대신
원본 Event/공간 claim의 reserve→publish→owned dequeue→transfer/retire 전이를 먼저 고정했다.
실제 completion 저장소를 같은 원장에 연결하고, 보존 비용은 모든 소유 String/Vec의 capacity로 센다.
기존 일반 publication도 새 저장소를 사용한다. **운영 저장소 코드가 변경된 미검증 WIP**이며,
예약 경로의 제품 활성화·end-to-end 교착 수리는 아니다. 세부 API 제약은 배치 계약이 단독 소유한다.

코드 정적 검토에서 영구적인 단일 Event 초과를 Full 재시도로 취급하는 안과, RELEASE 미리보기 ID를
나중에 다시 발급하는 안을 배제했다. 전자는 기다려도 수용 불가능하고 후자는 앞선 효과의 ID 소비와
충돌한다. 생산자만 예약 API로 바꾸거나 여러 결과 전체를 cap1 슬롯에 예약하는 안도 정상 진행을
막는다. 이 판정을 확인하려고 추가 시험 실행을 하지 않았다.

**마지막 실행 라운드는 아직 사용하지 않았다.** 새 비용/저장소/실제 publication 거부/RELEASE oracle를
작성했지만 컴파일·시험·변이는 미실행이다. 이전1254/1/7은 수정 전 소스의 결과로만 남긴다.
기존 mailbox 시험과 actor cap1/cap8 입력·oracle는 변경하지 않았다. 자세한 변경·미실행 목록은
[정산 증거의 로컬 저장소 절](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)을 따른다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완**. 다음 첫 행동은 새 API를 무조건 켜는 것이 아니라,
원인 작업의 후속 effect 보존과 수신 측 실제 공간 사이의 예약/책임 이전을 연결하는 것이다.

1. 실제 FIFO outbox에서 Event를 한 번 생성하고 ID 의무를 실제 번호로 전환한다. 예약된 저장소
   생산·owned 소비를 EventNode/broker와 함께 이관한다. RELEASE의 알려진 응답 표현과 가변 native
   결과 상한을 구분한다. native 전체 결과 상한이 없는 현재 HELLO를 임의 배수 예산으로 대신하지 않는다.
2. 기존 cap1/14입력/6결과를 그대로 승인 기준으로 사용한다. 예방적 진행으로 포화 설치가 불필요해질
   경우에만 실제 진행 witness가 있는 분기를 추가한다. 정상 입력을 제출하지 않거나 새 예산 때문에
   영원히 거절하는 구현은 실패다. remote·같은 순서 영역·취소/종료도 기존 검증 범위에서 누락하지 않는다.
3. 후보와 반례·정상 대조·변이 목록을 봉인한 뒤 마지막 검증 묶음을 실행한다. 현재 부분 API만 시험해
   마지막 라운드를 소모하거나, 새 이름으로 라운드를 초기화하지 않는다. 미검증 변경도 전체 WIP
   체크포인트로 보존하되 완료/성능 개선으로 보고하지 않는다.

이번 체크포인트에는 모델/GPU/C++/원격 실행·push가 없다. 전체 비무시 변경을 포함하고 생성물은
기존 ignore 경로에 유지한다. VRAM-only→RAM 오프로딩 강한 웨이브의 최종 목적은 바뀌지 않는다.

### 2026-09-07 후속 구현 — 고정 송신물과 예약 연결 전 정적 검토

앞 저장소 WIP는 `7f402aba5`에 보존했다. 이번 변경은 committed effect를 실제 FIFO 선두에서
한 번만 Event로 만들고, 최종 publication 실패에도 그 Event와 후속 의무를 보존하는 단계다.
기존 Full 내부는 이미 같은 Event를 재시도했다. 이번 수정이 그 사실을 처음 구현한 것은 아니다.
broker Full도 원본 allocation을 반환하도록 실제 destination 슬롯 확보를 복사보다 앞에 둔다.
계약의 정확한 상태 구분은 [배치 계약](adapter-batching-layers.md), 시험/정적 검토는
[정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)가 소유한다.

**컴파일·시험·변이 미실행 WIP**다. 작성한 시험8개와 기존 실패 표현 시험의 이관을 통과로 세지
않는다. 두 독립 정적 검토에서 ID 의무 보존과 head 권한 재검증을 확인했고, 시험 이관의 전체
telemetry/Envelope 대조 누락은 실행 전에 보강했다. 수정 전 actor RED의 입력·oracle는 그대로다.
이것은 공간 선예약이나 동기 worker의 양보 구현이 아니므로 actor 교착 해결을 주장하지 않는다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 마지막 검증 라운드는 미사용**이다.
다음 첫 행동은 부분 API를 시험하는 것이 아니라 아래 수용/책임 이전 설계를 실제 연결하는 것이다.

1. 현재 동기 FIFO에서 반환/일반 입력이 각각 어느 저장소를 점유하는지 확인하고, 원인 작업의
   전체 필수 효과 보존 공간과 전달 슬롯을 분리한다. pure 후보 원장에는 복제 가능한 RAII 권한을
   넣지 않는다. opaque 의무 ID와 worker 소유의 선형 공간 권한을 연결한다.
2. 동일 `(source, correlation)`의 PHYSICAL와 뒤따르는 관측은 목적지가 달라도 순서를 유지한다.
   SESSION 응답을 먼저 빼는 특례나 목적지별 무조건 우회만으로 설계를 닫지 않는다. 실제 수신 측
   공간, ACK 후 필수 효과, remote Full 및 native 가변 결과 상한까지 소유/거부 전이를 정한다.
3. 원인 승인 전 필요한 공간이 없으면 정상 보류하며, 기존 작업의 예약된 반환은 진행해야 한다.
   모든 입력을 막는 구현도 기존14입력/6결과 대조에서 실패해야 한다. 구현은 일반 중립 전달 API와
   어댑터의 flight/KV 의미를 분리하고, 실제 EventNode/worker 소비까지 연결한다.
4. 완성된 후보와 고정 반례·정상 대조·변이 묶음을 봉인한 뒤 남은 한 라운드를 실행한다. 컴파일러나
   시험 출력을 다음 설계 선택의 근거로 반복 사용하지 않는다. 예상 밖 실패는 그대로 기록하며
   세 번 안에 전체 B1~B8 또는 예외 없는 전역 완성을 보장했다고 하지 않는다.

이번에도 전체 비무시 변경을 WIP 체크포인트에 포함하며 생성물은 기존 ignore 경로에 둔다.
GPU/모델/원격/C++/push는 실행하지 않았다. 최종 성과 승격 조건과 승인 자원 범위는 §1 그대로다.

### 2026-09-07 후속 구현 — 전달 슬롯과 필수 결과 보존 공간 분리

앞 고정 송신물 WIP는 `bcbadf101`에 보존했다. 이번에는 알려진 fan-out 결과를 보관할 원자 예약과
작은 전달 큐를 분리했다. 큐 한 칸에 결과 세 개의 슬롯을 동시에 요구하는 안은 정상 작업도 시작하지
못하므로 배제했다. 이는 시험 실패 뒤 상한을 늘린 튜닝이 아니라 보존 공간/전달 슬롯의 책임 분리다.
그룹 메타데이터 자체의 비용·경합 재검사·취소와 wake의 경계는 [배치 계약](adapter-batching-layers.md)이 소유한다.

**컴파일·실행시험·변이 미실행 WIP**다. 그룹 예약10개·queue/retained 분리6개의 회귀 oracle를
작성하고 정적 대조만 했다. 일반 publication도 변경됐으므로 무동작 리팩터라 하지 않는다.
기존 actor 반례의 입력·cap1/cap8·결과·native oracle는 그대로이고 end-to-end GREEN을 주장하지 않는다.
생산자만 예약을 켜면 raw 소비자가 reserved front를 읽지 못하는 경계도 여전히 남아 있다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 마지막 검증 라운드는 미사용**이다.
다음 첫 행동은 별도 예약 API를 더 만드는 것이 아니라 **한 원인 작업의 생산→owned 소비→수신 측
책임 이전을 실제 EventNode/worker 경로에 연결하는 것**이다. 먼저 아래 세 연결 조건을 한 번에 닫는다.

1. 원인 ID/응답 슬롯으로 선확보한 의무를 FIFO에서 최종 Event ID/sequence에 한 번 결속한다.
   pure 후보/원장 복사에는 RAII claim을 넣지 않고 worker가 선형 claim을 소유한다. 같은 순서 영역의
   앞선 송신물도 보존하며 broker의 정확한 중복 원장 보관 비용을 독립적으로 센다.
2. PublicationBlocked/EffectsRunnable/Idle/NativeInProgress/Fenced/Closing을 구별하고
   input+capacity+shutdown을 함께 기다린다. Full을 단순 Ok로 바꾸고 기존 `receiver.recv()`로
   내려가면 공간이 돌아와도 영구 대기할 수 있다. ACK 정산과 새 decode 발행 자원 확보를 결합하지 않는다.
3. 알려진 SESSION/제어 응답과 가변 native 결과를 분리한다. 현재 HELLO의 row/sequence 한도와
   수신 frame 상한은 cut tensor count/shape/dtype/alias의 사전 byte 상한이 아니다. native 실행 뒤
   `output_desc.nbytes`로 할당하는 경로에 임의 배수 예산을 붙이지 않는다. 실제 결과 bound와
   remote grant/Full·취소·종료 수용 계약이 없으면 그 부분은 미완으로 남긴다.

이 연결을 끝내기 전 부분 API 시험으로 마지막 회차를 소비하지 않는다. 새 시험·정상 대조·제거 변이를
같은 후보 소스에 봉인하고 기존14입력/6결과/외부 dequeue0 진행을 판정한다. 준비 중 소스를 계속 바꾸며
GPU 수치나 시험 출력으로 설계를 고르는 방식으로 되돌아가지 않는다. 필요한 소스·시험·계약만 전체
WIP 체크포인트로 남기며 생성물은 ignore한다. 성능·다중 컴퓨터 실기 목표는 아직 완료되지 않았다.

### 2026-09-07 후속 구현 — 수용 연결 전 PREFILL 거부 원자성

앞 저장소 분리 WIP는 `d8fff7d27`에 보존했다. 실제 생산→소비 연결을 정적 추적하던 중 PREFILL이
context/incarnation/admission 검증보다 먼저 session key를 기억하는 전제를 발견했다. 그대로 포화 중
입력 처리에 연결하면 거부된 요청도 흔적을 남기므로, 새 예약 API 추가 대신 **실제 수용 함수의 첫
쓰기 경계를 뒤로 옮겼다**. 예약 연결은 아직 하지 않았다. 제한된 보장과 바뀐 오류 우선순위는
[배치 계약의 L2 절](adapter-batching-layers.md#l2-수용점유-admission)이 단독 소유한다.

`prefill_admission_tests.rs`의 실제 codec→handle/직접 PREFILL oracle7개를 작성했다. 거부 시 상태
보존, 진단1개, 원인 수정 뒤 재제출, 기존 pending 우선의 정상 FIFO를 함께 검사하도록 했다.
**컴파일·시험·변이 미실행 WIP**이며 실제 실패를 실행했다고 쓰지 않는다. 기존 actor cap1/cap8의
14입력/6결과/외부 dequeue0 판정은 변경하지 않았다. 마지막 실행 결과는 여전히 수정 전1254/1/7이다.

**B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완; 검증2회 사용·마지막1회 미사용**이다. 다음 첫 행동은
이 전제 수정 위에서 **producer→owned EventNode→broker→receiver의 실제 책임 이전**을 한 묶음으로
연결하는 것이다. raw 호환 다리로 회계를 끊거나 source claim을 exact dedupe 원장에 묶지 않는다.
blocked worker의 input/capacity/shutdown 대기, 원인별 필수 결과·반환 공간, native 결과 bound와
remote acceptance까지의 기존 미완 표면은 그대로다. 국소 PREFILL 수정으로 전체 교착을 닫았다고 하지 않는다.

코드 경로·소유자·오류 종류의 정적 대조를 먼저 끝내고, 완성 후보/반례/정상 대조/제거 변이를 봉인한 뒤
마지막 회차를 사용한다. 새 손잡이·큐 증설·실패 입력 축소로 진행 조건을 바꾸지 않는다. 유지할 소스·
시험·소유 문서만 전체 미검증 체크포인트에 포함하고 생성물은 ignore한다. GPU/원격/모델/C++/push는
이번 작업에서 실행하지 않았으며 최종 강한 웨이브 성과는 미완이다.

### 2026-09-07 후속 구현 — 실제 전달 거부의 원본 소유권

앞 PREFILL WIP는 `2b1d1d539`에 보존했다. 이번 연결 감사는 raw/owned 경계를 실제 호출자 전체로
추적했다. 중간에서 Event를 복사하고 claim을 버리는 opt-in 다리와, source claim을 exact dedupe
원장 수명에 묶는 안을 배제했다. 성공 경로 전체의 소유형 이관은 아직 하지 않았다.

실제 먼저 바꾼 것은 **canonical 거부 반환과 그 소비자**다. broker가 모든 거부의 원 Event를 반환하고,
adapter 입력 Closed도 원본을 돌려주며, EventNode terminal은 양방향 보류물을 반환한다. 제품 node
task가 그 결과를 보존하게 연결했다. 정확한 수명/미완 범위는
[중립 event 계약](event-protocol-v2.md#local-refusal-ownership--limited-implementation-boundary)이 소유한다.
이것은 raw Event 보존이지 공간 claim 이관이나 actor 교착 GREEN이 아니다.

broker4개·실제 node loop3개·llama try_offer2개 회귀를 작성했고 기존 API 시험을 새 반환값에 맞춰
원인/원문 대조를 유지했다. 원본 actor14입력/6결과/cap1·cap8/timeout/oracle는 그대로다.
**컴파일·시험·변이 미실행 WIP; B1/B2/B5 IN_PROGRESS, B3 end-to-end 미완**이다.
마지막 실제 결과1254/1/7은 수정 전 봉인 소스만 증명하며, 검증2회 사용·마지막1회 미사용이다.

다음 첫 행동은 이제 실패 원본을 반환하는 이 실제 경계에서 canonical 소유형 성공 경로를 이관하는
것이다. producer/held input·output/destination/WorkerInput/장기 request 원문을 함께 잇고, 정확한
중복 사본의 독립 비용 및 잠금 밖 알림을 유지한다. raw fallback이나 `handle(event.clone()); retire()`는
장기 보관 사본을 무과금으로 남겨 승인하지 않는다. 그다음이 아니라 **같은 후보의 승격 조건으로**
원인 작업의 필수 결과/반환 공간과 input/capacity/shutdown pump를 연결해야 cap1 진행을 판정한다.
native 결과 사전 bound·remote grant/acceptance·정상 prompt 웨이브는 계속 미완이다.

유지할 운영 변경·필수 회귀·소유 문서만 전체 미검증 체크포인트에 포함하고 생성물은 ignore한다.
이번에 remote/GPU/native/C++/push 실행은 없고, 새 한도나 정상 입력을 줄이는 정책을 추가하지 않았다.
