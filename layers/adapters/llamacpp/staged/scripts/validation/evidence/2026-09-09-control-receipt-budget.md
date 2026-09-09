# 2026-09-09 — 제어 응답 예산이 해제·정산을 막던 결함

종류: 결함 원인 확정·수정·소비 경로 시험·변이 검증. **성능 증거가 아니며 `pressure` 실기 재판정은 아직 남아 있다.**
기준 HEAD `249ff9d7a83`, 작업 트리 clean에서 시작했다.
현재 작업 순서는 [로드맵](../../../../../../../docs/distributed-batching-roadmap.md)이 소유한다.
직전 판정과 실행 산출물은 [측정 신뢰 회복 기록](2026-09-09-measurement-trust-recovery.md)에 있다.

## 증상

3090×2(`m42-server2`)에서 `pressure`(512 요청, resident 256)를 두 번 실행했고 둘 다 같은 최초 오류로
끝났다. 완료는 96/512와 64/512, `released_count`는 양쪽 다 0이었다.

```
LLAMA_ADAPTER_EVENT_REJECTED / stage control batch total receipt budget is exhausted
```

## 원인

`ownership.rs`의 예산 검사는 **모든 새 제어에 대해 한 건당 `MAX_CONTROL_BYTES`(1 MiB)를 예약**했다.
누적 상한 `MAX_RECEIPT_BYTES`는 64 MiB이므로, 실제 응답 크기와 무관하게 제어 batch의 폭이 64에 닿으면
거부된다. 두 제품 호출자(`worker/release.rs`, `worker/settlement.rs`)는 한 이벤트의 모든 sequence를
한 batch로 검사하므로, sequence 정원 256에서 전폭 해제·정산은 구조적으로 통과할 수 없었다.

실제 응답은 그렇게 크지 않다.

| 명령 | 응답 계약 | 상한 |
| --- | --- | ---: |
| `PhysicalRelease` | 요청 본문과 **바이트 단위로 동일**해야 하며 아니면 fence된다 (`worker/release.rs`) | 요청 길이 = 67–70 B |
| `PhysicalSettle` | identity prefix echo + `u32` 개수 + 물리 행당 `i32` 하나 (`control_identity::settlement_reply`, `frontier::validate_continuation_width`) | `physical_capacity × 4 + prefix + 4` |

### 거부된 폭 — 이전 기록의 “약 64”를 확정한다

이전 기록은 상수에서 계산한 예측으로 “약 64행”이라고 적고 미확정으로 남겼다. 실제 wire 명령으로
재면 **63**이다. 예산 코드가 아니라 바뀌지 않은 두 상수와 바뀌지 않은 인코더에서 나온다.

`control_identity::prefix`는 `4 + 2 + 2 + 8×3 + 4 + (4+세션) + (4+키)` 바이트다. 세션 `session`(7 B),
키 `session\0request-N`(17–18 B)이면 요청은 68–69 B다. 1 MiB 예약을 함께 세면

- 63건: `63 × 1,048,576 + 4,337 = 66,064,625 B ≤ 67,108,864 B` — 통과
- 64건: `64 × 1,048,576 + 4,406 = 67,113,270 B > 67,108,864 B` — 거부

시험 `a_full_width_control_batch_fits_when_each_command_reserves_its_own_bound`이 같은 폭(63)을
실행으로 확인한다. 다만 **09-09 실행에서 실제로 거부된 명령 종류와 batch 폭은 그 산출물에 기록되지
않았다.** 이 값은 그 실행의 폭이 아니라 상한 회계가 허용하는 폭이다.

## 수정

명령마다 **자기 계약이 증명하는 응답 상한**을 선언하고, 개별 검사와 batch 합계 검사에 같은 값을 쓴다.

- `ownership.rs`에 `ControlBudget { identity, operation_id, request, response_bound }`를 두고
  `validate_control_batch`가 이 목록을 받는다. `check_control`에도 `response_bound` 인자가 생겼다.
- `release.rs`는 `response_bound = body.len()`을 넘긴다. 바로 아래에서 응답 ≠ 요청이면 fence하므로
  이 상한은 그 자리에서 강제된다.
- `settlement.rs`는 이미 계산하던 `physical_capacity × 4 + prefix + 4`를 batch 검사까지 전달한다.
  이전에는 개별 `validate_control_sizes`에만 쓰고 합계 검사에는 1 MiB가 들어갔다.
- `commit_control`은 **도착한 응답의 실제 길이**로 정산한다. 상한은 native 실행 전 예약에만 쓰인다.
  상한보다 긴 응답은 commit 이전에 fence된다(release는 echo 비교, settle은
  `settlement_reply`와 `validate_continuation_width`).
- 거부 문구가 재현에 필요한 값을 싣는다. batch는 `member(s)`, `new`, `retained`, `reserve`, `need`,
  `limit`을, 개별은 `slot`, `retained`, `reclaimed`, `request`, `response bound`, `need`, `limit`을 남긴다.
  호출자는 명령 종류와 폭을 앞에 붙인다(`release of N sequence(s) for session S: …`,
  `settlement of N sequence(s) at physical capacity C: …`).

상한 자체(1 MiB·64 MiB)는 올리지 않았고 resident도 내리지 않았다. 공통 코어의 예산 구조도 건드리지
않았다. 바뀐 것은 어댑터가 예약하는 **크기의 계산**이다.

## 정정 — `released_count=0`의 뜻

