# HF 어댑터 조립

P4 workspace member `layers/adapters/hf/adapter`를 agent의 `hf-transformers` feature로 조립한다.
기본 feature는 off다. 기존 llama.cpp는 양쪽에서 제공하며 생성·INSPECT는 동일 factory 목록을 소비한다.
모델 종류와 tensor/state 의미는 HF Python에 있고 공통 core는 이를 해석하지 않는다.

| 목적 | 문서 |
| --- | --- |
| HF 코드·API·환경·시험 | [HF 안내](../layers/adapters/hf/README.md) |
| wire·예산·배포·복원 | [통합 계약](../layers/adapters/hf/docs/integration/README.md) |
| 현재 이관 상태·원본 대응 | [이관 기록](../layers/adapters/hf/docs/migration/README.md) |
| 격리 | [계층 계약](layer-isolation-contract.md#external-hf-boundary) |
| 기존 수용 증거 | [과거 보고](../layers/adapters/hf/tests/reports/p4-integration/20260914_023000.md) |

```powershell
cargo build --locked -p p4-agent -p p4-event-drive --features hf-transformers
cargo test --locked --workspace --no-fail-fast --features hf-transformers
cargo test --locked --workspace --no-fail-fast
```

root Cargo.lock이 유일한 HF 소비 기준이다. feature off도 별도 HF checkout은 필요 없다.
HF Rust 시험은 workspace에 포함되며 표준 Python fixture가 필요하다. 모델 패키지 설치와는 별개다.
과거 두-host FP32/취소/교체/회수 결과는 당시 source의 증거다. 새 이관 source의 결과는 이관 보고를 따른다.
BF16 실패와 초대형 H0–H7/SLO 미완료는 유지한다.
