# 외부 HF 어댑터 소비 구성

P4는 `hf-transformers` feature로 인접 HF의 `crates/p4-hf-adapter`를 정적으로 조립한다.
기본 feature는 off이며 기존 `llamacpp`는 두 구성 모두 제공한다.
생성과 INSPECT는 entrypoint의 같은 factory 목록을 소비한다. model 이름/torch/tensor/state는 공통 core에 들어오지 않는다.

| 목적 | 문서 |
| --- | --- |
| 작업 범위·완료 게이트 | [개발 계획 §0](external-analysis-improvement-plan.md#hf-integration) |
| 현재 실행 순서 | [로드맵](distributed-batching-roadmap.md#current-status) |
| 검증 계약 | [HF 검증](distributed-batching-verification.md#hf-integration-contract) |
| wire·환경·배포·명령 | [HF 통합 명세](../../p4hfadapter/docs/integration/README.md) |
| 실패 반례·실행 결과·미완료 | [HF 수용 보고](../../p4hfadapter/tests/reports/p4-integration/20260914_023000.md) |

```powershell
cargo build --locked -p p4-agent -p p4-event-drive --features hf-transformers
cargo test --workspace --no-fail-fast --features hf-transformers
cargo test --workspace --no-fail-fast
cargo metadata --locked --format-version 1
```

optional path 의존성이므로 feature off도 HF checkout/source가 필요하다. 두 workspace를 합치지 않는다.
P4 root lock은 최종 agent 빌드, HF lock은 외부 bridge 독립 시험에 사용한다.
metadata에서 p4-adapter/p4-protocol package ID가 각각 하나여야 한다.
HF의 build 도구는 정확한 두 commit을 source archive로 내보내고 독립 디렉터리에서 --locked 재현 빌드한다.

CREATE 성공은 Python 환경/모델 준비 완료가 아니다. LOAD readiness와 실제 요청/회수/UNLOAD/DELETE로 검증한다.
동일 agent 바이너리에서 Python bundle 교체와 기존 llama.cpp 정상 실행을 별도로 증명한다.
현재 구현 중간 지점이며 전체 수용 판정은 위 보고의 최종 결과를 따른다. 로컬 작은 Qwen의 성공은 H0–H7 승격이 아니다.
