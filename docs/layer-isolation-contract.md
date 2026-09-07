# P4·어댑터·llama.cpp 계층 격리 계약

2026-09-07 보강. 최초 코드 관찰 기준 `a9e1967fc`; 후속 관찰은 같은 HEAD의 미커밋 작업 트리다.
이 문서는 **층별 책임·허용 의존성·업데이트 충격 격리**의 소유자다.
아래 목표 경계가 모두 구현됐다는 뜻은 아니다. 현재 상태/실행 순서는
[로드맵](distributed-batching-roadmap.md), 격리 시험 I00~I09와 실기 판정은
[검증 규약](distributed-batching-verification.md), 전체 문서 역할은 [문서 안내도](document-map.md)를 따른다.

## 1. 설계 목적과 완료의 뜻

초대형 모델의 다중 컴퓨터 배치 실행을 유지하면서, 잦은 llama.cpp 변경이 P4 전달 코어·비행 원장·
배치 정책으로 번지지 않게 한다. llama.cpp는 단순 장치 wrapper가 아니다. 모델/graph/memory 실행을
소유하는 추상층이고, 그 아래 ggml/backend의 CUDA·CPU·Metal·HIP·Vulkan 등 구현이 별도로 변한다.

“헤더 include 0”, “컴파일 성공”, “이번 pin clean 적용”만으로 격리 완료가 아니다.
허용된 표면만 의존하게 빌드가 강제하고, 의미 변경은 conformance에서 잡으며, 제품 LOAD가
그 검증 결과/실행 identity를 강제해야 한다. 최종 성능 증명은 여전히 실기 웨이브 게이트다.

## 2. 전체 책임과 의존 방향

```text
OUTER: 제품 목표·자원/토폴로지·요청/SLO·스냅샷 트리거
  │  P4 소유 envelope + adapter-owned content-type
P4 event transport / broker / node lifecycle
  │  backend-neutral NodeAdapter, 보존되는 backpressure
llamacpp staged adapter: L0 큐 → L1 원장 ↔ L2 수용 → L3 정책 → L4 증명 → L5 발행
  │  버전화된 stage 명령·capability·identity·결과, 포인터 없음
native stage shell / engine facade
  │  P4 소유 의미 연산과 불투명 핸들
llama engine bridge / private-common compat
  │  llama public API + 격리된 private stage hook
llama.cpp 모델·graph·memory·backend 스케줄러 추상층
  │  ggml/backend 계약
CUDA / CPU / Metal / ... 구상 backend·plugin·장치·buffer
```

화살표는 의미 의존 방향이다. 상향 결과/telemetry는 명시 계약을 통과하며 역방향 소유권을 만들지 않는다.
실제 디렉터리 수나 CMake target 수를 그림에 맞추는 것은 목표가 아니다.

**격리에는 세 가지 서로 다른 합격 조건이 있다.** 의존 격리는 하위 구현을 상위 코드가 이름으로
알지 못하게 하고, 권한 격리는 전송·정책이 실행 원장이나 KV를 임의로 확정하지 못하게 하며,
의미 격리는 컴파일 가능한 upstream 변화도 기존 계약을 깨면 승격하지 못하게 한다.
셋 중 하나만 통과한 리팩터를 전체 계층 격리 완료로 보고하지 않는다.

### P4 저장소의 층별 역할

| 층 / 현재 경로 | 소유 | 소유하지 않는 것 |
| --- | --- | --- |
| OUTER: `tools/event-drive/`, 제품 호출자, `test/benchmarks/` | 모델/토폴로지·SLO·웨이브·배포 자원·스냅샷 명령 트리거·인수 판정 | 엔진 KV를 직접 변경, stage 완료 추측, wire ACK를 생성 완료로 치환 |
| `layers/protocol/` | event envelope, 주소·식별·라우팅·타입 경계 | llama seq/ggml type/CUDA device, 모델별 batch 문법·메모리 계산 |
| `layers/agent/` event 경로 | transport/broker/mailbox·순서·전달·노드 수명·범용 backpressure·종료 | prefill/decode 선택, KV cell 단가, llama context 대기/샘플링 |
| `layers/adapters/adapter/` | backend-neutral NodeAdapter·offer/take·completion 계약, 이벤트 소유권·Full/Closed 구분 | llama 또는 특정 engine의 private 타입·옵션·우회 캐스팅 |
| `layers/service/` | service 경로의 중립 업무 어휘/조율과 기존 KV 코디네이터 | 현재 event worker를 대신하는 숨은 scheduler. 이 층의 시험을 event 경로 증명으로 세지 않음 |
| `entrypoints/agent/` | composition root: 선택 runtime·adapter 등록·설정·process 자원 | 범용 라이브러리가 concrete adapter를 역참조하도록 만드는 등록 편의 |
| concrete adapter | 자신의 content-type 해석·엔진 계약 이행·배치/원장·capability | P4 공용 event 해석기를 모델별로 확장하거나 다른 backend의 상태를 직접 다룸 |

