# 검증 진입점

P4 root에서 실행한다. 표준 Python fixture는 `HF_TEST_PYTHON`으로 interpreter를 지정한다.

```powershell
cargo test --locked --workspace --no-fail-fast
cargo test --locked --workspace --no-fail-fast --features hf-transformers
python layers/adapters/hf/scripts/testing/run.py
python layers/adapters/hf/scripts/verification/documents/run.py
python layers/adapters/hf/scripts/verification/package_graph/run.py --output target/hf/package-graph
python layers/adapters/hf/scripts/verification/rust_mutation/run.py target/hf/rust-mutations
```

HF Rust 시험은 두 workspace 명령에 포함된다. feature off는 agent의 등록을 끄며 workspace member 시험을 숨기지 않는다.
Python 모델/cache/epoch/변이 검증에는 고정 torch/Transformers 환경이 필요하다.
`scripts/verification/lifecycle/`는 실제 broker의 fixture 오류·회수를,
`event_qwen/`와 `event_matrix/`는 실제 모델/상태/정상응답·교체를,
`distributed_recovery/`는 두 물리 host의 반환 단절/재수용을 검증한다.
새 출력 디렉터리와 generation을 사용한다. 과거 보고를 새 commit의 실행 증거로 세지 않는다.
이관 시험은 [계획](../tests/plans/migration-20260914.md), 판정은 [이관 기록](migration/README.md)을 따른다.
