# 2026-09-09 — 성능 측정을 못 읽게 만들던 세 결함과 철회된 근거의 정정

종류: 결함 재현·수정·검증, 그리고 문서 정정. **성능 증거가 아니다.**
새 추론 측정을 포함하지 않는다. 현재 작업 순서는
[로드맵](../../../../../../../docs/distributed-batching-roadmap.md)이 소유한다.

이 문서가 다루는 것은 로드맵 §0.7 P-2의 1·2·4번이다. 3번(`pressure` 재판정)은 여기 없다.

## 왜 이것들을 먼저 고쳤는가

세 결함은 서로 무관해 보이지만 결과가 같다. **성능 실험을 돌려도 결과를 읽을 수 없게 만든다.**

## 1. 인용해 온 상관관계는 원문이 이미 철회한 것이었다

`worker/drive.rs`의 발행 폭 주석은 처리량이 stage 겹침과 r=0.891로 함께 움직이고 폭과는
r=-0.357로 반대로 움직인다고 적고 있었다. 로드맵 §0.7의 최초 판(`daae4c8fd`)이 이를 폭 정책의
근거로 인용했다.

그 주석의 원자료인 [09-03 기록](2026-09-03-load-and-batching.md)은 그 표 바로 아래에서
`Everything in the paragraph above is backwards. See the next section.`이라고 적고, 다음 절
제목이 `The correlation was reverse causation (2026-09-04)`다.

| 원문이 보존한 대조 실험 | 값 |
| --- | --- |
| `P4_STAGED_MAX_ISSUE_ROWS`로 폭을 제한한 교차 실행 | 8회 (cap 0/24/0/24/0/12/0/48) |
| total row/s | 544.25 → 198.10 |
| 그때 stage 겹침 | 81.1% → **95.4%** (GPU 사용률도 최고) |
| 폭을 통제한 뒤 상관 | 폭 **+0.898**, 겹침 **-0.060** |
| 적합 | **tail step = batch당 34.2 ms + 행당 1.051 ms** |

즉 겹침이 오른 것은 원인이 아니라 결과다. 바쁜 stage는 고정비를 반복해 내느라 바빴다.
같은 기록의 후속 절이 그 고정비를 이미 분해해 두었다: tail에서 `llama_decode` 40.1 ms보다
**sampling 46.9 ms가 크고**, 행당으로는 transformer 0.11 ms 대 sampler 0.29 ms다.

**조치.** `drive.rs`의 주석을 09-04 결과로 교체했다(`2bc8f93cd`, 주석만 변경, `cargo check --lib`
통과). 로드맵 §0.7을 다시 썼다(`2bc8f93cd`). 코드에 남은 철회된 인용은 없다.

동시에 §0.7의 다른 세 서술도 정정했다. 서로 다른 집합을 곱한 값을 항등식이라고 부른 것,
RPC 구간 겹침(31.6%)을 GPU 동시 계산으로 읽은 것, 의도된 정책의 결과(prefill·decode 혼합 0건)를
결함으로 보고 그 위에서 191 TPS를 기대 이득으로 계산한 것이다. 근거는 각각
`run.mjs:118`의 자체 주석("These are service spans, not device time")과
`scheduler.rs:280`의 `plan_equal_ordinary` 정책이다.

## 2. 정리 실패가 실행 자신의 실패를 지우고 있었다

### 증상

`pressure`(resident 256)는 두 호스트 모두 UNLOAD `unload is busy; active_owners=224/256`으로
끝났다. 그런데 **산출물 파일이 아예 없어서** 해제 누수인지 추론이 먼저 깨진 뒤의 정상 거부인지
판정할 수 없었다.

### 원인

`run/inference.rs`의 ERROR 분기는 실패를 `Ok(InferenceResult{ error: Some(..) })`로 돌려준다.
그런데 `execute`는 그 뒤 UNLOAD 응답을 `receive_exact(...).await?`로 받았다. 추론이 깨진 뒤에는
노드가 아직 owner를 쥐고 있어 UNLOAD가 거부되고, 그 `?`가 **`InferenceResult` 전체를 버렸다.**
최초 오류, 요청, 출력, 관측, 정산 기록이 함께 사라진다. 호출자(`main.rs`)는 `Ok`일 때만 산출물을
쓰므로 파일이 남지 않는다.

### 수정

`teardown`을 분리하고 반환형을 **`Result`가 아니라 `Option<String>`**으로 두었다.
전파할 `Result`가 없으므로 호출부에 `?`를 쓸 수 없다.

