# 실행 API

agent의 adapter kind와 optional feature는 `hf-transformers`다. 생성·INSPECT는 동일 factory를 소비한다.
Rust 공개 타입은 `p4_hf_adapter::HfNodeAdapter`, `COMMAND`, `RESULT`다.
CREATE와 LOAD readiness는 별개이며 Python 환경·모델 파일은 CREATE가 준비하지 않는다.
구체 packet/identity/epoch/abort·예산은 [통합 계약](integration/README.md),
모델 계획과 독립 기준 실행은 [Qwen 계약](models/qwen3_5_0_8b/README.md)을 따른다.

`p4hfadapter.models.qwen3_5_0_8b.planning.plan_loading(request, profiles)`는
실행 가능한 `plan`과 자원별 합산 용량·목적값·입력 hash를 반환한다. 실패는 미측정/불일치
`ValueError`와 주어진 탐색 공간의 용량 부족 `LoadingInfeasible`로 구별한다.
CLI `profile`은 실제 stage를 측정하고 `plan`은 모델 패키지 없이 계획을 생성한다.
생성 plan의 선택적 `limits.prefill_chunk`는 scenario admission과 Python worker가 소비한다.
기존 plan에 해당 필드가 없으면 기존 context 상한을 유지한다. 상세 입력은
[자동 계획 계약](models/qwen3_5_0_8b/README.md#automatic-loading-planner)을 따른다.