현재 event 경계는 `layers/adapters/adapter/src/node_adapter/mod.rs::NodeAdapter`와
`layers/agent/src/event_node/mod.rs::EventNode`다. `Adapter::start(Work)`의 이전 service 경계와 섞지 않는다.
P4 코어의 2PC/전달 정확성 수정은 허용하지만 mock 및 다른 adapter의 중립 계약 시험을 동반한다.

**명칭의 경계:** 이 문서의 “P4 소유 타입”은 upstream이 아닌 이 저장소 소유라는 뜻이지,
모두 `layers/protocol/`에 넣으라는 뜻이 아니다. batch/fragment/KV/shape/capsule의 의미 타입과
stage wire는 **concrete adapter 소유**다. 공통 코어는 그 payload를 불투명하게 운반한다.
공통으로 승격할 때에는 llama를 모르는 두 번째 구현/mock으로 독립 의미를 먼저 증명한다.

### 의미 역할과 인증·제어 권한은 별개

후속 SESSION 권위 검사는 adapter가 선언한 stage 역할과 envelope endpoint를 대조한다.
`entrypoints/agent/src/event_runtime/transport.rs::serve`는 읽은 source를 인증된 peer 신원에
결속하지 않고 dispatch하며, `layers/agent/src/event_broker/mod.rs::EventBroker::dispatch`는
target 라우팅/전달 원장을 소유한다. 따라서 올바른 terminal endpoint 필드 자체를 사칭하는
발신자를 이번 adapter 검사로 구분한다고 주장하지 않는다.

실제 peer 인증/접속 정책은 중립 transport/composition의 책임이고, 누가 pipeline을 구성·적재·해제할
수 있는지는 제품 제어 권한 계약이다. SESSION 변경 불가가 최초 설정자의 권한을 증명하지 않는다.
이 부분은 미구현·열린 신뢰 경계이며, 사설/신뢰망을 쓸 때도 실기 명세에 제한을 명시해야 한다.
여러 OUTER가 같은 pipeline에 추론을 제출하는 합법적 기능과 control 권한을 구분한다.
즉석에서 한 OUTER만 허용하거나 P4 broker에 llama sequence 규칙을 넣는 수리는 금지한다.
인증을 추가할 때는 peer/source 사칭·세대 재사용과 중립 mock/다른 adapter 회귀를 함께 검증한다.

## 3. 어댑터 L0~L5의 권한

세부 배치 불변식은 [배치 계약](adapter-batching-layers.md)이 소유한다. 이 표는 **누가 무엇을 읽고 쓸 수 있는지**를 고정한다.

| 층 | 입력 / 결과 | 변경 권한과 금지 사항 |
| --- | --- | --- |
| L0 큐 | 요청·취소·캐시 명령 → bounded pending | 이벤트 소유권/순서·대기 예산만 관리. drop을 성공으로 바꾸거나 KV/flight 완료를 직접 기록하지 않음 |
| L1 원장 | 실제 accepted issue·stage 결과·예약 증거 → 일관 상태 | resident/flight/credit ticket/세대의 단일 변경 소유. 전체 반환 validation과 atomic commit; 다른 층의 counter 독립 갱신 금지 |
| L2 수용·점유 | OUTER가 정한 한도/명령 + 실제 stage 메모리 → 예약/대기/명시 거절 | prepared까지 회계, lease/예약·quota 이행. 자동 TTL/victim/snapshot 트리거를 임의 발명하지 않음 |
| L3 구성 정책 | 불변 eligible snapshot·budget·age·검증된 cost profile → 후보 allocation | 순수·결정론적 선택. I/O·native call·state mutation·전송 성공 가정 금지 |
| L4 증명/발행 준비 | 후보 + 전 stage capability/예약 → validated issue | shape·membership·position/phase·byte budget 확인. head에만 합법인 batch 금지. 발행 확정 전 계획을 실행량으로 세지 않음 |
| L5 발행/운반 | validated issue → stage 호출/캡슐 전달/정규화된 결과 | binary 포맷·전송·순서 이행. ACK/terminal/정지점을 혼동하거나 L1 없이 fragment를 소비하지 않음 |

L3는 “llama.cpp와 무관한 아무 일반 큐”가 아니다. llama memory/shape의 **정규화된 의미 제약**을 입력받되
private enum/struct·CUDA API를 직접 보지 않는다. capability의 생성은 native compat의 책임이고,
이행할 수 없는 capability를 광고하면 LOAD/conformance가 실패한다.

같은 GPU의 여러 stage, 한 stage의 여러 GPU, CPU fallback, 여러 호스트가 모두 입력으로 표현돼야 한다.
cost profile은 build/layout/model/workload family에 결속한다. GPU 번호 문자열이나 레이어 수를 실행 비용의 대용으로 쓰지 않는다.

### 상태 변경과 외부 효과의 단일 권위

층을 파일 이름으로만 나누지 않는다. **누가 상태를 확정하고 누가 효과만 실행하는가**가 격리 기준이다.