- 원래 결함을 그대로 되돌려 쓰면 **컴파일 오류**다. 확인:
  `error[E0277]: the ? operator can only be used on Results, not Options`.
- `error`(실행 자신의 최초 실패)와 `cleanup_error`(UNLOAD/DELETE 실패)를 분리했고,
  산출물은 항상 조립된다.
- **UNLOAD guard는 완화하지 않았다.** 정리 실패는 여전히 실행을 실패시킨다.

### 검증

`tools/event-drive/src/run/teardown_preserves_failure_tests.rs`. 실제 소비 경로
(`inference::drive`)를 노드가 보고한 ERROR까지 몰고 간 뒤 산출물을 조립한다. 네이티브·모델·전송
fixture가 아니다.

| 변이 | 결과 |
| --- | --- |
| 두 오류 필드를 합침(`cleanup_error.or(run.error)`) | 시험 2개 실패 |
| 판정에서 `cleanup_error.is_none()` 제거 | 시험 1개 실패 (양성 대조가 있는 것) |
| 호출부에 `?`를 되돌림 | **어떤 시험도 잡지 못한다. 그래서 타입으로 막았다** |

마지막 줄이 이 수정의 요점이다. 시험은 `assemble`이 무엇을 하는지 고정할 수 있지만
`assemble`에 **도달한다는 것**은 고정하지 못한다.

## 3. 신호로 종료한 자식을 정지 실패로 판정하고 있었다

`run.mjs::stopChild`는 `child.kill()` 뒤 `child.exitCode !== null`을 물었다. 신호로 죽은
프로세스는 `exitCode`가 null이고 `signalCode`가 설정된다. 이 환경에서 재현:

```text
exit event: code=null signal=SIGTERM
child.exitCode=null child.signalCode=SIGTERM
old predicate (exitCode !== null) -> false
```

즉 **모든 정상 정지가 정지 실패로 보고됐다.** `run.mjs:622`가 `!agentStopped`로 실행 전체를
실패시키므로, 그밖에 성공한 실행도 마지막에 실패로 뒤집혔다. 2026-09-07 기록에서 `agent_stopped`를
"멈춘 에이전트"로 읽은 것은 이 때문이며, 그 해석은 틀렸다.

수정은 종료 또는 신호 중 하나면 정지로 본다. 시험 4개
(`test/benchmarks/p4-4node/stop-child.test.mjs`, 실제 자식 프로세스 사용). 예전 술어로 되돌리는
변이는 첫 시험이 잡는다.

## 4. 시험 컴파일 복구

`issue_witness_tests.rs:409`가 `Deref`만 구현한 `RequestState`를 통해 대입해 E0594로 실패했고,
lib test 타깃 전체가 무너져 `cargo test --workspace`가 종료 101·실행 시험 0이었다.
`input_mut_for_test()`로 복사를 명시했다. `DerefMut`은 여전히 없고 제품 경로는 바뀌지 않았다.

## 5. `pressure` 재판정 — 미판정이던 원인이 확정됐다

수정 직후 같은 시나리오를 돌렸고, **고친 것이 곧바로 답을 내놓았다.**

실행 `20260908T194141Z-410fb9bb` (`target/pressure-20260909/`).
호스트는 **로컬 RTX 3090 + RTX 4080**이다. 보존된 3090×2 실행과 **다른 하드웨어**이므로 처리량 비교에
쓰지 않는다. 여기서 판정하는 것은 제어 평면 상태이지 성능이 아니다.
빌드 `0eadefebd3` + patch set `961bd89cd119`(0025 포함), stage 4개, 시퀀스 정원 256, 요청 512.

### 이번에는 산출물이 남았다

예전에는 UNLOAD 거부가 전파되어 산출물 파일 자체가 없었다. 이번에는 두 오류가 분리돼 기록됐다.

| 필드 | 값 |
| --- | --- |
| `error` | `LLAMA_ADAPTER_EVENT_REJECTED` / **`stage control batch total receipt budget is exhausted`** |
| `cleanup_error` | `unload is busy;work={...}` |
| `request_count` / `completed_count` / `released_count` | 512 / 96 / **0** |

UNLOAD 거부가 함께 실은 작업 스냅샷이 결정적이다.

```json
{"requests":0,"pending":0,"pending_releases":0,"pending_settlements":0,
 "flight_batches":0,"flight_executions":0,"open_batch_view":0,"effects":0,
 "active_owners":256,"active_frontiers":256}
```

