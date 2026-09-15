# LOAD·UNLOAD 노드 수명 M2 결정론적 실행계획

## 봉인 범위

기준은 `094a97e84233cc428f32341e55ea1636c6b1b064`다. M2는 M1 agent supervisor를
llama.cpp와 HF의 실제 retained worker에 연결한다. OUTER 호출자 이관과 CREATE·DELETE 제거는 M3,
실제 모델·다중 컴퓨터 수용은 M4 범위다.

## 사전 판정

- 내부 수명 Event는 agent가 source이고 node가 target이며 원 OUTER return route를 그대로 가진다.
- adapter는 추론의 논리적 OUTER owner를 바꾸지 않고 수명 terminal만 같은 agent로 돌려보낸다.
- 성공 LOAD는 native/child 준비와 identity 결속 뒤 `Present`, 성공 UNLOAD는 실제 종료 뒤 `Absent`다.
- native 효과 전 LOAD 거부는 `Rejected/Absent`, busy UNLOAD는 `Rejected/Present`다.
- 시작 뒤 cleanup 실패나 결과 불명은 `Failed/Unknown`이며 route를 열거나 owner를 제거하지 않는다.
- adapter 결과 JSON/IPC 본문에 typed completion을 결속해 readiness·first error·cleanup error를 보존한다.
- snapshot 문자열과 process/thread 종료 추정은 terminal 권한으로 사용하지 않는다.

## 과거 교훈 재사용

- `L001`: 이 PC에서는 build/model run을 하지 않는다. Spark에서 Cargo 절대 경로와 최대 8 jobs를 쓴다.
- `L002`: 실제 TCP 포트는 OS 배정 또는 동적 범위 밖의 봉인 포트를 사용한다.
- `L003`: lifecycle completion의 source·target·causation·generation과 원 OUTER 응답을 함께 대조한다.
- `L004`: Cargo·Node·Python의 절대 경로와 `HF_TEST_PYTHON`을 실행 전 한 번에 고정한다.
- `L005`: EventNode만 adapter completion front를 제거한다. 새 무제한 channel을 만들지 않는다.
- `L006`: source/binary/hash가 다른 변이는 별도 worktree와 target에서 재컴파일한다.
- `L007`: 기존 listener·agent·모델 프로세스는 종료하지 않고 시험 PID만 회수한다.
- `L008`~`L013`: shell 인용, test count, 원격 동기화, owned 값 이동을 build 전 검사한다.
- `L014`: 원격 patch는 `git diff --output`으로 만들고 LF/CRLF 수와 apply check를 먼저 확인한다.
- `L015`: 원격 runner는 `set -euo pipefail`과 확인된 절대 경로 도구를 쓰며, 두 adapter의 표적 시험이 각각 1건 이상임을 build 전에 단언한다.
- `L016`: blocking 첫 입력과 후속 drain은 공통 `handle_received_input`을 써야 한다. 첫 LOAD가 terminal을 내고 원 input claim이 0이 되는 시험을 유지한다.
- `L017`: queue 포화 시험은 byte profile 여유를 별도로 보존하고, 앞 입력의 output 1·upstream claim0을 확인한 뒤 수명 입력을 보낸다.

## 소유권과 상태표

| 경우 | typed status | resource state | agent 동작 |
| --- | --- | --- | --- |
| llama/HF 정상 LOAD | succeeded | present | drain 확인 뒤 route 개방 |
| opaque LOAD 사전 거부 | rejected | absent | 임시 route와 owner 제거 |
| child/native 시작 뒤 cleanup 성공 실패 | failed | absent | 실패 결과 뒤 owner 제거 |
| child/native cleanup 실패·불명 | failed | unknown | owner와 fence 유지 |
| llama/HF busy UNLOAD | rejected | present | fence 해제, 기존 작업 계속 |
| llama/HF 정상 UNLOAD | succeeded | absent | route·owner 제거 뒤 OUTER 결과 |

## 봉인 시험과 라운드

첫 라운드는 두 adapter의 typed decoder, agent-target terminal, 기존 OUTER 경로 회귀,
HF busy/cleanup 실패, llama native fixture LOAD/UNLOAD와 retention census를 함께 실행한다.
그 뒤 M1 실제 TCP와 workspace feature off/on을 실행한다. timeout·기대값·용량을 실패 뒤 완화하지 않는다.

| 라운드 | 실행을 여는 조건 | 허용 변경 |
| --- | --- | --- |
| 1 | `cargo check --tests`와 표적 시험 목록이 두 adapter 모두 1개 이상임 | 코드/fixture의 결정론적 결함만 수정 |
| 2 | 1회 실패 원인이 코드·시험·runner 중 하나로 단일 분류되고 장부에 기록됨 | 그 원인의 최소 수정 |
| 3 | 2회 결과와 source/target/retention 로그가 예상 상태표와 일치 | 확인 실행만 |
| 4~5 | 3회 안에 못 끝난 이유와 자동 사전 차단을 먼저 추가 | 사용자 허용 범위의 예외 확인 |

## 변이

독립 worktree에서 실제 adapter completion의 agent target 결속 또는 typed lifecycle 필드를 제거한다.
표적 실제 worker 시험이나 M1 agent TCP 시험이 실패해야 한다. baseline target을 공유하지 않는다.

## 실행 결과

M2는 5회차 최종 후보에서 완료했다. llama.cpp 3개와 HF 4개 표적 시험, workspace feature off/on
각 1,523 passed / 0 failed / 7 ignored, 최종 소스 독립 변이 3개를 통과했다. `L014`~`L018`은
[결정론적 실행 장부](../../docs/deterministic-execution-register.md)에 자동 차단 수단과 함께 남겼다.
정확한 source, 라운드, 명령 결과와 로그 hash는
[M2 보고](../reports/node-load-lifecycle/20260916_032621.md)를 따른다. 다음 단계는 M3 호출자 이관이다.