- L3의 결과는 allocation과 fairness/resume 변경의 **후보**다. 후보만 만들거나 issue가 거부된 경우
  실행량·credit·요청별 서비스 순번을 몰래 소비하지 않는다. 확정 규칙은 L1의 accepted issue와 결속한다.
- L4는 `ValidatedIssue`/`ValidatedSettlement`에 해당하는 검증 결과를 만든다. 타입 이름은 구현 시
  정하되, L5가 raw 후보를 받는 우회 entry point와 다른 층의 raw counter 쓰기는 허용하지 않는다.
- L1은 요청 delta·비행 원장·정산 receipt·출력/settle/release **의도**를 한 commit으로 확정한다.
  native 호출과 네트워크 송신을 순수 transaction 안에서 실행하는 것은 아니다.
- L5는 확정된 effect를 실행하고 결과를 L1에 되돌린다. Full이면 의도를 보관하고, 실행 여부가
  불명확하면 `Uncertain`/fenced 상태로 남긴다. timeout을 미실행으로 가정해 다시 실행하지 않는다.
- 실제 token을 얻는 연산은 engine이 소유한다. token/position/stop을 요청 결과로 승인하고 한 번만
  내보내는 의미는 L1이 소유한다. 꼬리 engine의 성공은 원장 승인을 우회할 출력 권한이 아니다.
- transport 재전달 방지, 생존 프로세스의 completion 멱등, crash 이후 durable exactly-once는
  서로 다른 보장이다. 마지막 것은 내구 receipt/outbox·수신자 dedup 계약 없이 주장하지 않는다.

기준 커밋의 `worker/drive.rs::Worker::emit_tail_results` @ a9e1967fc는 OUTER 출력을 먼저 발행한 다음
head에 TAIL_BATCH를 보냈다. 같은 날 후속 작업 트리에서는 꼬리가 TAIL_BATCH만 전달하고,
head validation/commit 뒤 출력 의도를 발행하도록 바뀌었다. 실제 시험과 소스 범위는
[정산 증거](../layers/adapters/llamacpp/staged/scripts/validation/evidence/2026-09-06-settlement-review.md)의 후속 기록을 따른다.
꼬리 직접 출력을 다시 도입하는 대안은 head 승인 receipt와 중복/재시작 계약을 먼저 별도 검증해야 한다.
이는 adapter 내부 결과 소유권 변경이며 P4 broker에 llama 토큰 해석을 추가할 이유가 아니다.

후속 요청별 해제 증명도 같은 경계를 따른다. adapter가 원본 제출과 native 제어 정산을 결속하고,
OUTER가 자기 송신 기록·terminal 승인·명시 해제 집합을 소비한다. 새 완료 wire의 소유자는
[배치 계약의 해제 절](adapter-batching-layers.md)이다. P4 공용 envelope에 모델 전용 필드를 넣거나
llama/backend 타입을 순수 완료 원장에 올리지 않았다. receipt 라우팅·member 검증은 sender 인증,
상태 직렬화 호환성 또는 실제 execution placement의 증거가 아니다.

반환 receipt만으로 모든 stage의 실행 권한이 봉인되지는 않는다. 요청 incarnation·연산 ID는 발행부터
각 stage의 수신/실행/정산/해제/ack까지 결속해야 한다. 같은 load/session/key/slot을 재사용했을 때
이전 RELEASE가 새 KV를 지우는 경로는 head의 ack 검사만으로 막을 수 없다.
이 의미와 wire version은 concrete adapter 소유다. P4 공통 코어에 llama sequence 규칙을 넣거나
요청 ID 재사용을 임의 금지하는 것으로 대체하지 않는다.

## 4. native 격리층 — 업데이트를 흡수할 자리

| 경계 | 허용 표면 | 격리해야 할 것 |
| --- | --- | --- |
| Rust adapter ↔ native stage protocol | P4 소유 versioned command/result/capability, 크기와 수명 명시 | 포인터·C++ object layout·직렬화된 common_params·상류 enum ordinal 누출 |
| stage shell ↔ engine facade | load/inspect/execute/settle/state/quiesce의 의미 연산, opaque owned handles | llama_context 내부·ggml buffer 내부·sampler ownership 세부·CLI struct |
| llama engine bridge | 현재 pin의 공개 `llama.h`/공개 ggml API, 명시 allowlist | 공개 API도 불변 ABI라고 가정하지 않음. 변화는 이 모듈 안에서 번역 |
| private/common compat | stage hook, internal graph/memory, upstream parser/sampler/speculative/checkpoint 호출 | private header를 다른 native runtime/server/공용 header에 노출하지 않음 |
| upstream ggml/backend | device/plugin allocation·kernel·stream·buffer 구현 | P4 원장/정책에서 device API 직접 호출, core에 CUDA 가정 추가 |

이것은 **bounded module 경계**다. 모든 C++를 무조건 거대한 한 파일에 넣는 것이 아니다.
필요하면 engine/common/stage-hook별 private 구현 파일로 분리하되 허용 모듈·심볼·빌드 타깃 목록을
검토 가능하게 관리한다. allowlist를 늘린 뒤 “침범 0”이라고 보고하지 않는다.