이전 기록은 `released_count=0`을 근거로 “해제가 실행된 적이 없다”고 적었다. **그 단정은 틀렸다.**
head는 `worker/effects.rs`의 `CommittedEffect::Release`에서 `release_stage_sequence`로 **자기 stage의
해제를 실제로 수행한 뒤** 다음 stage로 전달한다. 이 경로는 sequence 하나짜리 개별 검사를 쓰므로
batch 합계 검사에 걸리지 않는다. 걸린 것은 그 뒤 stage의 batch 경로다.

따라서 옳은 진술은 **“전체 stage의 해제 완료가 확인되지 않았다”**이며,
“모든 stage에서 해제가 시작되지 않았다”가 아니다. UNLOAD 거부가 실은 작업 스냅샷도
그 노드 하나의 상태다. 어느 stage에서 어디까지 진행했는지는 그 산출물로 판정할 수 없다.

## 시험

새로 추가한 세 개는 모두 실패 반례에서 출발한다.

| 시험 | 위치 | 무엇을 고정하는가 |
| --- | --- | --- |
| `a_full_width_control_batch_fits_when_each_command_reserves_its_own_bound` | `ownership.rs` | 생산 상한(1 MiB/64 MiB)과 실제 wire 명령으로 sequence 정원 256의 전폭 해제·정산이 통과하고, 같은 batch를 상한 회계로 재면 63에서 막힌다 |
| `a_release_batch_its_bounds_afford_is_admitted_at_the_consumption_path` | `stage_tests.rs` | 실제 소비 경로(`fixture.handle`)에서, 각 명령의 상한은 감당하고 1 MiB 회계는 감당하지 못하는 예산으로 전 member가 native 실행되고 재전송이 replay된다 |
| `a_settlement_batch_its_bounds_afford_is_admitted_at_the_consumption_path` | `stage_tests.rs` | 위와 같으며 정산 경로 |

기존 시험 중 두 개는 계약이 바뀌어 함께 고쳤다.

- `aggregate_..._receipt_budget_refuses_before_the_first_native_effect`(해제·정산 두 개):
  거부 판정을 1 MiB가 아니라 **각 명령의 계약 상한 합계**로 다시 쓴다. 거부 자체와 “아무것도
  소비하지 않는다”는 검사는 그대로다.
- `total_receipt_budget_is_checked_before_new_native_controls`: native 실행 전 게이트는 상한으로
  거부하고, `commit_control`은 도착한 응답 길이로만 거부한다는 새 분업을 고정한다.

전체 집계 `cargo test --workspace --no-fail-fast --locked`:
**1,367 passed / 0 failed / 7 ignored** (직전 기록 1,364에서 새 시험 3개 증가, 실패 0 유지).

`cargo clippy --all-targets --locked`는 이번에 고친 네 파일에 대해 경고를 내지 않는다.
`rustfmt --edition 2024`는 `ownership.rs`에 적용했다. 저장소에는 이번 변경과 무관한 미포맷 파일이
남아 있어 `cargo fmt --all`을 돌리지 않았다.

## 변이 검증

사용자 작업 트리가 아니라 분리된 detached worktree(`F:/dev/p4-mutation-receipt`, 자체 `target/`)에서
수행했고 매 회차 실제 재컴파일을 sha256으로 확인했다. 변이는 수정 이전 회계로 되돌리는 것이다.

| 회차 | 변이 | 시험 바이너리 sha256 | 결과 |
| --- | --- | --- | --- |
| baseline | 없음 | `d3400351f7c2486c…5546655c` | 515 passed / 0 failed |
| M1 | 개별 검사만 1 MiB 예약 | `143c17b303c60677…d9efe45a7` | 513 passed / **2 failed** |
| M2 | batch 합계만 1 MiB 예약 | `2906595fee880ada…eb9a173ccb` | 512 passed / **3 failed** |
| M3 | 둘 다 — `249ff9d7a83`의 회계 | `e0dafaa81e0ba318…3405c9ec79` | 511 passed / **4 failed** |

- M1 실패: `a_settlement_batch_..._consumption_path`(첫 sequence를 native 실행한 뒤 두 번째에서 거부),
  `total_receipt_budget_is_checked_before_new_native_controls`.
  해제 arm은 M1을 통과한다. 개별 검사만으로는 2건 폭에서 아직 상한에 닿지 않기 때문이며,
  이것이 batch 합계 검사가 따로 필요한 이유다.
- M2 실패: 새 시험 3개 전부. 전폭 batch의 거부 문구는
  `256 member(s), 256 new, retained 0 B, reserve 268453266 B, need 268453266 B, limit 67108864 B`로,
  요구량이 상한의 정확히 4배다.
- M3 실패: 위 4개.

## 남은 것

- **`pressure`(resident 256) 실기 재판정.** 이 수정은 어댑터(`p4-agent.exe`)에 있으므로 staged CUDA
  서버 재빌드는 필요 없고 agent 재배포로 충분하다. 판정 항목은 전체 요청의 해제, 슬롯 재사용,
  후속 요청 처리, idle UNLOAD까지다. **그때까지 이 결함이 해소됐다고 실기 기준으로 주장하지 않는다.**
- 09-09 실행에서 실제로 거부된 명령 종류와 batch 폭. 새 문구가 다음 실행부터 이를 기록한다.
- 수용 경로(B2/B3)의 pending 개수·바이트·토큰 예산은 이 변경의 범위가 아니다. `worker.rs`의 수용
  코드는 여전히 요청 저장 공간 예약을 미래 작업으로 명시한다.
