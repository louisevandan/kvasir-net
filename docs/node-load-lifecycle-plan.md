# LOAD·UNLOAD에 노드 수명을 통합하는 구현계획

작성: 2026-09-15 KST. 지위: 사용자 합의를 고정한 **미구현 계획·새 세션 인수인계**.
코드 감사 기준 HEAD: `c16cbfa2abe8e7bc0bd7ce4f3e4c568f6a6c0569`.
계획 작성 시 공통 adapter retention·transport·INSPECT·llama/HF에 별도 미커밋 변경이 있었다.
현재 구현·시험 통과를 뜻하지 않는다. 전체 실행 순서와 진행 상태는
[로드맵](distributed-batching-roadmap.md#current-status)이 소유한다.

## 1. 합의된 목표와 책임 — 다시 설계하지 말 것

- **노드는 모델 또는 담당 모델 구간의 적재 인스턴스다.** 빈 노드를 미리 생성하거나 정상 언로드 후 남겨 두지 않는다.
- 상주 에이전트는 모델이 없어도 존재한다. GPU·물리 머신·모델 파일은 노드의 identity가 아니다.
- 같은 GPU에 두 모델을 적재하면 노드 두 개다. 같은 모델을 두 번 독립 적재해도 노드 두 개다.
- 외부 호출자가 노드 ID를 공급한다. 해당 에이전트에서 이미 사용 중인 ID는 LOAD에서 거부한다.
  적재 중·실행 중·해제 중·회수 불명으로 자원을 소유한 인스턴스도 사용 중에 포함한다.
- ID 검사와 등록은 원자적으로 수행한다. GPU당 한 노드 제한, 모델 경로 중복 금지,
  전역 UUID 발급 서비스, 영구 ID 소진 규칙은 도입하지 않는다.
- 기존 node generation과 adapter load generation 검증은 유지한다. ID 중복 판단은 generation과
  무관하게 현재 ID 점유로 한다. 해제 후 같은 ID 재사용 시에는 기존 broker의 새 generation 규칙을 따른다.
- P4 외부 API는 **개별 LOAD와 UNLOAD**만으로 노드 생성·제거를 완료한다. 별도 CREATE·DELETE를 요구하지 않는다.
- **OUTER가 전체 모델의 다중 노드 적재를 조율한다.** 개별 명령 발행, 전체 성공 판정,
  하나라도 거부/실패했을 때 성공한 노드의 UNLOAD, 아직 적재 중이거나 결과가 불명인 노드의 추적·회수는 OUTER 책임이다.
- P4는 자기 에이전트의 개별 적재·해제와 정확한 결과 반환만 책임진다. 다른 에이전트의 노드를
  자동 회수하거나 전체 모델용 commit/abort·2PC·전역 배포 관리자를 만들지 않는다.

## 2. 범위와 완료의 의미

### 포함

1. 현재 event runtime에서 CREATE의 중복 검사·등록·adapter 생성 책임을 LOAD에 통합.
2. UNLOAD 완료에 native/worker 자원 해제, event 소유권 정리, node route/owner 제거를 통합.
3. backend 중립 수명 명령·완료 계약과 llama.cpp/HF 양쪽 구현.
4. Rust event-drive, HF Python 호출자와 현재 수용 스크립트의 CREATE·DELETE 제거 및 결과 검증 이관.
5. INSPECT, 거부/실패 결과, 기존 retained/receipt/resource-profile 보호, 실제 소비 경로 시험.

### 별도 작업

- OUTER의 다중 노드 실패 자동 회수 기능은 OUTER 소유 후속 작업이다. 이 계획에서는 호출 API를 이관하고
  그 책임을 명시한다. 기존 누락을 P4 내부 기능으로 메우거나 이미 구현된 것처럼 기록하지 않는다.
- 배치 정책·KV 연산·native 모델 계산·분산 성능 개선·일반 장애 복구 플랫폼은 범위 밖이다.
- 이전 service 경로의 `Agent::create_node/delete_node`를 현재 event 경로와 혼동하지 않는다.
  실제 제품 호출이 남았는지 조사하되 무관한 legacy API 전체 삭제를 이 작업의 선행 조건으로 만들지 않는다.
- 원격 배포·기존 프로세스 종료·push는 이 문서로 새 권한이 생기지 않는다.

완료는 정상 경로가 `LOAD → SESSION/요청 → UNLOAD`이고 마지막 응답을 받은 시점에 노드와
그 native 자원이 제거된 상태다. 실제 원격 응답 수신과 transport receipt 퇴역은 각각 관측한다.
소형 conformance 통과는 [최종 다중 머신 수용](distributed-batching-verification.md)의 대체가 아니다.

## 3. 새 세션 시작 절차

1. [AGENTS.md](../AGENTS.md), [로드맵](distributed-batching-roadmap.md),
   [검증 규약](distributed-batching-verification.md), [격리 계약](layer-isolation-contract.md),
   [문서 안내도](document-map.md)를 읽고 이 문서를 끝까지 읽는다.
2. `F:\dev\p4`의 HEAD·branch·전체 dirty/untracked·실행 경로를 확인한다. 기준 HEAD와 다르면
   아래 경로의 차이를 먼저 감사한다. 이미 구현된 부분을 옛 코드로 되돌리지 않는다.
3. 작성 당시 겹치는 작업은 `RetainedNodeAdapter::retention_snapshot`, transport 비용/잔량,
   INSPECT, llama worker의 request/native buffer 보존이다. 해당 최신 계약을 먼저 통합한다.
   오래된 trait 정의를 복사하거나 resource profile 검사·예약을 제거하지 않는다.
4. 다른 작성자의 변경·실행 중 측정 arm을 건드리지 않는다. 병행 중이면 충돌 없는 독립 checkout에서
   작업하고 그 변경의 통합 기준을 기록한다. 사용자 작업 트리를 reset/checkout으로 복원하지 않는다.
5. 최초 착수는 §4 호출 경로 감사와 §8 시험의 결정론적 사전 검토다. 문서만 요구받은 세션에서는
   구현·시험용 모델 실행을 시작하지 않는다. 구현 지시를 받은 새 세션은 §7의 작업 순서를 따른다.

```powershell
Set-Location F:\dev\p4
git rev-parse HEAD
git branch --show-current
git status --short
git diff --stat
git diff c16cbfa2abe8e7bc0bd7ce4f3e4c568f6a6c0569 -- entrypoints/agent/src/event_runtime layers/agent/src/event_broker layers/agent/src/event_node layers/adapters/adapter/src/node_adapter tools/event-drive/src/run
```

## 4. 기준 코드와 변경 위치

아래는 관찰된 코드 위치다. 표의 변경 내용은 구현 목표다.

| 위치 | 현재 역할 / 필요한 변경 |
| --- | --- |
| `entrypoints/agent/src/event_runtime/control.rs` | CREATE/DELETE와 `NodeOwner` 소유. LOAD 수락·중복 검사·노드별 비동기 수명 작업·최종 제거로 이관 |
| `entrypoints/agent/src/event_runtime/adapters.rs` | 지원 kind와 factory. 같은 목록으로 LOAD 지원 광고/실제 생성, disabled kind 사전 거부 |
| `entrypoints/agent/src/event_runtime/control/inspection/` | 등록 노드와 task/retention 관측. loading/unloading 및 회수 실패를 구분, 정상 해제 후 nodes에서 제거 |
| `entrypoints/agent/src/event_runtime/transport.rs` | ingress·보존 전송·reconcile·실제 비용. 반환 문맥과 기존 B1/B2/B3 비용 계약 보존 |
| `layers/protocol/src/event/` | 공통 수명 요청/결과의 codec 및 상수 추가 후보. 기존 P4E3 envelope·hop wire를 불필요하게 변경하지 않음 |
| `layers/agent/src/event_broker/{mod.rs,retained.rs}` | ID 등록·generation·입구 fence. LOAD의 원자적 점유와 UNLOAD 종료 barrier에 사용 |
| `layers/agent/src/event_node/retained.rs` | 실제 입력/출력 전달과 실패 시 held Event 소유. 수명 완료를 owner에게 전달하고 잔여 소유권을 보존 |
| `layers/adapters/adapter/src/node_adapter/mod.rs` | `RetainedNodeAdapter` 중립 경계. typed 수명 결과 추가, snapshot 문자열을 완료 신호로 사용하지 않음 |
| `layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/{control.rs,shutdown.rs}` | 실제 LOAD/UNLOAD와 busy 검사. 현재 unloaded 설정 뒤 응답 생성 순서를 typed 완료와 결속 |
| `layers/adapters/llamacpp/staged/adapter/src/v2/node/{retained.rs,worker.rs}` | 실제 adapter와 worker 종료·retention. native 자원·응답·claim의 해제 시점 연결 |
| `layers/adapters/hf/adapter/src/{construction,lifecycle,retained}/` | HF 실제 LOAD/UNLOAD/abort·Python child 수명. 별도 [HF 규칙](../layers/adapters/hf/AGENTS.md) 적용 |
| `tools/event-drive/src/run/{mod.rs,load.rs,replies.rs,config.rs}` | CREATE 전체 → 노드별 LOAD → SESSION, UNLOAD 전체 → DELETE. 새 개별 수명 API와 정확 결과 확인으로 이관 |
| `layers/adapters/hf/python/p4hfadapter/models/qwen3_5_0_8b/event_pipeline/__init__.py` | 노드별 CREATE/LOAD·shutdown/close. 새 수명 API 이관, 모델 연산/직접 worker 경로로 우회 금지 |
| `layers/adapters/hf/scripts/verification/`, `test/benchmarks/`, `tools/` | 동적 `node.{op}` 문자열까지 찾아 현재 호출자/fixture 이관. 날짜별 과거 artifact는 수정하지 않음 |

Rust LOAD는 `agent_load_waves`로 agent 간 병렬·같은 agent 내 순차 실행한다. 모두 성공한 후
build 호환성을 검사한다. `execute`의 `load::drive(...).await?`가 실패하면 뒤 teardown에 도달하지 않는다.
HF도 개별 명령을 보낸다. 이 부분 실패 회수 누락은 OUTER 문제이며 P4 전체 적재 기능의 부재와 구분한다.

## 5. 목표 명령 계약

이 절은 구현용 제안 규격이다. M1에서 codec·literal fixture로 고정하고 현행
[event 계약](event-protocol-v2.md)에 반영한다. 기존 명령인 것처럼 사용하지 않는다.

### 5.1 외부 수명 요청

- LOAD/UNLOAD 모두 `Endpoint::Agent(해당 agent)`를 대상으로 한다. LOAD 전에는 노드가 없고,
  UNLOAD 결과는 제거 후에도 에이전트가 책임져야 하기 때문이다.
- 새 content type: `application/vnd.p4.node.load-v1`, `application/vnd.p4.node.unload-v1`,
  결과 `application/vnd.p4.node.lifecycle-result-v1`.
- payload 형식: `metadata_len: u32 little-endian` + 해당 길이의 UTF-8 JSON metadata + 나머지 opaque adapter bytes.
  metadata에 `schema:1`, `node_id`, `node_generation`, `adapter_kind`, `adapter_content_type`를 둔다.
  LOAD에는 기존 CREATE의 queue/completion/retained capacity와 retained bytes 설정도 포함한다.
  adapter load generation·실제 plan·binary·장치·HF job 등은 opaque bytes 안에 기존 어댑터 형식으로 둔다.
- metadata 상한은 M1 literal fixture에 명시하고, 길이 overflow·잘림·필수 필드·unsupported version을
  생성/native 효과 전에 검사한다. 전체 payload와 내부 전달에 실제 retained-byte 상한을 적용한다.
  기존 LOAD resource profile과 transport/edge/receipt 검사를 그대로 실제 소비 경로에서 통과시킨다.
- 내부 전달은 원 요청의 OUTER source·return route·correlation·deadline·원인 identity를 보존하고
  target을 생성한 node endpoint로 결속한다. HF의 Outer source/토폴로지 검사를 우회하지 않는다.
  이 전달에서 같은 broker event ID를 다른 payload로 재등록하지 않는다. 파생 Event가 필요하면
  새 event ID와 원 요청 causation을 명시하고 원본 비용 소유권을 결속한다.
- 일반 SESSION/추론/정산 Event는 기존 node endpoint로 전송한다. unknown node에 일반 Event가
  왔다고 노드를 자동 생성하지 않는다. 외부 node-target LOAD/UNLOAD 우회 경로는 차단한다.

### 5.2 완료 결과와 재전송

- 결과 발신자는 해당 agent이며 node ID/generation, `operation:load|unload`,
  `status:succeeded|rejected|failed`, `resource_state:absent|present|unknown`,
  최초 오류와 cleanup 오류, adapter 결과의 content type/opaque bytes를 담는다.
- 최종 성공은 LOAD의 실행 준비 완료 또는 UNLOAD의 자원·node 제거 완료다. 단순 접수 ACK나
  socket write 성공을 수명 완료로 반환하지 않는다. OUTER의 source/causation 검증도 함께 바꾼다.
- 동일 Event의 transport 재전송은 기존 receipt/중복 억제 계약을 따른다. 새 LOAD 명령에 사용 중 ID가
  오면 같은 모델/plan이어도 거부한다. 새 UUID를 내부 발급하거나 기존 노드를 덮어쓰지 않는다.
- 없는 ID의 새 UNLOAD는 `rejected/resource_state=absent`로 명시한다. 과거 성공을 추측하지 않는다.
  이미 생성한 원래 결과의 전송 불명은 기존 transport reconcile/INSPECT로 추적한다.
- lifecycle 결과를 위해 전역 배포 원장이나 무제한 재시도 캐시를 만들지 않는다. 결과/실패 owner는
  에이전트의 bounded retained 저장소로 소유하며, 실제 전송·receipt의 비용 수명은 보존한다.

## 6. 로컬 상태 전이와 소유권

### LOAD

1. envelope·공통 metadata·지원 kind·설정·결과 저장 공간을 검사한다.
2. 현재 ID 점유 검사와 `loading` 등록을 한 원자적 절차로 수행한다. adapter 생성 실패 등 native 이전
   실패에는 생성한 mailbox/route/예약만 회수한다. 기존 같은 ID의 노드는 전혀 변경하지 않는다.
3. 개별 worker에 적재를 맡긴다. agent 제어 루프에서 긴 native LOAD를 기다려 다른 노드의
   INSPECT/UNLOAD/reconcile를 막지 않는다. 모델별 payload 검증과 실제 적재는 adapter 소유다.
4. typed 완료로 성공하면 노드를 실행 가능하게 하고 정확한 결과를 반환한다. loading 중 일반 요청은
   기존 not-ready 계약으로 거부하며 native 추론을 시작하지 않는다.
5. 적재 실패 후 자기 자원 회수가 확인되면 node를 제거하고 실패 결과는 agent가 소유한다.
   cleanup 불명/잔존 자원이 있으면 해당 ID의 실패 인스턴스를 격리·관측 가능하게 유지한다.
   다른 node는 변경하지 않는다. 이것은 재사용 가능한 빈 노드가 아니다.

### UNLOAD

1. ID/generation·adapter identity를 검사한다. 기존 accepted input과 경쟁하지 않게 종료 barrier를 둔다.
   새 작업의 수용과 종료 확정이 교차하지 않아야 한다. barrier 이전 입력을 버리지 않는다.
2. 기존 busy/정산/KV/effect/출력 소유 검사를 보존한다. busy 거부면 임시 fence를 풀고 같은 노드가
   기존 요청을 끝낼 수 있어야 한다. 정산 메시지를 막은 채 busy가 풀리길 기다리는 교착을 만들지 않는다.
3. 종료 가능한 시점에 adapter가 자기 native/child를 해제한다. 실패·불명은 성공으로 바꾸지 않고
   격리 및 최초 오류/cleanup 오류를 보존한다. 실패 node의 회수 경로도 명시하고 정상 UNLOAD로 위장하지 않는다.
4. adapter는 typed 수명 결과를 반환한다. `snapshot() == "unloaded"` 감시로 삭제하지 않는다.
   특히 llama.cpp의 현재 `set_snapshot("unloaded") → emit_json` 사이에는 삭제 경쟁이 있다.
5. 원 UNLOAD 입력과 terminal 결과를 agent 소유로 넘기고, 나머지 queued/held input/output가
   남지 않았음을 권위 있는 retained 관측으로 확인한다. **UNLOAD 자신의 입력/결과를 node에 둔 채
   count=0을 기다리는 자기 대기**를 만들지 않는다. 다른 Event를 clear/drop해서 통과시키지 않는다.
6. route와 NodeOwner를 제거하고 worker 종료·소유 자원 해제를 확인한다. 그 뒤 agent가 보존한
   UNLOAD 성공 결과를 OUTER로 보낸다. 정상 응답 후 INSPECT `nodes`에 해당 ID가 없어야 한다.
7. 이미 transport로 소유권이 넘어간 출력/receipt는 그 소유자가 계속 책임진다. node 제거를
   remote acceptance/KV 정산의 증거로 쓰거나 transport 기록까지 일괄 삭제하지 않는다.

typed 수명 통지는 중립 `RetainedNodeAdapter`/event-node 경계에 두고 요청 identity·결과·자원 상태를
결속한다. 모델별 JSON을 agent가 해석하지 않는다. 새로운 통지 경로 역시 기존 count/byte reservation을
소비하며, 별도 무제한 channel·원본 Event 복제로 completion 보존을 우회하지 않는다.
HF의 기존 명시적 abort는 실패 정리 의미를 유지한다. abort 결과만으로 정상 UNLOAD 성공을 만들지 않는다.

## 7. 구현 순서와 단계 산출물

**2026-09-16 M0 완료:** [호출 경로·소유권 감사](../tests/reports/node-load-lifecycle/20260916_014500.md)에
현재 control/broker/retained adapter/OUTER 호출자와 NL01–NL14를 매핑했다. M1의 첫 구현은 agent control을
막지 않는 node별 supervisor, agent 소유 terminal result, snapshot과 분리된 typed lifecycle completion,
bounded 반환 비용을 함께 세운다. B5에서 발견한 bind 실패 뒤 stdin join과 failed LOAD 회수는
NL05/NL08 fixture로 재사용한다.

이 순서는 이 변경 내부의 작업 순서다. 전체 로드맵의 다른 작업을 임의로 재정렬하지 않는다.

| 단계 | 작업 | 다음 단계 조건 |
| --- | --- | --- |
| M0 | **DONE** — 최신 HEAD/dirty 감사, 실제 호출자 전수 검색, §8 반례와 수명 소유권 설계 | [M0 보고](../tests/reports/node-load-lifecycle/20260916_014500.md)에 중복·실패·완료 응답·barrier·byte 소유권 매핑 |
| M1 | 공통 요청/결과 codec·typed adapter 완료·agent supervisor 설계 구현, neutral fixture | 단일 LOAD/UNLOAD 실제 event 경로와 경계·거부 무효과 시험 통과 |
| M2 | llama.cpp/HF 실제 worker 연결, profile/retention 통합, 완료/실패 제거 | 두 adapter에서 busy·cleanup 실패·응답 포화·정상 제거 통과 |
| M3 | Rust/HF OUTER·실기 스크립트 이관, CREATE/DELETE 및 직접 우회 제거 | 새 명령만으로 생성→정상 응답→해제, 구형 명령 부작용 없는 거부 |
| M4 | 필수 회귀·독립 제거 변이·두 adapter 실기, 소유 문서 갱신 | 최종 소스 결속·시험별 판정·미수용 항목 기록 |

각 단계의 복원 가능한 지점에서 저장소 커밋 규칙을 따른다. 다른 작성자를 멈추고 전체 비무시 변경을
감사한 뒤 자신이 소유하는 일관된 checkout을 커밋한다. 무관한 병행 변경을 임의로 포함하지 않는다.
최종 결과에는 commit·정확한 명령·시험 ID·exit code·최초 실패·남은 작업·다음 첫 행동을 남긴다.

## 8. 필수 반례와 수용 시험 — 모두 예정

시험 추가 시 `node_load_lifecycle` 이름으로 검색 가능하게 한다. pure 함수 시험만으로 아래 실제 경로를
대체하지 않는다. 기본 필수 시험을 새 feature 뒤로 숨기지 않는다.

| ID | 입력 / 실제 경로 | 필수 판정 |
| --- | --- | --- |
| NL01 | 실제 agent TCP LOAD, neutral adapter와 각 실제 adapter | CREATE 없이 loading 등록·적재 완료·INSPECT, native 적재 1회 |
| NL02 | 같은 ID의 동시 LOAD, 서로 다른 generation/plan 포함, loading/loaded/unloading 각각 | 수락 최대1, 거부 측 spawn/native/route/기존 원장·claim·출력 변화0 |
| NL03 | 같은 장치·같은 모델, 서로 다른 ID의 두 LOAD | 자원이 충분한 선언 구성에서 두 node 허용, 한 UNLOAD가 다른 node에 영향0 |
| NL04 | 잘못된 metadata/kind/disabled HF/길이/용량/profile, exact 및 ±1 | native 전 거부, 새 빈 node/예약 누수0, unknown 일반 Event 자동 생성0 |
| NL05 | 실제 LOAD 초기화 실패·부분 child 시작·cleanup 실패 | 확인된 회수면 nodes에서 제거, 불명이면 격리·first/cleanup 오류 보존, 타 node 불변 |
| NL06 | 실제 run-loop의 요청 없는 중간 stage KV·정산 대기·held 출력에서 UNLOAD | busy 거부, native 해제0, 기존 작업/정산 재개 후 같은 요청 정상 완료 |
| NL07 | unloaded 상태 기록 직후 정지, completion cap1·held terminal·OUTER Full/단절 | 완료 응답 전 삭제 경쟁/자기 대기 없음, owner/원본/byte claim 보존, 재개 뒤 성공1회 |
| NL08 | idle UNLOAD의 native cleanup 실패·결과 불명·worker 종료 | 성공 응답0, 이후 일반 실행 차단, 자원 상태와 실패 결과 관측 가능 |
| NL09 | 성공 UNLOAD 뒤 INSPECT와 ID 재사용, 구 generation 지연 LOAD/UNLOAD/추론 | node/worker/소유 listener 제거, 새 generation만 수락, 새 인스턴스에 과거 효과0 |
| NL10 | 한 agent의 느린 LOAD와 다른 node INSPECT/UNLOAD/reconcile | 긴 모델 적재가 공통 제어 루프를 막지 않음 |
| NL11 | 실제 OUTER 경유 응답, source/causation/return route 오염·부분 프레임·receipt 유실 | 정확한 요청별 결과만 수락, uncertain 보존, 전송 ACK를 수명 성공으로 오인하지 않음 |
| NL12 | 둘 이상의 개별 LOAD 중 하나 거부, 성공 node를 OUTER fixture가 UNLOAD | P4의 자동 타 node 회수0, OUTER 명령으로만 전체 회수, 실패/회수 결과 구분 |
| NL13 | 현재 Rust/HF 호출자 및 새 protocol에 구 CREATE/DELETE/direct node LOAD 투입 | 정상 경로 CREATE/DELETE0, 우회 명령 거부 무효과, 두 adapter 정상 추론/해제 |
| NL14 | 반복 LOAD/UNLOAD 및 실패 응답 포화 | active node/worker/claim 잔량0, 새 lifecycle 저장소 count/byte 상한 준수, 기존 세대 이력은 별도 계수 |

NL14에서 기존 `node_generations`가 과거 ID를 유지한다는 사실을 숨기지 않는다. 이번 변경으로
무제한 수명 결과 저장소를 추가하지 않는다. 기존 generation 검증을 없애 이력을 줄이는 수정은 하지 않는다.

독립 변이 최소 항목: 중복 검사 제거/등록 뒤 검사(NL02), snapshot만으로 조기 삭제(NL07),
busy 보호 제거(NL06), cleanup 오류를 성공 처리(NL08), held 결과 drop(NL07),
generation/응답 identity 검증 제거(NL09/NL11). 매 변이는 독립 복사본과 별도 target에서 실제 재컴파일하고
baseline source/binary/hash를 기록한다. 사용자 checkout을 변이 후 복원하는 방식은 금지한다.

## 9. 검증 실행과 보고

아래는 향후 구현 검증 명령이다. 이 계획 작성 시 실행한 결과가 아니다. 먼저 실제 도구 경로와
HF fixture interpreter를 확인한다. 로컬 빌드 제약과 원격 실기 자원 사용은 현재 로드맵을 따른다.
아래 Rust 명령은 동시에 실행하지 않는다.

```powershell
cargo test --locked -p p4-agent -p p4-agent-core -p p4-adapter -p p4-protocol node_load_lifecycle
cargo test --locked -p p4-llamacpp-staged-adapter -p p4-hf-adapter -p p4-event-drive
cargo test --locked --workspace --no-fail-fast
cargo test --locked --workspace --no-fail-fast --features hf-transformers
python layers/adapters/hf/scripts/testing/run.py
node tools/scripts/docs-lint.mjs --all
git diff --check
```

- 표적 필터가 0개 실행이면 PASS로 처리하지 않는다. feature on 실제 entrypoint의 NL01/NL13도 확인한다.
- 새 수명 API로 소형 llama.cpp/HF 정상 응답·취소/해제·재적재를 검증한다. HF의 기존 source/return-route,
  topology/handshake, native의 load identity와 resource profile 회귀를 포함한다.
- 허용된 동일 GPU에 작은 두 적재가 가능한 환경이면 NL03을 실제 모델로 확인한다. 필요한 환경이 없으면
  neutral/로컬 결과와 실제 모델 BLOCKED를 구분한다. GPU당 한 노드 제한으로 시험을 바꾸지 않는다.
- 원격 실행이 승인된 환경에서는 2물리 agent를 경유한 LOAD/UNLOAD와 NL12를 실행한다. 사용자가
  허용한 자원/namespace만 사용한다. 장문/8wave/H0–H7 성능 수용은 현 로드맵의 별도 gate다.
- `tests/plans/node-load-lifecycle-<date>.md`와 `tests/reports/node-load-lifecycle/<timestamp>.md`에
  실제 환경·source/binary/model·시험 ID/명령/exit·passed/failed/ignored/미실행·원본 증거 경로를 남긴다.
  새 파일은 README와 문서 안내도에 등록한다. 실패 뒤 기대값·상한·입력을 완화하지 않는다.

## 10. 문서 이관과 최종 체크

- [event 계약](event-protocol-v2.md): 외부 CREATE/DELETE를 새 LOAD/UNLOAD·결과 규격으로 교체.
- [격리 계약](layer-isolation-contract.md): 중립 수명 통지와 OUTER 전체 적재 책임 명시.
- [검증 규약](distributed-batching-verification.md): HF-REGISTER/HF-LIFE의 명령 흐름을 갱신하되
  기존 거부 무효과·retention·child 회수·generation 검증 조건을 유지.
- [배치 계약](adapter-batching-layers.md): UNLOAD의 로컬 정지점과 node 최종 제거 단계 연결.
- 실행 README/도구 문서·HF 현재 문서를 갱신하고, 역사 보고의 CREATE/DELETE 기록은 그대로 보존.
- README/문서 안내도 등록 및 로드맵 진행 상태 갱신. 이 계획의 단계를 실제 결과 없이 완료로 표시하지 않음.
- 최종 구현에 외부 CREATE/DELETE가 필요하지 않고, 노드가 없는 상태의 LOAD에서 시작해 UNLOAD 후
  node/native 자원이 사라지며, 정확한 최종 결과를 OUTER가 받는지를 실제 경로로 확인.
- OUTER 전체 실패 자동 회수가 별도 미구현이면 명시한다. 그것을 P4 완료 조건에 몰래 포함하거나
  P4가 대신 구현한 것으로 설명하지 않는다.

## 새 세션에 전달할 실행 요청

> `F:\dev\p4`에서 `docs/node-load-lifecycle-plan.md`를 끝까지 읽고 구현하라.
> 노드는 모델 적재 인스턴스이며 외부 LOAD/UNLOAD만으로 생성·제거한다. 사용 중 ID는 거부한다.
> 다중 노드 전체 성공 판정과 실패 시 회수는 OUTER 책임이다. 먼저 현재 HEAD와 병행 변경을 감사하고,
> 로드맵·검증·격리 계약을 지키며 계획의 실제 소비 반례와 독립 변이로 검증하라.
> 문서의 기준 코드가 바뀌었다면 최신 보존/비용 계약을 유지하고 수정 경로를 갱신하라.
