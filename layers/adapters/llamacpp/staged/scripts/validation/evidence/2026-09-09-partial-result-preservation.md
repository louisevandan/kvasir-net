# 2026-09-09 — 실패한 실행이 자기 부분 결과를 잃던 결함

종류: 결함 원인 확정·수정·실제 소비 경로 시험·변이 검증. 성능 증거가 아니다.
기준 HEAD `352bdb092`, 작업 트리 clean에서 시작했다.
현재 작업 순서는 [로드맵](../../../../../../../docs/distributed-batching-roadmap.md)이 소유한다.

## 증상

2026-09-09 포화 실험에서 4-stage 실패 4회가 **`artifact.json`을 하나도 남기지 않았다.**
남은 것은 `drive.stderr.log`의 한 줄뿐이다.

```
inference observation evidence Missing { requests: 256, stage_executions: 8 };
receive failed: 연결이 원격 호스트에 의해 강제로 끊겼습니다. (os error 10054)
```

그 실행은 출력 583건을 전달받은 뒤였다. 즉 승인된 출력·완료·관측이 실제로 있었는데,
**최초 원인(관측 증거 누락인지, 스트림 중단인지, 10054인지)을 구분할 자료가 남지 않았다.**
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
- 실패 실행의 요청별 행 수는 0으로 남는다. `apply_counts`는 증거가 완전할 때만 허용되며,
  `evidence_missing`이 그 이유를 말한다. **유효했던 것은 보존하되 없는 것을 만들지 않는다.**

거절 자체는 완화하지 않았다. 잘못된 이벤트는 여전히 거절되고 실행은 실패로 남으며,
`main.rs`는 `passed=false`에서 종료 코드 1로 끝난다.

## 시험

`consumer_budget_boundary_tests`의 실제 소비 경로(진짜 `EventWire`, 실제 PREFILL 제출을 검증하는
peer)에 고장을 주입한다. 주입 지점은 **모든 OUTPUT이 전달된 뒤 첫 RELEASE 이전** — 09-09 실패가
가졌던 모양(완료는 있고 해제는 0)이다.

| 시험 | 주입 | 고정하는 것 |
| --- | --- | --- |
| `a_cut_connection_after_the_outputs_keeps_them` | peer가 연결을 끊는다 | `receive failed`가 최초 오류이고 완료 2건·해제 0건·응답·관측이 남는다 |
| `an_expired_deadline_after_the_outputs_keeps_them` | peer가 멈춰 실행 자신의 deadline이 만료된다 | `overall deadline expired`로 보고되며 같은 것이 남는다 |
| `an_invalid_event_after_the_outputs_keeps_them_without_accepting_it` | peer가 알 수 없는 content type을 보낸다 | 거절이 최초 오류이고, **거절된 이벤트는 받아들여지지 않는다**(정상 실행보다 증거가 적고 span은 0) |
| `a_refused_unload_after_a_broken_run_reports_both_and_still_fails` | 위 절단 실행에 UNLOAD 거부를 얹는다 | `error`와 `cleanup_error`가 각자 남고, artifact가 생성되며, `submissions`가 정확하고, `passed=false` |

전체 집계 `cargo test --workspace --no-fail-fast --locked`: **1,371 passed / 0 failed / 7 ignored**
(직전 1,367에서 새 시험 4개 증가).

기존 boundary·output-contract 시험 89개는 계약 변경에 맞춰 거절을 `run.error`에서 읽도록 고쳤다.
거절 대상과 문구는 그대로이며 완화하지 않았다.

## 변이 검증

분리된 detached worktree(`F:/dev/p4-mutation-partial`, 자체 `target/`)에서 수행했고 회차마다
재컴파일을 sha256으로 확인했다.

| 회차 | 변이 | 시험 바이너리 sha256 | 결과 |
| --- | --- | --- | --- |
| baseline | 없음 | `5470617d301f82c8…8d833751` | 89 passed / 0 failed |
| M1 | `outcome?;` — 수정 이전처럼 루프의 오류를 전파 | `73e4c7ad9cffd87c…ef4dbc25` | 84 passed / **5 failed** |

M1 실패: 위 새 시험 4개 전부와
`a_future_wave_cannot_extend_the_overall_evidence_deadline_or_send_after_it`.

## 남은 것

- **09-09의 10054는 최초 원인이 미확정이다.** 이 수정은 다음에 같은 일이 일어나면 판정할 자료가
  남게 할 뿐이다. 재시도 성공으로 해결 처리하지 않는다.
- CLI 종료 코드는 `main.rs`가 `passed=false`에서 1로 끝나는 단일 분기이며, 이 시험은 `passed=false`
  까지 고정한다. 프로세스를 띄워 종료 코드를 확인하는 시험은 아직 없다.
- 다음은 계획용 recurrent 할당으로 가용량이 축소되는 staged 서버 결함이다.
  [근거](2026-09-09-saturation-and-utilisation.md)