### public 타입과 수명 계약

- adapter/상위 shell에 노출할 DTO는 P4가 사용하는 의미만 가진다. 함수마다 소유/대여·thread affinity·
  buffer 유효 기간·error/취소·sync 보장·callback 재진입을 명시한다.
- `llama_context`에 대한 synchronize/logits reorder·KV 변경은 한 소유자가 직렬화한다.
  여러 sampler가 같은 context를 호출하는 것은 opaque handle이라는 이유로 안전해지지 않는다.
- llama.cpp public 타입은 **native bridge 내부**에서 사용할 수 있다. 현재 일부 native header가 이를 노출하는 것은
  기존 중간 경계다. pure policy/ledger·P4 core/wire에는 올라오지 않는다.
- ABI/version을 늘리는 기준, optional capability의 unknown 처리, mixed-version fleet의 거부를 정의한다.
  알 수 없는 값은 “기본 CUDA”나 “지원”으로 자동 해석하지 않는다.
- common plan/옵션은 upstream이 끝까지 소유한다. 직접 읽는 몇 필드만 복제하거나 27개 getter를 만들어
  구조체 거울을 만드는 대신 **파싱/변환 연산의 호출 위치**를 격리한다. JSON 문법 소유와 호출 위치는 별개다.

### 반드시 구체화할 인터페이스 대장 — 목표 산출물

아래 이름은 현재 존재하는 API라는 주장이 아니라 구현할 **경계별 계약 분류**다.
구현 slice마다 실제 파일·심볼·입출력·소유권·오류·버전·소비 시험 ID를 결속한다.

| 경계 | 계약에 반드시 들어갈 내용 | upstream 변경이 번역될 위치 |
| --- | --- | --- |
| Policy input / candidate | immutable eligible snapshot, 별도 row/byte/KV 예산, 안정 tie-break, 후보와 accepted fairness delta 구분 | native에서 capability를 생성; L3는 정규화된 shape 제약만 읽음 |
| Issue / settlement | logical issue ID, physical membership, expected 구간, accepted/uncertain, 원자 delta와 effect receipt | L1/L4에 엔진 무관한 정산 의미; native ID·결과를 L5에서 매핑 |
| Engine plan / options | 불투명 전체 plan, parse/clone/apply/inspect 연산, 원래 문법·default·간접 옵션 보존 | common/parser 호환 모듈; `Impl`·internal accessor는 consumer에 비공개 |
| Engine execution | owned context, logical→physical split, 실행 승인 시점, buffer 수명, thread affinity, quiesce/settle/release 결과 | llama engine bridge와 private stage-hook 모듈 |
| Capability / placement | 기능별 지원·버전·상한·증거, 구성 요청값과 실측값 구별, actual device/buffer placement | engine/ggml 공개 추상 API에서 수집; vendor 전용 값은 선택 telemetry로 격리 |
| State codec | state ABI·layout·정체성·position·import/export/trim 의미, 지원 행렬과 거부 이유 | state 호환 모듈; 직렬화 byte를 L3가 해석하지 않음 |
| Tensor transport codec | wire dtype/flags·shape/stride/alias/view·endian·범위와 버전, unknown 거부 | native codec이 ggml 표현과 adapter wire 표현을 변환 |

불투명 클래스의 `impl()`를 공개하고 consumer가 internal header를 include할 수 있으면 완성된 경계가 아니다.
friend/internal accessor가 필요하면 compat private 구현 및 white-box test target에만 권한을 준다.
공용 include root로 internal header가 다시 열리지 않는지 실제 consumer 컴파일로 확인한다.

wire는 두 종류를 혼동하지 않는다. 정책/원장이 읽는 **정규화 의미 DTO**와 native 사이에서 운반하는
**엔진 전용 tensor payload**를 구분한다. 후자는 codec identity가 결속된 불투명 표현일 수 있으나,
raw ggml ordinal을 엔진 중립 의미로 해석하거나 서로 다른 pin에서 자동 호환한다고 가정할 수 없다.
기존 raw 표현은 versioned codec+협상으로 봉인하거나 adapter-owned stable dtype/flags로 변환한다.
이 선택과 마이그레이션은 B5에서 확정한다. 단순 정수 타입으로 감싼 것만으로 격리 완료가 되지 않는다.

### CMake·Rust의 강제 경계

- protocol/agent/adapter-contract crate는 concrete backend에 normal dependency를 갖지 않는다.
  등록은 entrypoint에서 하고, pure scheduler/ledger 시험은 llama checkout·GPU·네트워크 없이 빌드된다.
- 중립 완료 저장소는 불투명 Event의 실제 보존 비용·move-only 공간 소유·통지를 제공할 수 있다.
  native 작업이 만드는 필수 결과의 수/상한·flight/KV 권한은 어댑터가 결정한다. 저장소 예약을
  source 인증·분산 wire grant·KV 완료로 사용하지 않는다. 세부 소유 전이는 배치 계약의 로컬 저장소 절을 따른다.
