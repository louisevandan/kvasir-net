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

새 실패를 관측하면 다음 절차를 같은 변경 안에서 끝낸다.

1. 원문 로그와 source/binary/input identity를 보고서에 보존한다.
2. 반증된 가설과 인과 경계를 한 문장으로 고정하고 새 ID를 부여한다.
3. 사람이 기억해야 하는 주의문 대신 preflight, assertion, 단위 시험 또는 실제 소비 경로 시험을 추가한다.
4. 다음 실행계획이 그 ID와 차단 경로를 참조한 뒤에만 새 라운드를 연다.
5. 차단 수단 제거 변이가 원래 실패를 다시 허용하는지 확인한다. 실행하지 못했다면 적용 중이 아니라 예정으로 표시한다.

단계 종료 때 해결된 항목을 삭제하지 않는다. 제품 경로가 사라졌거나 더 강한 검사로 대체됐을 때만
상태와 대체 ID를 기록한다. 보고서의 숫자나 당시 HEAD를 이 장부에 복사해 최신 실행 증거처럼 사용하지 않는다.
