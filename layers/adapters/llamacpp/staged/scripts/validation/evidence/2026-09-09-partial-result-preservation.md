# 2026-09-09 — 실패한 실행이 자기 부분 결과를 잃던 결함

종류: 결함 원인 확정·수정·실제 소비 경로 시험·변이 검증. 성능 증거가 아니다.
기준 HEAD `352bdb092`, 작업 트리 clean에서 시작했다.
현재 작업 순서는 [로드맵](../../../../../../../docs/distributed-batching-roadmap.md)이 소유한다.

## 증상

2026-09-09 포화 실험에서 **실패한 실행 4회가 `artifact.json`을 하나도 남기지 않았다.**
그 4회는 같은 종류가 아니다.

| 실행 | drive 오류 | 분류 |
| --- | --- | --- |
| `20260909T055821Z-151f5b02` | `node event error … node-0` | 2-stage **적재** 거부 |
| `20260909T065649Z-e6c5c4c4` | `node event error … node-0` | 2-stage **적재** 거부 |
| `20260909T060144Z-0a42617d` | `node already exists` | 노드 중복 |
| `20260909T061603Z-6cadb520` | `inference observation evidence Missing { requests: 256, stage_executions: 8 }; receive failed: event stream ended mid-frame` | 4-stage **추론** 중단 |

추론이 중단된 것은 마지막 하나뿐이고, **그 drive 오류는 `event stream ended mid-frame`이다.**
`os error 10054`는 같은 실행 agent 로그의 **별개 관측**(`P4_EVENT_CONNECTION_STOPPED`)이며
drive가 본 것이 아니다.

산출물의 `delivery.counted = 583`도 수신한 OUTPUT 수가 아니다.
[`delivery.mjs`](../../../../../../../test/benchmarks/p4-4node/delivery.mjs)의 `counted`는
`P4_EVENT_OUTER_MISSING discarded=`의 최대 누계, 즉 **OUTER로 전달되지 못하고 폐기된 이벤트 수**다.
따라서 **그 실행이 어디까지 승인했는지는 지금도 알 수 없다.** 폐기 수만으로 단절의 최초 원인을
확정해서도 안 된다.

바로 그것이 이 결함이다. 추론이 시작된 뒤의 실패에서 승인된 것이 남지 않으니,
**최초 원인을 관측 증거 누락·스트림 중단·전송 실패로 구분할 자료가 없다.**
재시도가 성공했다고 해서 닫힌 문제가 아니다.

## 원인

[`inference.rs`](../../../../../../../tools/event-drive/src/run/inference.rs)의 `drive`는
요청·출력·완료·해제·관측·stage span을 지역 변수에 모으면서, 루프 안의 모든 거절과 전송 실패를
`return Err`/`?`로 내보냈다. 그 순간 지역 변수가 통째로 사라진다.
[`run/mod.rs`](../../../../../../../tools/event-drive/src/run/mod.rs)는 `inference::drive(..).await?`로
받으므로 `assemble`에 닿지 못하고, CLI는 산출물을 쓰지 못한 채 끝난다.

2026-09-09의 앞선 수정은 **teardown 실패**가 실행을 지우지 못하게 했다. 추론 자체의 실패는 그대로였다.

## 수정

추론이 시작된 뒤의 실패는 **결과이지 결과의 부재가 아니다.**

- `drive`의 수신 루프를 async 블록으로 감쌌다. 루프 안의 `?`와 `return Err`는 검사가 있어야 할
  자리에 그대로 두고, 그 오류가 **이 실행의 최초 오류**가 된다. 노드가 이미 오류를 보고했다면
  그것이 유지된다(`get_or_insert_with`).
- 제출 이전 검사(요청 수 범위, `InferenceIdentity::new`)만 여전히 `Err`다. 그때는 모은 것이 없다.
- 해제 수신의 산술을 검사 뒤로 옮겼다. 거절될 이벤트가 `released` 카운트를 올린 채 남지 않는다.
- `RequestArtifact`에 `submission`(`delivered` / `uncertain`)을 넣었다. 쓰기 실패는 도착했을 수도
  있으므로 "미제출"이 아니라 **불명**이며, 식별자는 어느 쪽이든 소비된다.
- `RunArtifact`에 `evidence_missing`과 `submissions` 요약(configured/delivered/uncertain/
  unsubmitted/incomplete/unreleased)을 넣었다. 증거가 오지 않아 멈춘 실행과, 다 받고 무언가를
  거절한 실행은 다른 실패다.
- **귀속은 증거를 따르지 판정을 따르지 않는다.** 관측 증거가 완전하면 실행이 실패했든 해제가
  남았든 `apply_counts`를 적용한다. 증거가 요청별 행 수를 입증하는데 0으로 보고하면 아무도 재지
  않은 값을 적는 것이고, `evidence_missing`이 `null`인데 설명할 0이 남는다.
  따라서 `evidence_missing`이 독자와의 계약이다 — `null`이면 행 수는 귀속된 최종값이고,
  값이 있으면 귀속되지 않은 상태이며 그 0은 일이 없었다는 뜻이 아니라 증거가 없다는 뜻이다.
  **유효했던 것은 보존하되 없는 것을 만들지 않는다.**