- concrete adapter의 통합 시험 전용 target은 dev-dependency로 중립 EventBroker/EventNode의 공개
  API와 tokio runtime·동기화·시간 primitive를 사용할 수 있다. 실제 전달 경계의 시험 조립이며
  production 의존 역전이나 agent private API 접근 허용이 아니다. normal/build 의존과 구별하고
  pure 원장/정책 시험의 llama·GPU·network 독립성은 유지한다.
- native consumer target에 upstream `src/`, `common/`, `ggml/src`가 직접 또는 transitive include로 노출되지 않게 한다.
  `target_include_directories` 문장에서 지웠더라도 연결 타깃의 INTERFACE가 다시 퍼뜨리면 미완이다.
- full source build와 imported-library/relink를 **둘 다** 검사한다. 한 경로에서 PRIVATE인 것으로 다른 경로를 승인하지 않는다.
- facade 외 production source의 common/private 직접 호출·전방 선언·함수 시그니처 누출도 검사한다.
  include 문자열 검사만으로 header 타입 격리를 선언하지 않는다.
- `llama-common` 최종 DLL/라이브러리가 실행물에 필요한 것과 그 include/API를 consumer에게 공개하는 것은 다르다.
  compat의 private link 의존성은 허용한다. STATIC의 link-only 전파도 고려하고 direct consumer 호출은 금지한다.
- 내부 옵션의 27필드 대조 같은 white-box 시험은 compat 전용 test target에서 유지할 수 있다.
  시험을 삭제하거나 public facade를 테스트 getter로 부풀리지 않는다. runtime 소비 시험은 별도로 남긴다.
- Release에서도 assertion이 실제 실행되어야 한다. 의도적 실패 fixture가 CTest를 실패시키는지 확인한다.

### 의존 manifest를 구현할 때의 최소 레코드

manifest는 아직 없는 **B5 구현 산출물**이다. 다음 필드와 실제 해석기를 함께 납품한다.
파일 이름만 분류하고 컴파일러가 사용한 의존성을 확인하지 않는 목록은 수용하지 않는다.

| 필드 | 반드시 명시할 내용 |
| --- | --- |
| owner / consumers | P4 공통·adapter L0~L5·stage shell·engine bridge·common/private compat·backend 중 어느 책임인지, 허용 consumer 목록 |
| exports | 공개 헤더·함수·타입·wire codec과 버전, 소유/대여·move 이후 상태·thread affinity·buffer 수명·실패 의미 |
| imports | normal/build/dev 의존과 direct/transitive include·compile definition·link-only 의존을 구분한 허용 목록 |
| configurations | full source/imported relink/no-llama, Debug/Release, OS/toolchain 및 선언 backend 조합 |
| exceptions | 현재 누출의 실제 경로/심볼·이유·제거 조건·소유 단계. 예외를 완료나 영구 허용으로 바꾸지 않음 |
| checks / provenance | 실행하는 I/T ID, 정상 consumer와 금지 침범 fixture, source/target 그래프와 결과 digest |

Rust는 workspace 기능 조합별 실제 dependency graph를, C++는 CMake target의 계산된 usage requirements와
실제 compile/link 명령을 검사한다. `PUBLIC` 문장 하나만 고치거나 스캐너 allowlist만 줄여서는 완료가 아니다.
upstream 라이브러리의 링크 필요성, private API의 호출 권한, DLL의 배포 필요성을 서로 분리한다.

계층 추가 자체가 hot path 비용을 늘리지 않도록 큰 불변 prompt/tensor는 소유권·수명·byte 예산이
명확한 참조/이동으로 넘기고, L3/L4/L1 사이에는 작은 후보 delta를 전달한다. adapter 내 층마다
스레드/RPC/직렬화 큐를 새로 만들 필요는 없다. 물리 프로세스·호스트 경계에서만 필요한 transport를 쓴다.
전역 안전성 검사를 없애서 빠르게 만드는 대신 독립 전체 대조 시험을 유지하면서 touched member 인덱스를 쓴다.

## 5. 현재 구현에서 확인한 경계와 구멍

이 표는 코드 관찰이다. 새 강제 gate의 완료 표가 아니다.

