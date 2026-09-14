P4의 모델별 Hugging Face Transformers 어댑터: Rust retained bridge와 Python 부분 실행.

| 목적 | 문서 |
| --- | --- |
| 범위·소유 | [개요](docs/overview.md) |
| 책임·연결 | [아키텍처](docs/architecture.md) |
| API | [API](docs/api.md) |
| 실행 | [사용법](docs/usage.md) |
| 제한 | [제약](docs/constraints.md) |
| 구현 배치 | [내부 구조](docs/internals.md) |
| 시험 | [검증](docs/testing.md) |
| wire·예산·배포 | [통합 계약](docs/integration/README.md) |
| 모델 | [Qwen3.5-0.8B](docs/models/qwen3_5_0_8b/README.md) |
| 자동 로딩 계획·실측 프로필 | [모델 계획기](docs/models/qwen3_5_0_8b/README.md#automatic-loading-planner) |
| 로딩 계획기 검증 | [계획](tests/plans/loading-planner-20260915.md) · [보고](tests/reports/loading-planner/20260915_013600.md) |
| 양자화 후보 | [설계](docs/quantization.md) |
| 작업 규칙 | [AGENTS](AGENTS.md) |
| 이관·복원·현재 증거 | [이관 기록](docs/migration/README.md) |
| 이관 검증 결과 | [보고](tests/reports/migration/20260914_120000.md) |
| Rust 공개 경계 | [crate 안내](adapter/README.md) |

저장소와 Cargo workspace는 P4 하나다. Python import명은 `p4hfadapter`, agent kind는 `hf-transformers`다.
현재 개발 순서는 [P4 로드맵](../../../docs/distributed-batching-roadmap.md)을 따른다.