거절 자체는 완화하지 않았다. 잘못된 이벤트는 여전히 거절되고 실행은 실패로 남으며,
`main.rs`는 `passed=false`에서 종료 코드 1로 끝난다.

## 시험

`consumer_budget_boundary_tests`의 실제 소비 경로(진짜 `EventWire`, 실제 PREFILL 제출을 검증하는
peer)에 고장을 주입한다. 주입 지점은 **모든 OUTPUT이 전달된 뒤 첫 RELEASE 이전**으로 골랐다.
보존할 것이 있는 지점이면서 세 고장을 같은 자리에서 비교할 수 있기 때문이다.
**09-09 실기 실패가 같은 지점에서 끊겼다는 증거는 없다** — 위에서 적었듯 그 실행이 어디까지
승인했는지는 산출물이 남지 않아 알 수 없다. 이것들은 유효한 합성 반례이지 역사적 재현이 아니다.

| 시험 | 주입 | 고정하는 것 |
| --- | --- | --- |
| `a_cut_connection_after_the_outputs_keeps_them` | peer가 연결을 끊는다 | `receive failed`가 최초 오류이고 완료 2건·해제 0건·응답·관측이 남는다 |
| `an_expired_deadline_after_the_outputs_keeps_them` | peer가 멈춰 실행 자신의 deadline이 만료된다 | `overall deadline expired`로 보고되며 같은 것이 남는다 |
| `an_invalid_event_after_the_outputs_keeps_them_without_accepting_it` | peer가 알 수 없는 content type을 보낸다 | 거절이 최초 오류이고, **거절된 이벤트는 받아들여지지 않는다**(정상 실행보다 증거가 적고 span은 0) |
| `a_refused_unload_after_a_broken_run_reports_both_and_still_fails` | 위 절단 실행에 UNLOAD 거부를 얹는다 | `error`와 `cleanup_error`가 각자 남고, artifact가 생성되며, `submissions`가 정확하고, `passed=false` |
| `complete_evidence_attributes_rows_even_when_the_run_failed_with_a_release_outstanding` | 해제가 거절되지만 증거는 완전한 기존 사례 | 증거가 완전하면 실패·미해제와 무관하게 행 수가 귀속되고, 증거가 없을 때만 0으로 남는다 |
| `a_wave_that_fails_mid_write_separates_delivered_uncertain_and_unsubmitted` | wave 도중 쓰기 실패 | delivered 1 / uncertain 1 / unsubmitted 2로 갈린다 |
| `a_run_that_breaks_and_then_fails_teardown_still_writes_its_artifact_and_exits_nonzero` | 로컬 TCP peer + **실제 CLI 바이너리** | `execute → teardown → JSON 파일 기록 → 종료 코드 1`이 실제로 일어난다 |

전체 집계 `cargo test --workspace --no-fail-fast --locked`: **1,374 passed / 0 failed / 7 ignored**
(직전 1,367에서 새 시험 7개 증가).

기존 boundary·output-contract 시험 89개는 계약 변경에 맞춰 거절을 `run.error`에서 읽도록 고쳤다.
거절 대상과 문구는 그대로이며 완화하지 않았다.

## 변이 검증

분리된 detached worktree(`F:/dev/p4-mutation-partial`, 자체 `target/`)에서 수행했고 회차마다
재컴파일을 sha256으로 확인했다.

| 회차 | 변이 | 시험 바이너리 sha256 | 결과 |
| --- | --- | --- | --- |
| baseline | 없음 | `5470617d301f82c8…8d833751` | 89 passed / 0 failed |
| M1 | `outcome?;` — 수정 이전처럼 루프의 오류를 전파 | `73e4c7ad9cffd87c…ef4dbc25` | 84 passed / **5 failed** |
| M2 | `apply_counts`를 옛 자리(전량 해제 게이트 안)로 되돌림 | `689eaccdcd25f898…1f23e480` | 90 passed / **1 failed** |
| M3 | `apply_counts` 자체를 제거 | `d65a2a5660265453…7ece85d2` | 78 passed / **13 failed** |

M1 실패: 위 새 시험 4개 전부와
`a_future_wave_cannot_extend_the_overall_evidence_deadline_or_send_after_it`.

M2는 새 귀속 시험 **하나만** 실패한다 — 옛 자리는 성공 실행의 집계는 그대로 두고 실패·미해제
실행에서만 0을 남기므로, 그 시험이 정확히 그 회귀를 잡는다. M3는 귀속 자체를 없애 12개 기존
시험까지 함께 무너뜨린다.

## 남은 것

- **09-09의 10054는 최초 원인이 미확정이다.** 이 수정은 다음에 같은 일이 일어나면 판정할 자료가
  남게 할 뿐이다. 재시도 성공으로 해결 처리하지 않는다.
- CLI 종료 코드는 `main.rs`가 `passed=false`에서 1로 끝나는 단일 분기이며, 이 시험은 `passed=false`
  까지 고정한다. 프로세스를 띄워 종료 코드를 확인하는 시험은 아직 없다.
- 다음은 계획용 recurrent 할당으로 가용량이 축소되는 staged 서버 결함이다.
  [근거](2026-09-09-saturation-and-utilisation.md)
