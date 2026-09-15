# LOAD·UNLOAD 노드 수명 M1 결정론적 실행계획

## 봉인 범위

기준은 `e1fc1c8ee4fda82ab87217af28a78d43eb1b9970`이다. M1은 공통 codec에 이어 typed adapter lifecycle
completion, agent의 비동기 node supervisor, neutral fixture의 실제 TCP LOAD/UNLOAD를 구현한다.
llama.cpp/HF 실제 worker 이관은 M2, OUTER 호출자와 구 CREATE/DELETE 제거는 M3 소유다.

## 과거 교훈 재사용

- `L001`: 이 PC에서는 build와 model run을 하지 않는다. 원격 Spark에서 Cargo 절대 경로와 최대 8 jobs를 쓴다.
- `L002`: M1 neutral은 OS가 배정한 loopback port만 쓴다. M2 모델 전에는 event preflight를 통과한다.
- `L003`: 실제 TCP 시험은 OUTER→agent→node→agent-owned result→같은 OUTER 경로 전체를 대조한다.
- `L004`: Spark Cargo는 `/home/m42/.cargo/bin/cargo`로 먼저 고정한다.
- `L005`: ordinary completion의 유일 소비자는 EventNode다. supervisor가 같은 front를 poll하거나 무제한
  channel에 Event를 복제하는 구현은 코드 검토에서 거부한다.
- `L006`: patch한 기존 문서는 원래 CRLF로 정규화하고 전체 docs-lint를 통과한 뒤 코드 시험으로 간다.

## 1회차 전 소유권·호출 경로 검토

| 값 | 생성자 | 대기 중 owner | 성공 시 다음 owner | 실패 시 owner |
| --- | --- | --- | --- | --- |
| 외부 lifecycle Event | OUTER | agent bounded input | terminal result 발행 뒤 retire | control remainder 또는 명시 rejected result |
| model별 LOAD/UNLOAD Event | agent supervisor | node bounded input/adapter | adapter completion | node failure remainder |
| adapter completion | adapter | adapter bounded completion/EventNode | agent supervisor용 bounded 경계 | EventNode failure remainder |
| terminal lifecycle result | agent | bounded reply/transport | 원 OUTER | agent control remainder |

agent control은 공통 metadata, kind, ID 점유, 용량을 native 전에 검사하고 `loading`을 원자 등록한다.
긴 adapter 작업을 await하지 않는다. snapshot 문자열은 terminal 권한이 아니다. typed completion과 input/output
drain을 확인한 뒤에만 제거한다. 결과 source는 agent이고 causation/return context는 원 외부 요청과 일치한다.

## 금지된 설계

- EventNode와 supervisor가 같은 adapter completion front를 경쟁 소비.
- 무제한 `mpsc`나 원본 Event의 무회계 clone으로 완료 보존.
- LOAD 완료를 adapter snapshot 문자열로 추정.
- terminal result 전에 owner/route 제거, busy UNLOAD에서 admission fence 유지, 실패를 성공으로 변환.
- 시험을 맞추기 위한 queue/timeout 증가나 정상 입력 축소.

## 봉인 시험 묶음과 실행 라운드

첫 묶음은 `NL04`, `NL02`, `NL10`, `NL07`, neutral `NL01`을 함께 실행한다. malformed/exact±1과 disabled kind는
factory/native 전 무효과여야 한다. 느린 LOAD 중 INSPECT가 응답하고 중복 LOAD의 adapter start는 최대 1이다.
LOAD 성공 결과와 UNLOAD 성공 결과는 같은 OUTER로 돌아오며, 마지막 INSPECT는 nodes 0과 retained 0이다.

| 라운드 | 실행을 여는 조건 | 허용 변경 |
| --- | --- | --- |
| 1 | 위 소유권 표가 코드와 일치하고 rustfmt·문서 검사가 통과하며 전체 묶음이 작성됨 | 완성 후보 1회 |
| 2 | 1회 실패의 단일 인과 경로, 새 교훈 ID, 자동 차단 시험이 기록됨 | 그 원인에 필요한 한 변경 |
| 3 | 2회 변경과 제거 변이가 기대대로 실패하고 source/입력이 다시 봉인됨 | 확인 실행만 |

환경 설정·명령 오타도 실행 전 검토 실패다. 같은 명령을 그대로 재시도하지 않는다. 3회 뒤 새 설계가 필요하면
M1을 완료로 표시하지 않고 재설계한다.

## 실행 환경과 명령

- 개발 checkout: `F:\dev\p4`; 저부하 rustfmt, docs-lint, Python preflight 시험만 실행.
- Rust build/test: Spark `m42@192.168.0.26`, `/home/m42/.cargo/bin/cargo`, `CARGO_BUILD_JOBS<=8`,
  `RUST_TEST_THREADS<=8`, 별도 source copy와 target.
- 순서: 표적 protocol/adapter/agent 시험 → 실제 TCP neutral 묶음 → workspace feature off/on. 동시에 실행하지 않는다.
- 각 라운드는 source digest, 정확한 명령, exit code, pass/fail/ignored, 첫 실패, 남은 횟수를 보고서에 남긴다.

M1 완료에는 실제 TCP neutral 결과와 전체 회귀가 필요하다. 문서·codec·mock 함수만 통과한 상태는 진행 중이다.
