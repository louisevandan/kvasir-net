# LOAD·UNLOAD 노드 수명 M3 결정론적 실행계획

## 봉인 범위

기준은 `2ee9b4905c7971356f6088876caf346f2ab1acb6`다. M3는 Rust event-drive,
Qwen HF OUTER controller와 현행 lifecycle 검증 스크립트를 Agent-target NODE_LOAD/NODE_UNLOAD로
이관한다. agent event runtime의 legacy CREATE/DELETE 수용 분기를 제거한다. service 계층의 별도
`Agent::create_node/delete_node`와 날짜별 역사 artifact는 이 단계에서 바꾸지 않는다.

## 사전 판정

- 외부 NODE_LOAD/NODE_UNLOAD는 Agent가 target이고 원 OUTER source/return route를 보존한다.
- 공통 little-endian lifecycle metadata 뒤의 opaque bytes만 adapter별 기존 LOAD/UNLOAD 형식이다.
- 결과 source는 요청 대상 Agent다. correlation/causation을 먼저 검증한 뒤 lifecycle metadata의
  node/generation/adapter/operation/status/resource state를 대조한다.
- 성공 LOAD의 opaque adapter 결과에서만 build readiness를 읽는다. 전송 ACK나 접수는 성공이 아니다.
- 한 wave의 LOAD가 거부되면 같은 wave에서 이미 성공한 노드와 앞 wave 성공 노드를 OUTER가 개별
  NODE_UNLOAD한다. P4가 다른 agent의 노드를 자동 회수하지 않는다.
- 정상 종료는 NODE_UNLOAD 결과만으로 node와 native 자원 제거를 확인한다. DELETE를 덧붙이지 않는다.
- legacy CREATE/DELETE와 node-target lifecycle은 부작용 없이 거부한다.

## 과거 교훈 재사용

- `L001`~`L020`을 모두 적용한다. 이 PC에서는 build/model run을 하지 않고 Spark에서 jobs 8 이하로
  실행한다. patch는 LF/hash/apply check 뒤 옮긴다.
- `L019`: lifecycle 결과의 Agent source로 stage를 추측하지 않고 causation ID→요청 node 결속 뒤
  metadata identity를 다시 검사한다.
- `L020`: HF 정상 UNLOAD와 실패 회수 abort 모두 Agent lifecycle을 통과해야 하며, cleanup 뒤
  `owned`와 `ready`가 실제 결과에 맞춰 비워져야 한다.
- `L021`: 원격 준비·시험·변이는 모두 검토 가능한 LF runner를 전송해 SHA를 대조한 뒤 실행한다.
  PowerShell 안에 원격 `$()`나 파이프를 중첩하지 않는다.
- `L022`: 새 파일은 로컬 patch 생성 전과 원격 적용 후 같은 경로를 `git add -N` 처리한 상태에서
  diff SHA를 비교한다. untracked 파일이 동일성 검사에서 빠지는 것을 허용하지 않는다.
- `L023`: diff SHA는 양쪽 모두 `--binary --full-index`로 생성해 Git 객체 ID 축약 설정의 영향을
  제거한다.
- `L024`: 실패 기대 시험은 성공 타입의 `Debug`에 기대는 `unwrap_err` 대신 명시적 `match`를 쓰고,
  `cargo check --tests`가 통과한 뒤에만 실행 시험으로 넘어간다.
- `L025`: unknown-node 우회 요청은 실제 ingress 로그와 snapshot의 `rejected_remote`를 대조하고,
  노드 무효과는 `nodes=[]`로 독립 확인한다.
- `L026`: HF worker EOF의 불명 원인은 문자열 그대로 보존하고, 이후 supervisor abort의 성공/absent를
  별도 terminal에서 확인한다. 오류 관측을 boolean으로 축약하지 않는다.
- `L027`: lifecycle controller의 import/codec 시험은 모델 package가 없는 표준 Python에서도 수집돼야
  한다. safetensors는 실제 tensor step에서만 요구하고 runner는 수집 시험 수를 실행 전에 확인한다.
- 시험 peer는 실제 요청 target/source/causation과 lifecycle metadata를 해석한다. content type 문자열
  개수만 세어 새 경로 사용을 주장하지 않는다.
- Python과 Rust는 같은 literal fixture를 각각 독립 구현하지 않는다. wire prefix, schema, identity,
  status/resource state를 상호 호환 fixture로 고정한다.
- partial rollback 시험은 첫 거부에서 socket을 닫지 않는다. OUTER가 성공 노드에 보낸 UNLOAD와 그
  완료까지 관측해야 한다.

## 필수 시험

| ID | 실제 경로 | 판정 |
| --- | --- | --- |
| M3-01 | Rust event-drive LOAD builder/response consumer | Agent target, common codec, typed succeeded/present, opaque readiness 사용 |
| M3-02 | Rust 두 agent LOAD 중 하나 rejected/absent | 성공 node에 OUTER UNLOAD 1회, CREATE/DELETE 0 |
| M3-03 | event-drive CLI socket fixture의 inference 실패+teardown | 새 LOAD/UNLOAD만 사용하고 기존 실패 artifact 보존 |
| M3-04 | HF Pipeline 실제 Client codec | CREATE/DELETE 0, Agent lifecycle 결과 검증, 정상 shutdown 후 owned/ready 0 |
| M3-05 | HF lifecycle 실패/cleanup fixture | failed/unknown과 cleanup error를 숨기지 않고 recovery 명령 기록 |
| M3-06 | agent 실제 TCP legacy CREATE/DELETE 및 node-target lifecycle | 응답은 거부, INSPECT nodes/retention/native 변화 0 |
| M3-07 | 전체 workspace feature off/on과 HF Python suite | 기존 llama.cpp/HF 양쪽 회귀, failed/ignored 별도 집계 |

## 라운드

첫 완전 후보 전에 Rust/Python codec, 모든 현행 호출자와 agent legacy branch를 함께 수정하고 formatter,
문서 검사, 표적 시험 발견 수를 확인한다. 최대 3회이며 1회차는 compile+M3-01~06+회귀를 모두 포함한다.
실패 시 같은 명령을 다시 실행하기 전에 결정론적 실행 장부에 원인과 자동 차단을 추가한다. 2회차는 그
단일 원인만 수정하고 3회차는 기능 변경 없이 clean 확인과 독립 변이를 수행한다.

독립 변이는 Rust lifecycle target을 node로 되돌리기, Python shutdown에 DELETE를 다시 넣기, agent의
legacy CREATE 분기를 복원하기 중 실제 소비 시험이 검출하는 두 가지 이상을 최종 소스의 별도 worktree와
target에서 재컴파일한다.
