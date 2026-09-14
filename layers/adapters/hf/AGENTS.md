# HF 어댑터 작업 규칙

P4 root의 AGENTS.md와 현재 로드맵·검증 규약·격리 계약을 먼저 읽고 HF README의 해당 계약을 따른다.
이 폴더는 P4의 구상 어댑터다. 별도 Git/workspace나 인접 HF checkout을 만들지 않는다.

- Rust bridge는 retained/IPC/식별·용량·출력/child 수명을, 모델별 Python은 연산·스케줄링·KV/recurrent·codec을 소유한다.
- 모델/역할별 폴더를 유지한다. 공통 모델 framework를 선행 구축하지 않는다. 다른 adapter나 P4 공통 core로 모델 의미를 올리지 않는다.
- Rust는 P4 root Cargo.lock을 소비한다. Python 환경 lock과 모델 manifest는 HF 소유다.
- 모델은 root `.cache/hf/models/`, 환경은 `.cache/hf/environments/`, 결과는 `target/hf/`에 둔다. 신규 실행 디렉터리를 사용한다.
- 기능 변경은 실제 소비 반례와 독립 복사본의 제거 변이로 검증한다. retained claim·buffer·state·child 회수를 함께 확인한다.
- HF fixture 시험은 표준 Python이 필요하다. HF_TEST_PYTHON으로 지정할 수 있으며 부재를 PASS로 처리하지 않는다.
- docs/history와 날짜별 보고의 당시 상태·경로는 현행 계약이 아니다. BF16 실패와 미검증 범위를 유지한다.
- Windows 명령에는 Windows 경로를 사용한다. 원격 실행·push·다른 프로젝트 변경 범위를 임의 확대하지 않는다.
