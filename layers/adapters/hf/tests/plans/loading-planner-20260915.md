# HF Qwen 로딩 계획기 재현 계획

생성일 2026-09-15 KST. 동시 개발 작업의 구현·실측을 인수하며 작성했다.
목표는 CPU/CUDA stage 실측과 공유 용량을 소비해 기존 Qwen 실행기가 읽는 계획을 만들고,
계획에서 약속한 prefill 한도를 실제 state 소비 지점에서도 지키는 것이다.

환경은 P4 root, 고정 Qwen3.5-0.8B checkpoint와 `.cache/hf/environments/qwen3_5_0_8b`다.
새 실행 결과는 root `target/hf/<실행명>`에 기록한다. 실행 중인550B arm의 소스·바이너리를 변경하지 않고
Rust는 별도 `--target-dir`를 사용한다. 아래 scripts 경로는 `layers/adapters/hf` 기준이다.

1. `npm run test:model-loading`: 기존 llama.cpp TS 시험과 HF Python 시험이 모두 통과해야 한다.
   `tests/models/qwen3_5_0_8b/planning/test_planning.py`의 독립 전수 탐색180사례도 포함한다.
2. 고정 환경 Python의 `scripts/verification/loading_planner_shape/run.py`로 실제 `StageSessions`의
   초과 입력 거부와 forward·active·retired 보존을 검사한다.
3. `scripts/verification/loading_planner_mutation/run.py --output <새 폴더>`에서 독립 복사본의
   reserve/disabled/shared-capacity/serial-objective/source-binding 및 두 소비 한도 제거를 검출한다.
4. 고정 환경 Python의 `scripts/verification/loading_planner/run.py --output <새 폴더>`를 실행한다.
   CPU 0–12/12–24/0–24 실측, tied weight 복제량·cache 합계·release 후 active0,
   자동 생성 계획과 기존 verify의 매 스텝 logits/greedy 및 산술·한국어 EOS를 검사한다.
5. `scripts/verification/loading_planner_event/run.py --agent <별도 agent> --plan <생성 plan>
   --scenario <동일 scenario> --output <새 폴더>`로 실제 P4 event의 logits/cache,
   UNLOAD/DELETE와 소유 agent 종료를 검사한다.
6. `cargo test --workspace --no-fail-fast --features hf-transformers --target-dir <별도 폴더>`의
   최종 exit와 모든 summary를 합산하고 docs-lint를 실행한다.

표준 출력, 첫 실패와 정리 오류, 원본·bundle hash, 명령·exit, 실제 모델 비교와 PID를 보존한다.
실측하지 않은 cut/장치·작업 크기를 외삽해 승인하지 않는다. 원격 물리 실행·SLO는 별도 수용이다.