**비행 중인 작업이 하나도 없다.** owner와 frontier 256개만 남아 있다.

### 원인 사슬

1. `validate_control_batch`의 제품 호출자는 `worker/release.rs:324`와 `worker/settlement.rs:246`
   **둘뿐이다.** 즉 예산에 걸린 것은 **해제·정산 경로 자신**이다.
2. 그래서 96개 요청이 `release_member`와 `issued_work`를 받고도 `released`는 **0개**다.
   해제가 실행된 적이 없다.
3. 해제가 없으므로 owner·frontier 슬롯이 계속 점유된다: 256/256.
4. `require_idle_unload`는 owner가 0이 아니면 거부한다. 그 함수의 주석이 스스로 밝히듯 이것은
   **"explicit, healthy-worker UNLOAD"의 preflight**이며 실패 정리는 별도 경로다.

**판정: UNLOAD 거부는 원인이 아니라 결과다.** 해제되지 않은 상태가 방치돼 새는 것이 아니라,
**해제 자체가 용량 한계에 막혀 시작되지 못했다.**

### 그 용량 한계

`ownership.rs`의 두 상수다.

| 상수 | 값 | 쓰임 |
| --- | ---: | --- |
| `MAX_CONTROL_BYTES` | 1 MiB | 제어 1건의 **최대 응답**을 native 실행 전에 예약 |
| `MAX_RECEIPT_BYTES` | 64 MiB | 누적 receipt 총량 상한 |

행마다 실제 크기가 아니라 최악 응답 1 MiB를 예약하므로, 누적분이 0이어도 **제어 batch는 약 64행에서
상한에 닿는다.** 시퀀스 정원 256에서 전폭 해제·정산은 구조적으로 이 한계를 넘는다.

행 단위 거부 자체는 의도된 설계이고 시험도 있다(`ownership.rs:763`, `stage_tests.rs:1608`:
어떤 native 효과보다 먼저 거부하고 아무것도 소비하지 않는다). 문제는 거부 동작이 아니라
**두 상수의 조합이 resident 256과 양립하지 않는다는 것**이다.

### 아직 확정하지 않은 것

- 거부된 해제 batch의 **실제 폭**은 이 산출물에 기록되지 않는다. 64행 상한은 상수에서 계산한 예측이며,
  관측된 폭으로 확인하지 않았다.
- 보존된 3090×2 실행(`active_owners=224/256`)이 **같은 원인**이었는지는 증명되지 않았다.
  그 실행들은 최초 오류를 남기지 못했다.
- 이 실행은 512 요청 중 96개만 완료한 채 131.4 s에 중단됐다. 처리량 수치를 인용하지 않는다.
- 따라서 §0.5 3번(수용·비행 budget)은 resident 상향의 선행조건으로 남는다. 예산을 세우기 전에
  resident를 올리면 같은 벽에 다시 닿는다.

## 종합 결과

| 항목 | 값 |
| --- | ---: |
| `cargo test --workspace --no-fail-fast --locked` | **1,364 passed / 0 failed / 7 ignored** |
| 그중 staged adapter lib | 512 |
| 그중 `p4-event-drive` | 85 |
| 하네스 `*.test.mjs` (파일별 실행) | 76 |

하네스는 파일별로 실행했다. 이 기기의 Node 26에서 `node --test <디렉터리>`는 디렉터리를 모듈로
해석해 `MODULE_NOT_FOUND`로 실패한다. 이번 변경과 무관하며 수정 전에도 같다.

## 남은 것

- **`pressure` 재판정은 하지 않았다.** 선행 결함은 해소됐고 이제 실패해도 산출물이 남지만,
  판정 자체는 GPU 호스트에서 실행해야 한다. 원격 3090×2(`m42-server2`)는 SSH로 접속되나
  로그온 세션이 없어(`explorer` 0개) 대화형 예약 작업을 띄울 수 없다. 매핑 드라이브의 모델을
  여는 경로가 그 세션에 묶여 있다.
- 이 문서는 어떤 처리량·GPU 사용률도 새로 주장하지 않는다. 34.2 ms·46.9 ms 등 인용한 값은
  전부 09-03/09-04 기록의 것이며 2B 실행에서 나왔다. 35B의 고정비 내역은 아직 측정되지 않았다.
- 계측을 새로 만들 필요는 없다. 타이머는 `server_physical.cpp:34`, 스위치는
  `remote-agent.mjs:75`의 `--step-trace`에 이미 있다.