| 관찰 | 현재 근거 / 판정 |
| --- | --- |
| Rust staged adapter | Cargo 의존은 p4-adapter/p4-protocol/serde 계열. 정책에 llama native 링크가 없는 기반은 존재 |
| 후속 B1 Rust 경계 | 작업 트리의 `node/flight.rs::FlightLedger`, 요청 issue/settle 전이, `worker/outcome.rs::apply_fragment`는 adapter-owned 행/소유/결과를 검증한다. `worker/effects.rs`가 commit된 의도를 실행한다. 공통 protocol/agent에 llama 타입을 추가하지 않음 |
| 가짜 stage 연결 | 기존 `process/core.rs::ServerControl`의 Box 전달 구현으로 실제 worker 메서드를 구동한다. 별도 scheduler 흉내가 아니라 기존 stage 명령 경계의 시험 대역이다. 실제 LOAD 협상·전체 worker loop·native conformance의 대체는 아님 |
| 후속 실행 identity | `node/ownership.rs::StageOwners`와 native `runtime/physical_authority.hpp::PhysicalAuthority`가 incarnation·제어 연산 receipt를 검사한다. 새 wire/BindLoad의 의미는 [배치 계약](adapter-batching-layers.md)이 소유하며 llama/ggml 타입을 사용하지 않음. PHYSICAL 재실행·crash 복구까지 해결한 것은 아님 |
| 후속 구현의 잔여 | 수신 receipt 보존창 밖의 재전달·fresh ID의 phase/range 순서·재시작 신선성·지속 큐 처리 기회·장문 후보 복사 비용은 미완. 보존 중인 PHYSICAL replay는 [배치 계약](adapter-batching-layers.md)의 후속 수신 원장을 따른다. B1/B2 또는 I gate 전체 완료를 주장하지 않음 |
| L3 후보 순수성 | 후속 작업 트리의 `Scheduler::prepare_plan_with_physical_capacity`/`validate_prepared`/`commit_plan`은 선택과 fairness 승인을 분리한다. 실제 drive와 Simulation은 승인 때 commit하며, 거부→재계획 소비 시험을 둔다. 과거 plan 편의 API 자체의 단위 시험을 제품 호출 증명으로 세지 않음 |
| private llama src 접근 | CMake가 `p4_llama_compat`에만 llama `src/`를 직접 PRIVATE 부여. private include canary도 존재 |
| common 의존 | 후속 gate 실행: 78파일, header include 부채 0 / source 5. 단, gate는 특정 include 패턴 검사임 |
| 타입 누출 | `runtime/request_options_grammar.hpp::parse_grammar_triggers`가 `common_grammar_trigger`를 전방 선언하고 vector 시그니처에 사용. include 0 ≠ common 타입 0 |
| transitive build 노출 | CMake의 runtime이 `llama-common` PUBLIC link. imported relink `_p4_inc`에 common/vendor/ggml-src가 들어가 여러 imported target INTERFACE에 배포됨. 전체 격리 완료 아님 |
| imported 신원 검증 | CMake imported 분기의 source stamp와 별도 build/runtime 디렉터리의 파일 존재 검사는 실제 적재 lib/DLL/plugin이 그 소스에서 만들어졌다는 증거가 아님. 검증 규약 I07/T52의 stale/shadow 주입이 필요 |
| opaque plan | `compat/p4_llama_compat.hpp::LlamaPlan`과 internal header가 존재, plan parser 호출 이동됨. 아직 raw/internal 접근 잔여 |
| opaque의 우회구 | `compat/p4_llama_compat.hpp::LlamaPlan::impl`와 `SamplingOptions::impl`가 public, CMake의 compat PUBLIC `src` 경로로 internal header에 도달 가능. target·접근 권한 격리가 추가로 필요 |
| opaque 수명 시험 보강 | 2026-09-07 후속 작업 트리에서 `runtime/request_options_test_plan.hpp::consume_request_options_plan`이 sampling snapshot을 소유권 이전 전에 취득한다. 실제 options E2E와 모델 없는 `plan_lifetime_test`가 이 시험용 helper를 사용한다. 이전 순서/early-return 변이와 의도적 실패를 통한 Release assert 작동을 확인했다. 실제 모델 options E2E는 아직 미실행이며 I04/T03 전체 완료가 아니다 |
| native 공개 API | compat header가 `llama.h`/`ggml-backend.h` 및 공개 핸들을 노출. pure Rust 경계와 native 내부 경계의 증명 범위는 다름 |
| wire 의미 누출 | Rust `v2/capsule.rs::TensorDescriptor`는 `tensor_type: i32`, `Invocation`은 `flags: u32`를 운반. native `llama_stage_runtime_physical.cpp::StageRuntime::capture_execution`은 hook flags를 복사. codec 의미·버전 결속을 별도로 감사해야 함 |
| identity | pin/patch/backend inventory 전달 기반 존재. 제품 LOAD의 실행 소유권 bind는 추가됐지만 실제 placement·stage/state ABI·검증 manifest 강제는 미완. 실행 epoch와 backend 의미 호환을 혼동하지 않음 |

native 경로 기준은 `layers/adapters/llamacpp/staged/server/src/`, 빌드 기준은
`layers/adapters/llamacpp/staged/server/CMakeLists.txt`의 `p4_staged_llama_runtime`, `p4_llama_compat`,
`P4_STAGED_LLAMA_BUILD_DIR` 분기다. 위 소스 부채 숫자를 다른 문서 산문에 다시 복제하지 않는다.

Rust `StageOwners`와 C++ `PhysicalAuthority`는 같은 의미의 다른 구현이다. 두 unit suite가 각각
통과했다고 언어 경계의 일치를 보증하지 않는다. 검증 규약의 공통 transcript·독립 oracle 대조를
통과해야 하며, 그 대조 또한 실제 llama KV/backend conformance의 대체가 아니다.

