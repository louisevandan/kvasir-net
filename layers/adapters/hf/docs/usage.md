# 빌드와 실행

모든 명령의 작업 디렉터리는 P4 root다. fixture만 실행할 때는 torch 설치가 필요 없다.

```powershell
cargo build --locked -p p4-agent -p p4-event-drive --features hf-transformers
python layers/adapters/hf/scripts/testing/run.py
python layers/adapters/hf/scripts/deployment/worker/run.py target/hf/bundle-A --label A
python layers/adapters/hf/scripts/models/qwen3_5_0_8b/cli/run.py inspect --plan layers/adapters/hf/plans/qwen3_5_0_8b/single_cpu/plan.json
```

모델 환경은 `environments/qwen3_5_0_8b/requirements.lock`으로 고정한다. 모델 준비 도구는
`scripts/models/qwen3_5_0_8b/preparation/run.py`이며 root `.cache/hf/models/`에 revision 고정 캐시를 만든다.
모델 실행 시 해당 환경 Python을 사용하고 `--model-dir`로 실제 checkpoint를 줄 수 있다.
기본 경로 파일은 P4 root `.cache/hf/models/checkpoint-path.txt`다. 다른 checkout은 자기 캐시 또는 명시 경로를 사용한다.

자동 로딩 계획은 모델별 `profile` → `plan` → 기존 `run`/`verify` 순서다.
[자동 계획 명령](models/qwen3_5_0_8b/README.md#automatic-loading-planner)의 요청 예시에서
가용 용량과 reserve를 실행 환경에 맞춰 갱신한다. 결과는 `layers/adapters/hf/target/` 아래 새 폴더에 둔다.
환경·가중치는 Cargo가 설치하지 않는다. 상세 실행과 source 복원은 [통합 계약](integration/README.md)을 따른다.
