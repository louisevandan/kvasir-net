# 결정론적 실행 장부

이 문서는 반복 실행에서 얻은 사실을 다음 단계의 사전 차단으로 재사용하는 활성 장부다. 역사 보고는
원문 증거를 소유하고, 이 장부는 아직 적용 중인 교훈과 실행 전 확인 수단만 소유한다. 오류를 경험했다는
사실만 적지 않는다. 모든 항목은 원인 증거, 자동 차단 경로, 재사용 지점을 함께 가진다.

| ID | 증명된 실패 원인 | 다음 실행을 여는 자동 차단 | 현재 재사용 지점 |
| --- | --- | --- | --- |
| L001 | 이 Windows PC의 제한된 cold build도 모델 실행 없이 Kernel-Power 41 직전에 끊겼다. thread 비율은 소비 전력 상한이 아니다. | 로컬 build·model run 금지. 원격 build는 명시적 executable과 제한 jobs를 봉인한다. | 모든 Rust/native gate와 M1~M4 |
| L002 | native listener가 macOS/Linux dynamic port 범위와 겹치면 bind가 실패한다. 일부 native는 그 뒤 stdin reader join에서 멈춘다. | `tools/validate_event_runtime_preflight.py`가 모든 참여 OS의 실제 dynamic range, 고유 endpoint, 빈 node/native/listener를 검사한다. | 모델 LOAD 전부, M2/M4 |
| L003 | SSH 성공이나 한 방향 socket 연결은 광고 주소와 OUTER 반환 경로가 맞다는 증거가 아니다. | preflight 입력은 설정 그대로의 agent INSPECT 왕복에서 target/source/reply target/return route를 대조한다. | 다중 host 실기 전부 |
| L004 | 비대화형 원격 shell의 PATH에는 설치된 Cargo/Node/Python이 없을 수 있다. 같은 명령 재시도는 환경을 바꾸지 않는다. | 첫 build 전에 `command -v`와 설치 위치를 읽고 실행계획에 절대 경로를 봉인한다. Spark Cargo는 `/home/m42/.cargo/bin/cargo`다. | M1~M4 원격 시험 |
| L005 | adapter completion mailbox를 두 소비자가 poll하면 front 소유권과 byte claim이 경쟁한다. 별도 무제한 통지는 기존 boundedness를 우회한다. | EventNode만 ordinary completion을 소비한다. lifecycle 설계는 별도 소비자를 추가하기 전에 단일 front owner와 count/byte charge 이동을 시험에서 단언한다. | M1 supervisor, NL07/NL10/NL14 |
| L006 | CRLF 문서에 LF patch 조각을 삽입하면 내용이 맞아도 저장소 문서 검사가 mixed EOL로 거부한다. | patch 뒤 수정 문서 전체의 줄바꿈을 원래 형식으로 정규화하고 `node tools/scripts/docs-lint.mjs --all`을 커밋 전 실행한다. | 모든 문서 변경, M1 계획 |
| L007 | 줄바꿈 정규화 명령에 실제 CR/LF를 잘못 인용하면 기존 문서 전체가 한 줄로 합쳐지고 작은 의도 변경이 수천 줄 삭제로 커밋될 수 있다. | byte 단위 `CRLF→LF→CRLF` 정규화 뒤 line count와 `git diff --numstat`을 원본/의도 범위와 대조한다. 비정상 대량 변경은 push 여부와 무관하게 부모 원문에서 전진 복구한다. | 모든 커밋 전 검사 |
| L008 | 완성 후보 전에 formatter를 적용하지 않으면 import 순서 같은 기계적 차이가 원격 첫 gate를 불필요하게 실패시킨다. | 코드 편집 직후 formatter를 적용하고 별도 `cargo fmt --all -- --check`를 통과한 뒤에만 원격 build/test 라운드를 연다. | 모든 Rust 변경, M1 |
| L009 | Windows PowerShell→SSH→원격 Python을 한 문자열에 중첩 인용하면 원격에 도달하기 전에 로컬 parser가 코드를 명령으로 해석할 수 있다. | 여러 줄 원격 변경은 검토 가능한 스크립트 파일로 만들고 전송·준비·실행을 분리한다. 실행 전 원격 diff를 확인한다. | 독립 변이와 원격 fixture |
| L010 | 원격 검증 복사본의 `origin`이 GitHub가 아니라 임시 bundle이면 fetch가 최신 원격 추적 ref를 만들지 않으며, `git rev-parse --verify <전체 SHA>`도 object 부재를 검출하지 못한다. | 기준 commit을 검증용 bundle로 전달한 뒤 `git fetch <bundle> <SHA>`와 `git cat-file -e '<SHA>^{commit}'`을 통과해야 worktree를 만든다. | 원격 clean replay와 독립 변이 |
| L011 | 의미 변이의 교체문이 문법적으로 불완전하면 시험 단언이 아니라 컴파일 오류만 검출해 변이 증거가 되지 않는다. | 변이 diff가 한 의미만 바꾸는지 보고 formatter/check를 먼저 통과시킨다. 컴파일 실패 변이는 검출 수에 넣지 않는다. | 모든 Rust 독립 변이 |
| L012 | 원격 Cargo가 있어도 해당 toolchain에 rustfmt component가 설치됐다고 가정할 수 없다. | 실행계획 전에 필요한 component를 조회한다. baseline은 로컬 rustfmt check, 원격 변이는 한 줄 diff 검토와 `cargo check`를 거쳐 의미 시험을 실행하며 검증 중 component를 설치하지 않는다. | M1 원격 Rust 변이 |
| L013 | 한 함수 호출의 앞 인수로 owned metadata를 이동한 뒤 뒤 인수에서 같은 값을 읽으면 Rust 인수 평가 중 소유권 검사가 실패한다. | 이동 인수를 받는 오류 생성 호출 전에 진단 문자열과 비교값을 먼저 계산하고, 원격 시험 runner보다 `cargo check --tests`를 먼저 통과시킨다. | M1 supervisor와 이후 owned 오류 경로 |
| L014 | PowerShell pipeline으로 `git diff` 출력을 파일에 쓰면 patch 줄바꿈이 CRLF로 재직렬화되어 동일 base에도 `git apply`가 모든 context를 거부한다. | patch는 shell pipeline을 거치지 않고 `git diff --output=<path>`로 생성하며, 전송 전 LF/CRLF 수와 SHA-256, 원격 `git apply --check`를 기록한다. | M2~M4 원격 source 동기화 |
| L015 | 원격에 `rg`가 없는데 `rg ... | wc -l`을 실행하면 마지막 `wc`가 성공해 시험 발견 실패가 0건으로 위장된다. | 원격 runner는 `set -euo pipefail`을 켜고 사용할 도구를 `command -v`로 확인한다. 시험 수는 절대 경로 `/usr/bin/grep`으로 세고 adapter별 1건 이상을 build 전 단언한다. | M2~M4 원격 시험 발견 |
| L016 | llama.cpp worker의 blocking 첫 입력 분기와 후속 nonblocking drain이 서로 다른 후처리를 써서 첫 supervised LOAD만 input claim 퇴역 뒤 terminal 게시를 건너뛰었다. | 두 수신 분기는 `handle_received_input` 하나만 호출한다. 첫 입력으로 LOAD를 넣는 actual owned-worker 시험이 terminal·source claim0을 단언한다. | M2 llama.cpp lifecycle worker, NL01/NL07 |
| L017 | completion 포화 시험이 queue slot과 retained byte를 동시에 소진하면 의도한 게시 대기가 아니라 profile 사전 거부를 시험하며, 공용 `busy` snapshot만 기다리면 앞 입력의 claim과 다음 입력의 claim을 혼동한다. | 포화 fixture는 제한할 차원만 정확히 채우고 다른 profile 예산에는 여유를 둔다. 다음 입력 전 `queued_count=1`과 upstream claim0을 함께 단언한다. | M2 adapter response saturation, NL07/NL10 |
| L018 | HF Python frame 상한에 typed lifecycle wrapper 크기를 더하지 않으면 경계 응답의 agent terminal이 OUTER fallback으로 바뀐다. | lifecycle 여부를 native 효과 전에 판별하고 Python frame과 고정 wrapper 여유를 completion 저장소에 함께 예약한다. | M2 HF lifecycle frame boundary에서 opaque 응답은 frame 안이지만 wrapper 추가 뒤 frame을 넘는 실제 UNLOAD가 agent typed terminal로 완료됨을 단언, NL07/NL14 |
| L019 | Agent-target lifecycle로 이관하면 여러 노드의 최종 응답 source가 모두 Agent가 되어 과거의 node-source 기반 stage 식별이 성립하지 않는다. | OUTER는 전송 계층이 검증한 causation ID를 원 요청 node index에 결속한 뒤 lifecycle metadata의 node/generation을 다시 대조한다. 역순 응답과 identity 오염 시험을 유지한다. | M3 Rust event-drive LOAD/UNLOAD, NL11/NL12 |
| L020 | HF의 불명 상태 회수 `abort`를 구형 direct node 명령으로 남기면 Python child는 정리돼도 Agent의 route와 NodeOwner가 제거되지 않는다. | `abort`를 UNLOAD supervisor terminal로 분류하고 Python의 정상·실패 cleanup 모두 Agent-target NODE_UNLOAD만 사용한다. 부분 LOAD 실패가 앞 성공 node를 abort하고 owned/ready를 비우는 시험을 유지한다. | M3 HF OUTER와 lifecycle fixture, NL05/NL12/NL13 |
| L021 | PowerShell의 큰따옴표 SSH 명령 안에 둔 원격 `$()`는 백슬래시로 감싸도 로컬 PowerShell이 먼저 해석해 Linux 경로를 로컬 명령으로 실행한다. | 원격 다단계·변수·command substitution은 항상 LF runner 파일로 만들고 SHA-256을 대조한 뒤 `/bin/bash <runner>`만 호출한다. inline SSH는 `$`, 파이프, redirect가 없는 읽기 전용 단일 명령으로 제한한다. | M3~M4 원격 준비·시험·변이 |
| L022 | `git apply`로 새 파일을 만든 원격 worktree에서는 그 파일이 untracked라서 기본 `git diff`가 제외하고, 원 patch와 재생성 diff의 SHA가 달라진다. | patch 생성 전 로컬과 적용 후 원격에서 동일한 새 경로에 `git add -N`을 실행한 뒤 diff SHA를 대조하고, `git status --short`의 새 파일 목록도 봉인한다. | M3~M4 source/binary 결속과 독립 변이 |
| L023 | 같은 source diff도 Git의 `core.abbrev` 설정이 다르면 patch의 `index` 객체 ID가 7자/9자로 달라져 바이트 SHA가 불일치한다. | 로컬 생성과 원격 재생성 모두 `git diff --binary --full-index`를 사용한다. SHA 불일치 시 실행 전에 첫 상이 행을 대조하며 내용 차이로 추측하지 않는다. | M3~M4 source patch 동일성 검사 |
| L024 | Rust 시험에서 `Result<LoadedBuild, _>::unwrap_err()`는 실패만 기대해도 성공 타입에 `Debug`를 요구해 시험 실행 전 컴파일을 막는다. | 제품 타입에 시험 편의용 trait를 추가하지 않고 명시적 `match`로 오류를 추출한다. 원격 라운드는 항상 `cargo check --tests`를 첫 Cargo gate로 둔다. | M3 event-drive 부분 LOAD 회수 시험과 이후 opaque 성공 타입 시험 |
| L025 | 수신 Agent가 unknown node ingress를 거부한 실패는 그 Agent 내부에서 검출돼도 transport snapshot에서 송신자 관점 `rejected_remote`로 분류된다. | 시험은 이름을 추측하지 않고 권위 함수 `failure_state(Failure::Ingress)`와 실제 `P4_EVENT_INGRESS_FAILED`를 함께 대조한다. node/native 무효과는 별도 `nodes=[]`로 단언한다. | M3 node-target lifecycle 우회 거부, NL13 |
| L026 | HF worker의 결과가 EOF로 불명이 되면 opaque 실패의 `uncertain`은 boolean 표시가 아니라 원인 문자열을 보존한다. boolean을 기대한 새 시험은 회수 경로에 도달하기 전에 실패했다. | 불명 실패 시험은 `partial frame: EOF` 원문을 먼저 단언하고, 이어지는 supervisor abort의 typed UNLOAD `succeeded/absent`를 별도로 단언한다. 오류 관측과 자원 회수 증거를 한 값으로 축약하지 않는다. | M3 HF 불명 worker 회수, M3-05, NL05/NL13 |
| L027 | 모델 연산을 하지 않는 HF lifecycle controller 시험도 `event_pipeline`의 module-level safetensors import 때문에 표준 Python에서 수집 전에 중단됐다. 시험 runner의 “모델 의존성 없이 실행” 계약과 control/data 계층 경계가 어긋났다. | safetensors codec은 tensor를 직렬화하는 `step`에서만 import한다. 원격 runner는 전체 Python suite 전에 표준 Python으로 lifecycle module을 import하고 시험 수를 발견해 1건 이상인지 단언한다. | M3 Python Client lifecycle와 M4 환경 preflight |

새 실패를 관측하면 다음 절차를 같은 변경 안에서 끝낸다.

1. 원문 로그와 source/binary/input identity를 보고서에 보존한다.
2. 반증된 가설과 인과 경계를 한 문장으로 고정하고 새 ID를 부여한다.
3. 사람이 기억해야 하는 주의문 대신 preflight, assertion, 단위 시험 또는 실제 소비 경로 시험을 추가한다.
4. 다음 실행계획이 그 ID와 차단 경로를 참조한 뒤에만 새 라운드를 연다.
5. 차단 수단 제거 변이가 원래 실패를 다시 허용하는지 확인한다. 실행하지 못했다면 적용 중이 아니라 예정으로 표시한다.

단계 종료 때 해결된 항목을 삭제하지 않는다. 제품 경로가 사라졌거나 더 강한 검사로 대체됐을 때만
상태와 대체 ID를 기록한다. 보고서의 숫자나 당시 HEAD를 이 장부에 복사해 최신 실행 증거처럼 사용하지 않는다.