현재 `last_load_generation`은 `AdapterState::default`에서 초기화되고 `Worker::new`가 새 state를 만든다.
따라서 현재 수명 내 highwater/receipt 보호를 **같은 Worker/load 수명 밖의 신선성 보장**으로 확대하지 않는다.
agent 프로세스를 유지해도 Worker 재생성만으로 경계가 바뀔 수 있다. 새 Worker/agent와 재연결을 넘는
load/run 신선성은 T18/T24의 별도 잔여이며, 내구 epoch 권위가 이미 있다는 뜻이 아니다.

## 6. 매우 잦은 upstream 변경의 처리 과정

업데이트 빈도는 현재 기간에서 새로 측정한다. 과거 일평균 커밋 수를 영구 상수로 쓰지 않는다.
“pull하면 값만 바뀐다”도, “24패치 clean이므로 의미가 동일하다”도 허용하지 않는다.

1. **발견과 채택 분리**: 후보 commit/release를 수집하되 production pin은 승인 전 움직이지 않는다.
   새 pin 실험은 clean 별도 worktree에서 한다. dirty upstream을 지우거나 원격 production을 따라 움직이지 않는다.
2. **변경 분류**: 공개 API·common parser/sampler·private graph/memory·state codec·model feature·ggml/backend·build를 분리한다.
   변경 파일/심볼뿐 아니라 KV/position/alias·view·stream 동기화 의미 변경을 검토한다.
3. **패치 재생**: stage hook / upstream fix / model feature의 목적·의존 순서·허용 파일/심볼을 기록한다.
   순수 upstream fix는 흡수됐는지 탐지하고 regression으로 확인한 뒤 제거한다. 모델/훅 패치에 의존성이 있으면
   manifest에 명시하며 모든 묶음이 임의 순서로 독립 적용된다고 가정하지 않는다.
4. **격리 내 적응**: 허용 compat 모듈과 patch queue를 수정한다. 위층의 기능 요구가 같다면 protocol/ledger/policy
   소스와 deterministic traces는 그대로여야 한다. 바뀌면 upstream에 끌려간 변경인지 새 요구인지 따로 심사한다.
5. **정체성 판정**: build provenance, stage ABI, state ABI, actual layout을 분리한다. 매 빌드마다 state identity를 깨지도,
   upstream state writer가 바뀌었는데 자동 호환을 선언하지도 않는다. 저장 규약의 행렬을 통과한 것만 허용한다.
6. **게이트**: pristine replay·분류/EOL·I/T/native·N-1 호환 시험 후 선언한 production backend와 fleet에서 검증한다.
   CPU 성공으로 CUDA/Metal 성공을 대신하지 않는다. GPU machine이 없으면 해당 승격을 BLOCKED로 둔다.
7. **웨이브와 채택**: 최종 모델의 고정 spec으로 정상 응답·유효 TPS/GPU·메모리·drain 비회귀를 확인한다.
   이전 pin/binary/model identity를 보존해 안전한 배포 rollback은 가능하게 하되 state 호환성은 별도 판정한다.

모든 upstream commit을 즉시 배포하거나 항상 전체 GPU 실험을 해야 한다는 뜻이 아니다.
후보 검사는 자동화하고 **선정 pin의 production 승격**에 모든 해당 게이트를 요구한다.

### 채택 pin의 변경 예산과 버전 경계

- 동일 제품 의미·동일 정규화 capability를 유지하는 적응에서 P4 공통/adapter L1·L3 소스 변경은
  **0을 목표 계약**으로 둔다. C++ compat 파일 개수 1을 목표로 두지 않는다. 허용 모듈 안의 수정량과
  실제 수동 추종 비용을 기록한다. 위층 수정이 필요하면 누출 수리인지 새로운 제품 계약인지 먼저 판정한다.
- 비교 trace는 논리 입력·선택·발행 membership·정산·거부의 버전화된 의미 투영이다. build ID·실제 시각·
  장치 이름 차이를 임의로 삭제해 같게 만들지 않는다. 제외 필드와 이유를 비교 전에 봉인한다.
  정규화 입력 자체가 달라졌다면 새 capability 검토이며, 이전 정책 trace와 무조건 동일해야 하는 시험이 아니다.
- P4 event 버전, adapter stage 명령 버전, tensor codec, engine 실행 ABI, state ABI, actual layout은
  각각의 변경 이유를 가진다. parser rename 때문에 저장 캐시 전부를 무효화하거나 kernel 교체를
  state 호환 증거로 삼지 않는다. 구체적인 저장 identity는 저장 규약의 단독 계약을 따른다.
- 지원 pin/backend 조합과 혼합 fleet 허용 행렬은 명시한다. 원칙은 검증된 조합만 LOAD 허용이다.
  backend가 바뀌면 비용 profile을 다시 검증하지만, 그 사실만으로 L3에 CUDA/Metal 분기를 추가하지 않는다.
