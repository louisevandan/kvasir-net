# Release A 전송 불명 결과 정산·재연결 계획

2026-09-15. 기준 `94c796c1e`. 범위는 P4 event transport의 한 hop 전달이며 adapter payload,
native 완료, request/KV 정산 권한은 바꾸지 않는다. [FINISH 수용](../reports/release-a/20260915_142237.md)
다음 게이트이고, Qwen122B 전체 topology 실행 전에 닫는다.

## 실행 전 확정한 결론

현재 socket write 완료는 원격 broker 수용 증거가 아니다. partial write의 현재 원본과 뒤의 미시작 queue는
agent 메모리에 보존되지만 외부 조회·해소 API가 없다. 원격 duplicate window도 receipt 조회나 outstanding
pin을 제공하지 않는다. 따라서 재접속 뒤 자동 재송신은 중복 native 효과를 만들 수 있고, receipt 없음은
미수신과 eviction을 구분하지 못한다. 단순 peer-cache 삭제나 agent 재시작은 수용 후보가 아니다.

첫 구현 후보는 다음 상태와 권한을 한 번에 갖춘다.

| 상태 | 판정과 허용 동작 |
| --- | --- |
| `rejected_local` | encode·version·capability가 socket write 전에 거부됐다. 원본과 무효과를 보존하고 재전송하지 않는다. |
| `not_started` | connect·route 실패로 socket을 건드리지 않았다. 같은 원본 allocation·event ID·digest를 명시 재연결 뒤 다시 전달할 수 있다. |
| `uncertain` | prefix/body/flush를 시작한 뒤 결과를 모른다. 자동 재송신하지 않고 remote hop receipt를 조회한다. |
| `accepted_exact` | remote가 같은 event ID와 canonical bytes를 broker에 commit했고 receipt가 pin돼 있다. local 원본만 retire하고 다음 queue를 진행한다. |
| `conflict` | 같은 ID의 다른 bytes가 관측됐다. 실패 원본과 증거를 보존하고 해당 연결 세대를 quarantine한다. |
| `unknown` | receipt 부재·기한 초과·peer 불가·증명 horizon 밖이다. 성공/미수신으로 추정하지 않고 quarantine한다. |

## 구현 경계

1. protocol은 connection generation, delivery attempt ID, event ID, canonical event digest와 receipt 상태를
   버전화한다. capability 협상 없는 구형 peer에는 새 동작을 보내지 않고 기존 fail-closed를 유지한다.
2. receiver는 broker commit과 hop receipt 생성을 결속한다. outstanding receipt의 count/bytes/horizon을
   별도 계산하고 공간이 없으면 event commit 전에 거부한다. eviction은 미정산 receipt를 제거하지 않는다.
   sender가 local 원본을 retire한 뒤 보내는 receipt ACK만 pin을 해제하며, ACK 중복·유실은 idempotent하다.
3. sender는 실패 ID별로 현재 원본·미시작 queue·started 여부·target·digest를 소유한다. INSPECT는 payload를
   노출하지 않고 상태별 수, byte 수, 가장 오래된 시각과 failure ID를 제공한다.
4. reconcile은 exact receipt만 `accepted_exact`으로 바꾼다. `not_started` 재전달과 exact 뒤 queue 재개는
   원래 순서를 유지한다. conflict/unknown 뒤 새 세대는 기존 세대를 격리한 후에만 시작한다.
5. Rust event-drive와 HF Python client도 OUTER 수신 receipt를 같은 의미로 소비한다. FINISH ACK, request
   terminal, release, receipt/KV 정산은 서로 대체하지 않는다.

## 첫 완전 gate

| ID | 실패 주입과 통과 조건 |
| --- | --- |
| R1 | encode/version 거부는 원장·remote 효과와 재전송0. connect/route 실패는 원본·비용을 보존하고 명시 재연결 뒤 정확히1회 commit |
| R2 | prefix 일부/body 일부/flush 뒤 reset: 모두 uncertain. 재연결만으로 재송신0 |
| R3 | remote commit 뒤 hop receipt 유실: exact 조회로 local 원본만 retire, native/queue 효과 추가0 |
| R4 | 같은 ID·다른 bytes, receipt absent, receipt horizon 초과: conflict/unknown quarantine, 성공 승격0 |
| R5 | receipt count/bytes 경계±1과 data queue 포화: 거부 전후 원장·예약·credit·출력 효과 동일, reconcile 제어 진행 |
| R6 | uncertain predecessor 뒤 정상 event: 선행 exact 또는 quarantine 전 추월0, 해소 뒤 원 순서 유지 |
| R7 | 구형 peer/잘못된 version·generation·attempt/digest: 실행 전 거부, fallback replay0 |
| R8 | 실제 TCP OUTER 및 agent↔agent, Rust/HF client, llama.cpp/HF 정상 생성·취소·회수·재수용 |
| R9 | 두 물리 host 단절·late receipt·재연결 후 다음 정상 wave. source/binary와 failure/receipt bytes 봉인 |

첫 실행은 R1–R9 구현, 독립 oracle과 제거 변이, docs gate, feature off/on
`cargo test --workspace --no-fail-fast`, HF Python 전체, 양쪽 adapter 실제 경로를 모두 포함한다.
부분 crate 통과는 `first_pass`가 아니다. 2회차는 첫 gate가 드러낸 한정 차이만 수정하고 3회차는 기능 변경
없이 clean rebuild·전체 회귀·독립 worktree 재컴파일 변이로 확인한다.

## 완료와 다음 단계

완료는 실패 원본·receipt·예약 byte가 상태별 상한으로 설명되고, uncertain을 성공이나 미수신으로 추정하지
않으면서 다음 정상 wave를 수용한 경우다. 그 뒤에만 Qwen122B 전체 topology PLAN→LOAD→정상 응답→
요청별 deadline·8wave·취소/회수/재수용을 시작한다.
