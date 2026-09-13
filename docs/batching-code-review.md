# llama 어댑터 배치 계획 — 마이크로 배치 제안과 코드 대조

2026-09-13 코드 검토. 기준 HEAD는 `d122125bafeaa6d32790761669f1bfa5868d8078`이다.
지위는 **실행 경로 검토와 개선 후보**이며 구현·성능 승격 문서가 아니다.
검토 대상 `staged/adapter`와 `staged/server`에는 시작 시 비커밋 변경이 없었다.
작업 트리의 별도 OUTER 모델 배치 정책·로드맵 변경은 이 검토에 포함하지 않는다.
검토 중 별도 작업으로 HEAD가 `484b856ee7e53aea5b850b654c45da53cb0724a6`으로 이동했다.
두 HEAD 사이 위 adapter/server 경로의 diff는 없으며, 최종 작업 트리의 해당 소스도 동일했다.

질문은 “프리필을 작은 조각으로 나누어 노드 공백을 줄이고, 디코드 요청을 넓게 묶어
남는 예산에 프리필을 편성하면 현재 구현보다 유리한가”이다. 첨부 그림은 비교 대상이며
그 안의 설명을 구현 지시로 취급하지 않았다.
계약은 [배치 계층](adapter-batching-layers.md), [격리](layer-isolation-contract.md),
실기 수용은 [검증 규약](distributed-batching-verification.md)이 소유한다.
실행 순서는 [로드맵](distributed-batching-roadmap.md#current-status)을 따른다.
아래 검토 우선도는 새 개발·배포·실험을 시작하는 지시가 아니다.

## 1. 판정

**D 우선·잔여 P 배정은 이미 구현되어 있다. 그러나 기본 배처, 선택적 파이프라인 정책,
native 내부 UBATCH 분할을 같은 기능으로 설명하면 실제 차이를 놓친다.**

| 사용자 제안 | 실제 코드 | 차이와 판정 |
| --- | --- | --- |
| 프리필을 잘게 나누어 단계마다 이어서 처리 | 논리 발행은 반복 가능하지만 native 내부 UBATCH는 한 호출이 끝난 뒤 묶어서 반환 | `n_ubatch` 축소만으로 그림의 UBATCH 단위 노드 중첩이 생기지 않는다. 논리 발행 단위와 전달 단위의 차이가 큼 |
| 생성 배치를 늘려 메모리 읽기 비용을 나누기 | 기본 ordinary는 ready D를 용량까지 선택. pipeline 정책은 활성 생성 인구/창으로 참여 폭을 제한 | 이미 폭 조절은 있으나 비용 최적화가 아닌 인구 산술. 폭·독립 묶음 수의 공동 선택은 남음 |
| 생성부터 넣고 남은 예산에 프리필 | 두 ordinary 경로 모두 D를 먼저 배정. 선택적 합계 예산은 D+P 전체에 적용 | 원칙은 일치. `mixed_prefill_rows`만 설정하면 D는 그 숫자에 추가되므로 합계 예산과 다름 |
| 부하에 맞춰 프리필 크기를 자동 조절 | 기본/프로파일 설정은 고정 행 상한. 별도 온라인 서비스 예측기는 P를 줄여 재계획 | 자동 재선택은 일부 있음. 실행 비용에 따른 D 폭·창 선택, 실제 KV 비용·요청 deadline 반영은 없음 |
| 한 긴 요청만 있어도 작은 조각 여러 개로 파이프라인 채우기 | RequestState에 fragment 한도는 있으나 pipeline 정책은 fragment1만 허용 | 조합 미지원. 단순 설정 변경으로 해결할 수 없음 |

따라서 “프리필 chunk만 조금 더 줄이면 된다”는 결론은 부족하다. 현재 범위에서 가장 가까운
비교 대상은 **선택적 pipeline 정책의 논리 발행량·D 참여 폭·창**이다. UBATCH 즉시 전달은
별도 실행 계약 변경이며, 현재 계약의 스케줄러 튜닝과 구분해야 한다.

## 2. 실제 소비 경로와 코드 근거

아래 링크의 줄 번호는 위 HEAD 기준이다. 파일 경로와 함수명을 함께 남겨 이후 이동에도 찾을 수 있게 한다.
`W`는 adapter `v2/node/worker`, `S`는 adapter `v2/scheduler`를 설명할 때만 쓰는 약칭이다.

| 단계 | 실제 코드 참조 | 확인한 동작 |
| --- | --- | --- |
| LOAD 용량 | [control.rs:76–91](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/control.rs#L76), `Worker::load` | readiness 검증 후 `n_batch`, `n_ubatch`, equal-width/atomic capability를 state에 저장 |
| 설정 | [state.rs:522–579](../layers/adapters/llamacpp/staged/adapter/src/v2/node/state.rs#L522), `AdapterState::default` | 기존 행/창 제한과 선택적 pipeline 정책, fragment 한도 |
| 설정 수용 | [worker.rs:431–461](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker.rs#L431), `Worker::prefill` | 유한 창·양수 예산·fragment1 검증, 합계 예산과 온라인 controller 동시 사용 거부 |
| 실행 루프 | [worker.rs:309–378](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker.rs#L309), `Worker::run_to_exit` | 유한 ingress 처리 후 `drive_one_batch`, coalescing timer의 `recv_timeout` 재진입 |
| 적격성 | [state.rs:368–379](../layers/adapters/llamacpp/staged/adapter/src/v2/node/state.rs#L368), `RequestState::phase_within` | 미발행 prompt와 outstanding 한도, 생성은 이전 outstanding이 0이어야 적격 |
| 수요·인구 수집 | [drive.rs:81–133](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L81), `Worker::drive_one_batch` | 한 session의 ready 수요와 admitted ready/inflight/waiting 인구를 별도로 집계 |
| 묶음 선택 | [pipeline.rs:71–115](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/pipeline.rs#L71), `PipelinePolicy::select` | 활성 인구로 P/D 참여 상한을 계산. active decode가 있으면 P quantum 적용 |
| 총량·모드 선택 | [drive.rs:217–244](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L217), [scheduler.rs:171–205](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L171) | 합계 issue cap 후 bounded ordinary / legacy ordinary / equal / atomic 분기 |
| 실제 행 배정 | [scheduler.rs:588–682](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L588), `plan_bounded_ordinary`; [686–758](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L686), `plan_ordinary` | D부터 1행씩, 남은 P를 회전 배정. 두 경로의 기아 방지 차이는 §5 |
| 시간 예측·재계획 | [worker/service.rs:57–115](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/service.rs#L57), `prepare_generation_service_plan`; [scheduler/service.rs:298–321](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/service.rs#L298), `project_tail` | open 발행 순서·stage RPC 비용 투영, P 반감 재선택/보류/probe |
| 합류 대기 | [drive.rs:252–277](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L252) | D-only 후보만 짧게 대기. 목표 폭은 묶음 폭·행 용량으로 제한 |
| native 발행 | [drive.rs:417–461](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L417) | issue 준비 뒤 동기 `stage_request(LogicalBatch, PhysicalResult)` |
| UBATCH 캡처 | [llama_stage_runtime_physical.cpp:92–128](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_physical.cpp#L92), `capture_execution`; [131–235](../layers/adapters/llamacpp/staged/server/src/runtime/llama_stage_runtime_physical.cpp#L131), `execute_first_batch` | 콜백 결과를 vector에 누적. `llama_decode` 완료와 행 보존 검증 후 전체 반환 |
| native 결과 포장 | [server_physical.cpp:196–301](../layers/adapters/llamacpp/staged/server/src/server/server_physical.cpp#L196), `Session::handle_logical_batch` | 캡처 전체에 owner를 대응하고 한 PhysicalResult로 인코딩 |
| 승인 후 전송 | [drive.rs:528–583](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/drive.rs#L528) | accepted issue, frontier, fairness 확정 뒤 정확한 결과 bytes를 `ForwardObserved`에 보관·전달 |
| 다음 stage | [worker/physical.rs:7–103](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/physical.rs#L7), `Worker::physical` | CapsuleSet의 fresh 멤버를 검증하고 PhysicalBatch 호출. 다음 stage가 임의로 D/P 재배치하지 않음 |

배치 정책은 concrete adapter 소유이며, 이 개선을 P4 공통 transport에 llama 전용 스케줄러를
넣는 작업으로 옮기지 않는다.

## 3. 구현된 정책의 범위

### 3.1 기본 ordinary와 선택적 pipeline은 다르다

`P4_STAGED_PIPELINE_BATCHING`이 정확히 `1`일 때만 `PipelinePolicy`가 생성된다.
기본 `OrdinaryLimits`는 각 축 0이며, 여기서 0은 legacy 한도 유지이지 “0토큰 처리”가 아니다.
ordinary의 논리 용량은 `n_batch`이고, `n_ubatch`는 native 분할 상한이다.
bounded ordinary도 물리 용량을 확인하지만 배정량을 항상 한 UBATCH 이하로 자르지는 않는다.
근거: `Scheduler::prepare_plan_with_limits` 181–197행.

pipeline 창을 `W`, admitted 활성 D 인구를 `N_D`라고 하면:

```text
D 묶음 수 목표 = min(N_D, W)
D 참여 폭 상한 = ceil(N_D / max(D 묶음 수 목표, 1)), 최소 1
P 참여 폭 상한 = ceil((미발행 prompt 인구 + 마지막 prompt 반환 대기 인구) / W)
실제 참여 폭에는 명시적 사용자 한도와 적격 요청 수가 추가로 적용된다.
```

이는 묶음 identity를 고정 예약하는 방식이 아니다. 매 발행 때 인구로 **폭 상한**을 만들고,
회전한 ready 요청을 그 안에서 고른다. `decode_groups`/`prefill_groups`는 실제 동시 GPU 실행 수가 아니다.
`select`에는 단계별 시간·대역폭·KV 비용 입력이 없다.

생성이 진행 중이면 `mixed_prefill_rows`가 P 전체 상한으로 적용된다. D가 지금 다른 stage에
있어 ready D가 0이어도 적용한다. 생성 전 초기 pure-prefill에는 이 제한을 적용하지 않는다.
선택적 `mixed_batch_rows`는 생성이 활성인 동안 D+P 합계 cap이며 두 상한을 함께 지킨다.

### 3.2 온라인 비용 정책은 존재하지만 별도 실험 경로다

`P4_STAGED_PREFILL_SERVICE_MS`는 `configured_budget`에서 기본 비활성으로 읽는다.
활성화하면 `ServiceShape(P행, D행, 요청 수, 최대 input position)`와 stage별 RPC 표본을 사용한다.
`project_tail`은 기존 open 배치와 후보의 완료를 `max(앞 stage 도착, 해당 stage 사용 가능)+비용`으로
추정한다. 예산 초과/비용 미상 후보는 P를 절반씩 줄여 다시 계획하며, 최소 probe나 D-only로 바뀔 수 있다.

따라서 “시간을 보고 프리필을 다시 자르는 코드가 전혀 없다”는 지적은 틀리다.
하지만 이 경로는 D 참여 폭이나 `max_open_batches`를 최적화하지 않고, 합계 행 예산 설정과 동시
사용도 거부된다. 고정 `mixed_batch_rows`는 런타임이 자동으로 프로파일을 찾아 바꾸는 값이 아니다.
코드 근거: [service.rs:7–29](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/service.rs#L7),
[재선택 루프](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/service.rs#L70),
[동시 설정 거부](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker.rs#L458).

## 4. 같은 입력에서 얼마나 달라지는가

아래는 §7의 로컬 probe로 **현재 소스의 selector를 직접 호출한 행 배정 결과**다.
GPU/TPS·실제 worker 도착 패턴·native UBATCH membership을 측정한 결과가 아니다.
입력은 ready D 32요청, ready P 8요청(각 4096행), 논리 용량512, 물리 상한128,
pipeline 사용 시 창4·P 상한128·추가 사용자 한도 없음이다.

| 경로 | 선택 D행 | 선택 P행 | 논리 합계 | 참여 요청 |
| --- | ---: | ---: | ---: | ---: |
| 기본 ordinary | 32 | 480 | 512 | 40 |
| pipeline, P 상한128만 | 8 | 128 | 136 | 10 |
| pipeline, 합계 상한128 추가 | 8 | 120 | 128 | 10 |

두 번째와 세 번째의 차이는 P 8행이며, 첫 번째와 세 번째의 차이는 D 24요청을 다음 독립
발행 기회에 남기고 이번 P를 360행 줄인 것이다. 이 입력에서는 136행을 물리 상한128에
넣을 수 없으므로 native 분할이 필요하지만, 정확한 P/D 물리 혼합은 콜백 결과로 확인해야 한다.
이 숫자로 TPS 향상률을 계산하지 않는다.

그림처럼 MB1 결과를 즉시 다음 노드에 보내는지의 차이는 다음과 같다.

```text
현재 한 논리 발행이 여러 UBATCH로 나뉘는 경우:
head: [UB1 계산/캡처][UB2 계산/캡처]…[전체 검증·포장] → next에 CapsuleSet 전달

별도 독립 논리 발행 A/B가 가능한 경우:
head: [A native 완료·승인·전달][B native 완료·승인·전달]…
next:                          [A 실행]…
```

현재 중첩 기회는 두 번째의 **독립 논리 발행 사이**에 있다. 첫 번째의 UBATCH 수를 늘려도
head에서 첫 UBATCH가 끝난 즉시 next가 시작하는 경로로 바뀌지는 않는다.

## 5. 개선 후보와 필요한 반례

### G1 — 기본 ordinary의 프리필 진행 보장 차이: 우선 검토

`plan_ordinary` 705–724행은 D가 용량을 소진한 뒤 P를 배정한다. 용량8, 매 기회 ready D8과
P1을 동일하게 제공한 32회의 승인 선택에서 **D256/P0**이었다. bounded ordinary에
`decode_members=8, prefill_members=1, prefill_rows=128`을 주면 **D224/P32**였다.
bounded 경로는 P가 있으면 D용량을 `capacity-1`로 제한하고, capacity1에서는 patience를 쓴다.

이는 기본 selector의 포화 반례다. 실제 도착·반환으로 매번 D8이 유지되는지까지 이번 probe가
증명하지 않으므로 서비스 전체의 무한 기아를 관측했다고 주장하지 않는다. 초기 답변의
“현재 스케줄러에 프리필 기아 방지가 있다”는 표현은 이 두 경로를 구별해야 한다.

개선 후보는 기본 ordinary에도 명시적인 P 진행 기회 계약을 적용하거나, 지원 조합별 정책 선택을
명확히 하는 것이다. **검증 전 기본값을 pipeline ON으로 바꾸는 제안은 아니다.**
실제 worker 반례는 ready D 수가 용량 미만/같음/초과인 지속 반환과 P 대기를 조합하고,
각 P의 최초·연속·마지막 선택 간격 및 D의 정상 진행을 검사해야 한다.
거부 시 fairness·예약·native 호출 보존과 guard 제거 변이도 필요하다.

### G2 — UBATCH 분할과 전송 경계: 효과가 큰 구조 차이

`capture_execution`은 결과를 `captured_executions_`에 쌓고 `execute_first_batch`가
`llama_decode` 종료 뒤 반환한다. `handle_logical_batch`와 `drive_one_batch`는 전체 행 검증과
승인 뒤 한 번 전달한다. **UBATCH 단위 스트리밍 파이프라인은 구현되어 있지 않다.**

현재 계약 안의 작은 개선 후보는 논리 issue의 P 상한·참여 수를 조절해 독립 요청을 남기는 것이다.
진짜 UBATCH 조기 전달은 별도 후보다. 부분 native 실패 뒤 이미 하류로 나간 KV 효과,
부분 결과 identity·저장 공간 선예약·backpressure·취소/정산의 권한을 먼저 설계해야 한다.
콜백 안에서 바로 전송하는 작은 수정으로 취급하면 안 된다.

판별 반례는 한 논리 호출이 최소2개의 UBATCH를 만들도록 하고, UB1 완료→첫 전송 시각과
UB2 완료 시각을 측정하는 것이다. 조기 전달 후보는 하류가 UB1을 받은 뒤 UB2가 실패하는 경우도
정확히 보존해야 한다. 현재 동작은 source로 확인했으며 native 조기 전달 시험은 미실행이다.

### G3 — 생성 폭·창은 비용 최적값이 아니다

`PipelinePolicy::select`는 인구와 창만으로 D 폭을 계산한다. D32/창4이면8, 창8이면4다.
각 단계가 메모리 읽기나 배치당 고정 비용 때문에 더 넓은 D를 필요로 하는지 판단하지 않는다.
반대로 ready 요청을 모두 모으는 선택은 독립 flight 공급을 줄일 수 있다.
2ms coalescing도 현재 합법적인 묶음 폭 안에서 기다리는 기능이지 최적 폭 탐색기가 아니다.

개선 후보는 모델·backend·문맥·토폴로지에 결속한 사전 측정으로 D 폭과 논리 창 후보를 함께
선택하는 것이다. 한 장치의 여러 stage도 독립 GPU 수로 계산하지 않는다. 비교 시 row 폭과
window를 별도 축으로 고정하고 stage 공백·배치당 시간·정상 생성 TPS·ITL을 함께 판정한다.
`select`에 정규화한 비용 입력을 추가하더라도 KV/flight 승인 권한은 기존 원장에 둔다.

### G4 — 고정 행 예산과 실제 문맥 비용 사이의 차이

기본/고정 프로파일 경로는 같은 P행 상한을 문맥 전 구간에 적용한다. 온라인 경로에는 최대
position의 2진 구간별 비용 표본이 있지만, `ServiceShape::last_position`은 실제 backend
`n_kv`가 아니다. [ServiceShape 정의](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/service.rs#L13),
[예측 입력 필터](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/service.rs#L219).
선택 요청의 position만으로 다른 요청이 차지한 공유 KV 범위·mask 비용을 설명할 수 없다.

개선 후보는 먼저 native의 실제 KV 접근 범위·mask 생성/복사·kernel·RPC 대기 비용을 분리하여
측정하고, 필요한 정규화 관측만 adapter의 비용 표본에 전달하는 것이다. 다음으로 같은 모델의
짧은/긴 문맥에서 D 폭·P quantum의 비용 곡선을 비교한다. 자동 예측기와 고정 예산을 단순 합치는
것보다 비용 입력의 의미를 먼저 검증해야 한다.
현재 검토로 메모리 대역폭이 주 병목인지 또는 GPU 활용률이 얼마나 변할지는 결정할 수 없다.

### G5 — 한 요청의 여러 프리필 조각: 의도적으로 닫힌 조합

`RequestState::phase_within`은 prompt의 여러 outstanding을 표현하지만,
pipeline policy는 입구와 drive에서 `prefill_fragments != 1`을 거부한다.
따라서 한 요청뿐인 구성은 P chunk를 줄여도 여러 논리 flight로 채울 수 없다.

독립 요청 부족이 실제 병목으로 확인되면 일반 attention의 multi-fragment와 pipeline 정책을
함께 지원하는 별도 후보를 검토한다. 필요한 반례는 동일 요청 chunk의 stage별 순서·중복 위치,
중간 취소/실패, 마지막 prompt의 조기 생성, 슬롯 재사용이다. 단순히 두 guard를 지우지 않는다.
recurrent/hybrid는 [등폭 경로](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L328),
Verify/Replay는 [atomic 경로](../layers/adapters/llamacpp/staged/adapter/src/v2/scheduler.rs#L305)를
유지하며 ordinary 혼합 배치 계약을 그대로 적용하지 않는다.

### G6 — 지연 목표와 비용 최적화의 관측 범위

`Demand`에는 요청 deadline·대기 시간·실제 KV 비용이 없다. 온라인 service 예산은 후보의
pipeline RPC 완료 예측이며, OUTER의 토큰 수신 ITL을 직접 제어하는 deadline scheduler가 아니다.
`drive_one_batch`는 미발행 대기 이유를 일부 snapshot으로 남기지만, 정책별 누적 대기 시간과
요청별 지연 예산 소비를 완전하게 보고하지 않는다.

개선 후보는 먼저 발행 차단 이유/기간과 요청별 TTFT·ITL을 실제 관측에 결속하는 것이다.
그 뒤 P 진행 기회·D 지연을 동시에 제한할 필요가 확인되면 요청별 age/deficit을 후보 입력에
추가한다. 거부된 계획은 이 상태를 소비하지 않아야 한다.
기존 `prepare/validate/commit`와 source/binary 결속 변이 규약을 유지한다.

## 6. 이번 검증의 범위

검토 대상 소스의 비커밋 차이가 없음을 실행 전 확인했다. 이번 변경은 문서와 색인이며
제품 코드·기대값·시험 입력을 수정하지 않았다. 기존 시험은 다음과 같이 다시 실행했다.

| 명령 (`F:/dev/p4`) | 결과 | 증명 범위 |
| --- | --- | --- |
| `cargo test -p p4-llamacpp-staged-adapter --lib v2::scheduler -- --nocapture` | 33 passed / 0 failed / 0 ignored, 524 filtered | selector·pipeline·서비스 모델 및 mixed planner의 로컬 시험 |
| `cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy -- --nocapture` | 10 passed / 0 failed / 0 ignored, 547 filtered | 실제 Worker 루프·Frame 경계·정산, fake native |
| `cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::service_budget -- --nocapture` | 2 passed / 0 failed / 0 ignored, 555 filtered | 비용 피드백·재선택·probe의 실제 Worker 경로, fake native |

합계는 서로 겹치지 않는 선택 시험45개다. 전체 `cargo test --workspace --no-fail-fast` 집계가 아니며,
나머지 시험·새 개선의 변이·native/GPU 실기·성능 비교는 이번 검토에서 미실행이다.
기존 시험 `profiled_pipeline_actual_loop_spends_only_residual_tokens_on_prefill`은
[bounded_strategy.rs:6](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/loop_tests/bounded_strategy.rs#L6),
독립 D 묶음은 [36행](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/loop_tests/bounded_strategy.rs#L36),
초기 P 폭은 [78행](../layers/adapters/llamacpp/staged/adapter/src/v2/node/worker/loop_tests/bounded_strategy.rs#L78)에 있다.
이들의 fake native 통과를 CUDA/Metal의 물리 배치 비용 증명으로 세지 않는다.

로그·probe 원본은 로컬 `target/batching-code-review-20260913/`에 보관한다.
이는 장기 실기 증거 번들이 아니다. §7은 target 삭제 뒤에도 핵심 selector 대조를 재현하도록
실행 방법을 남긴다. 문서 링크/EOL/색인 검증은 아래 마감 기록에 기록한다.

## 7. selector 대조 재현

이 probe는 소스 변경 없이 public `Scheduler`와 현행 `PipelinePolicy` 소스를 사용한다.
임시 Rust 프로젝트를 만들고 다음 manifest를 사용한다. 다른 checkout에서는 두 절대 경로를 함께 바꾼다.

```toml
[package]
name = "p4-batching-review-probe"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
serde = { version = "=1.0.229", features = ["derive"] }
p4-llamacpp-staged-adapter = { path = "F:/dev/p4/layers/adapters/llamacpp/staged/adapter" }
```

`src/main.rs`:

```rust
#![allow(dead_code)]
use p4_llamacpp_staged_adapter::v2::{Demand, Phase, Scheduler, OrdinaryLimits, SchedulerError};
#[path = "F:/dev/p4/layers/adapters/llamacpp/staged/adapter/src/v2/scheduler/pipeline.rs"]
mod pipeline;
use pipeline::{PipelinePolicy, PipelinePopulation, PhasePopulation};

fn demands(d: usize, p: usize) -> Vec<Demand> {
    (0..d+p).map(|i| Demand {
        request_id: format!("r{i}"), sequence_id: i as u32,
        compatibility: "review-session".into(),
        phase: if i < d { Phase::Decode } else { Phase::Prefill },
        available_rows: if i < d { 1 } else { 4096 }, atomic: false,
    }).collect()
}
fn case(label: &str, total: Option<usize>, pipeline: bool) {
    let input = demands(32, 8);
    let mut limits = OrdinaryLimits::default();
    let mut capacity = 512;
    if pipeline {
        let selected = PipelinePolicy { mixed_batch_rows: total, mixed_prefill_rows: 128 }
            .select(&input, limits, 4, 0, PipelinePopulation {
                decode: PhasePopulation { ready: 32, ..Default::default() },
                prefill: PhasePopulation { ready: 8, ..Default::default() },
                ..Default::default()
            }).unwrap();
        limits = selected.effective_limits;
        if let Some(t) = total { capacity = capacity.min(t); }
    }
    let s = Scheduler::new();
    let plan = s.prepare_plan_with_limits(&input, capacity, 128.min(capacity), false,
        usize::MAX, false, limits).unwrap();
    let rows = |phase| plan.allocations().iter().filter(|a| a.phase == phase).map(|a| a.rows).sum::<usize>();
    println!("{label}: D={} P={} total={} members={} limits={limits:?}", rows(Phase::Decode),
        rows(Phase::Prefill), rows(Phase::Decode)+rows(Phase::Prefill), plan.allocations().len());
}
fn main() {
    case("legacy", None, false);
    case("pipeline-prefill-cap", None, true);
    case("pipeline-total-cap", Some(128), true);
    for (label, limits) in [("legacy-saturated", OrdinaryLimits::default()),
        ("bounded-saturated", OrdinaryLimits { decode_members: 8, prefill_members: 1,
            prefill_rows: 128, prefill_rows_per_request: 0 })] {
        let input = demands(8, 1);
        let mut s = Scheduler::new();
        let mut p = 0;
        let mut d = 0;
        for _ in 0..32 {
            let plan = s.prepare_plan_with_limits(&input, 8, 8, false, usize::MAX, false, limits).unwrap();
            for a in s.commit_plan(plan).unwrap() {
                if a.phase == Phase::Prefill { p += a.rows; } else { d += a.rows; }
            }
        }
        println!("{label}: 32 accepted selections, D={d} P={p}");
    }
}
```

실행 명령:

```powershell
cargo run --offline --manifest-path target/batching-code-review-20260913/probe/Cargo.toml --target-dir target/batching-code-review-20260913/probe-build
```

이 예제는 selector의 승인 기회를 직접 만든다. saturation 대조에서 수요를 매번 재제공하므로
실제 생성 내용·요청 종료·KV 변경·네트워크 동작을 흉내 내지 않는다.
최초 probe 빌드는 `scheduler.rs`를 직접 외부 `#[path]` 모듈로 읽어 하위 모듈을 찾지 못해
E0583으로 실패했다. public crate 의존과 `pipeline.rs` 직접 참조로 실행 진입점만 고쳤다.
제품 소스는 바꾸지 않았으며 이 빌드 오류를 배처 결함으로 세지 않는다.

## 8. 마감 기록

- 코드 검토와 selector 대조 완료. 재실행한 기존 선택 시험45개 실패0, probe의 다섯 결과는 §4/G1과 일치.
- 제품 코드와 기존 시험은 변경하지 않았다. 새 검토 문서를 README와 문서 안내도에 등록했다.
- `node tools/scripts/docs-lint.mjs`: Git 추적 문서97개 clean. 새 untracked 문서를 검사했다는 의미는 아니다.
- 추적 Markdown97개와 새 검토/기존 별도 초안2개를 `target/batching-code-review-20260913/docs-snapshot/`에
  원문 그대로 복사하고 같은 lint의 `--all` 실행:99개 clean. source/링크/EOL 검사의 범위를 이 집합으로 명시한다.
- 원래 workspace 전체의 `node tools/scripts/docs-lint.mjs --all`은465개 중 오류366개로 실패했다.
  출력은 기존 `.cache/llama-pipeline-upstream/`의 upstream Markdown을 문서 안내도에 등록하라는 오류였다.
  캐시를 삭제하거나 lint 규칙을 완화하지 않았다. 이 전체 디스크 검사를 GREEN으로 기록하지 않는다.
- 검토 문서의 코드/문서 링크33개는 대상 파일 존재와 코드 줄 범위를 확인했다.
  문서에 수록한 Rust probe 본문은 실제 실행 파일과 일치했다. README/문서 안내도의 diff whitespace 검사 통과.
- source 기준은 위 두 HEAD의 adapter/server 무차이와 최종 working-tree 무차이로 확인했다.
  동시 작업의 기존 문서 변경을 보존하며 이번 문서·색인은 작업 트리에 남겼다. commit/push는 하지 않았다.
- 성능·GPU 활용 개선은 미측정이다. 구현 요청으로 이어질 경우 먼저 해당 G 항목의 실제 소비 반례와
  비용 관측을 준비하고, 실행 순서와 승격은 로드맵/검증 규약에 연결한다.

<a id="review-correction"></a>

## 9. 통합 계획 최종 검수 정정 (2026-09-13)

§6/§8의 45/45는 최초 작성자의 실행 기록이다. 뒤이은 동일 소스 경로 재실행에서는
scheduler 33/33, bounded_strategy 9/10, service_budget 2/2로 **44 passed / 1 failed / 0 ignored**다.
시험 재검토 기준은 `2ff8cc703511aa23b72c08af61da45c557b4c278`이며 484b856ee 이후 OUTER 변경은
Rust/native 실행 경로를 수정하지 않았다. 이 정정은 기존 성공 실행이 없었다는 주장이 아니라
현재 재현 가능한 실패를 추가한 것이다. 후속 245d6b785의 OUTER 모듈 이동에도 Rust/native 차이가 없음을 확인했다. 전체 workspace 시험 결과는 아니다.

실패 시험은 `v2::node::worker::loop_tests::bounded_strategy::phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input`이다.
단독 실행도 exit 101/0 passed/1 failed였다. `bounded_strategy.rs:283`의 timer 대기 권한 보존 assertion에서
`next_event 57→58`, `free_sequences []→[1]` 차이가 났다. 정상 RELEASE가 snapshot 사이에서 처리된
시험 동기화 문제인지 실제 timer의 권한 변경인지 아직 판별하지 않았다. assertion 삭제/timeout 연장으로
통과시키지 않고 event 순서와 실제 소비 경로를 확인한다.

```powershell
cargo test -p p4-llamacpp-staged-adapter --lib v2::node::worker::loop_tests::bounded_strategy::phase_pacing_actual_loop_expires_decode_wait_without_another_tail_or_input -- --exact --nocapture
```

- 로그: `target/external-slide-review-20260913/review-20260913/phase-pacing-alone.log`, SHA-256 `4d3e2a4979ac853b91998c3832d45603af615188b726d63d9a1a472cadb471ec`.
- 시험 소스 SHA-256: `c68a5e55ab9c1cd39908efa5e683db00dc251cd8f8d56e9578693a5a110121d1`.
- 실행 binary `target/debug/deps/p4_llamacpp_staged_adapter-9aafc450c15b9e1d.exe`, SHA-256 `b262d1ad70beb632cbfd394a0e9126b63e0743545b65e0e5b6123a5b1632eda6`.
- selector probe는 재현했으나 실제 GPU/연속 서비스 성능 증거는 아니다. 제품/시험 소스는 변경하지 않았다.

G1–G6의 제품 채택·보류 이유는 [개발 계획 §5](external-analysis-improvement-plan.md#batch-decisions),
Release A의 반례/실기 수용은 [검증 규약](distributed-batching-verification.md#release-a-contract)에 연결했다.
G1/G3/G4/G6의 진행/관측 계약과 G2/G5의 추가 상태 계약을 구분하며, 6개 알고리즘을 모두 만드는
계획으로 해석하지 않는다. 새 세션의 첫 회귀 과제는 위 timer RED다.

최종 문서 마감 시 245d6b785에서 같은 단독 시험을 다시 실행해 exit 101/0 passed/1 failed를 확인했다.
추가 로그는 `target/external-slide-review-20260913/review-20260913/phase-pacing-final-review.log`,
SHA-256 `322d563772b3131459750b8d56c7cfc4c116a658c9e687cc03a4f1bc8b38d94d`다.