- 후보 pin 채택은 가동 중 context에 새 라이브러리/핸들을 덮어 끼우는 작업이 아니다. 기존 작업의
  drain/실패 수렴과 새 load의 identity 협상이 필요하다. 이전 바이너리로 되돌릴 수 있어도 새 state를
  이전 reader가 읽을 수 있는지는 별도다. ABI가 같다는 문자열만으로 역호환을 승인하지 않는다.

### 변화 종류별 충격 흡수와 실패 위치

| 변경 | 보통 수정되는 범위 | 위층을 수정하지 않고 검출/차단할 것 |
| --- | --- | --- |
| common CLI/샘플러/옵션 구조·default | common 호환 구현과 그 내부 시험 | 옵션 유실·default 의미 변경은 I04 실패. public getter 사본을 늘리지 않음 |
| llama 공개 API / private graph hook | engine bridge / stage-hook 패치 | I03에서 동일 입력의 위층 source·정산 trace 유지. 호출·버퍼 수명 변경은 conformance 실패 |
| model memory / state writer / split 의미 | capability·state codec·model 패치 | I05/T53/K03에서 의미 불일치 검출; 미감사 capability 광고/복원 거부 |
| ggml dtype / buffer / backend interface | tensor codec·engine backend bridge | unknown codec·alias/view·layout 조합 LOAD/실행 거부; ordinal 재배열도 반증 |
| CUDA·CPU·Metal kernel / plugin / toolchain | upstream backend·빌드/패키징 | 해당 backend의 실제 연산·placement·library identity·수치/웨이브 회귀 검사. L1/L3에 vendor 분기 추가 금지 |
| 새로운 제품 의미 기능 | 별도 adapter 계약 변경 | 새 요구에 필요한 DTO/version과 정책 변경을 명시 승인. upstream 적응 패치에 숨기지 않음 |

backend별 최적 kernel/stream을 P4가 다시 구현하는 것이 목표가 아니다. ggml/backend 추상 API를 이용하고,
backend별 수명·동기화·buffer 제약은 conformance가 확인한 capability로 올린다.
같은 모델이라도 backend 집합이 바뀌면 호환·비용 profile을 다시 확인한다. runtime discovery와
“사용 가능한 장치 목록”은 실제 텐서 placement나 해당 조합의 승격 증거가 아니다.

## 7. 추종 비용을 실제로 보고할 항목

pin별 drift 기간/커밋 수, 변경 분류, clean/fuzz/3-way/manual/conflict 패치 수, 수정 모듈/심볼,
상위 policy/ledger/protocol 변경 수와 이유, build/test 시간, 수동 작업 시간, 실패/회귀 원인,
state ABI 증감 및 남은 부채를 기록한다. patch 파일 개수 고정만으로 비용 흡수를 증명하지 않는다.

다음 두 방향의 반증이 필요하다:

- common/private API rename 같은 변화는 compat 적응만으로 동일 상위 계약/trace를 유지한다.
- 반환 의미·KV/alias·state codec 변화는 컴파일이 돼도 conformance가 실패한다.

실제 연속 pin들의 추종 기록을 누적한다. 특정 하루 무충돌 표본으로 잦은 업데이트 내성을 일반화하지 않는다.
격리 수정이 끝나도 기존 batch/worker 오류는 별개이며, 어느 한쪽 완료로 다른 쪽을 닫지 않는다.

## 8. 구조 격리의 납품 조건

실행 순서는 로드맵이 소유한다. 다음은 순서를 새로 만드는 것이 아니라 **B1~B5/B8이 내야 할 산출물**이다.

- 의존 manifest: 실제 crate/target/module별 owner·허용 direct/transitive dependency·public header/API·
  private include root·허용 upstream hook 목록. 예외는 근거·제거 조건·검사 ID를 가지고 자동 승인하지 않는다.
- 계약 대장: §4의 인터페이스별 실제 심볼과 소유/수명/취소/동기화/버전 명세. 같은 문구를 여러 문서에 복사하지 않는다.
- 빌드 증거: llama 없는 pure 빌드, 선언 backend의 full build, imported relink 각각에서 허용 consumer는
  성공하고 금지 include/type/link 침범은 실패. 디렉터리 이동과 문자열 검색 결과는 대체 증거가 아니다.
- 변경 증거: 의미 보존 API 변화는 compat만 수정하고 위층 source/trace를 유지; 컴파일 가능한 의미 변화는
  conformance가 거부. 동작하는 mock과 실제 engine은 같은 정규화 결과/실패 계약을 따른다.
- 운영 증거: 제품 LOAD가 필수 계약·codec·layout·승격 manifest를 대조한다. 필수 unknown은 거부하고,
  선택 telemetry 부재와 실행 안전성 capability 부재는 구분한다. 드라이브의 사전 대조만으로 닫지 않는다.

요약하면 **P4는 전달, 어댑터는 실행 의미와 배치, 호환층은 upstream 번역, llama.cpp는 모델 실행,
ggml/backend는 실제 장치 계산**을 소유한다. 격리 완료는 이 책임이 코드·타입·빌드·실패 시험으로
강제되며 채택 pin 변경에도 유지되는 상태다. 지금은 그 목표의 일부만 구현돼 있다.
