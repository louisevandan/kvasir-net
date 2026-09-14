# 실행 API

agent의 adapter kind와 optional feature는 `hf-transformers`다. 생성·INSPECT는 동일 factory를 소비한다.
Rust 공개 타입은 `p4_hf_adapter::HfNodeAdapter`, `COMMAND`, `RESULT`다.
CREATE와 LOAD readiness는 별개이며 Python 환경·모델 파일은 CREATE가 준비하지 않는다.
구체 packet/identity/epoch/abort·예산은 [통합 계약](integration/README.md),
모델 계획과 독립 기준 실행은 [Qwen 계약](models/qwen3_5_0_8b/README.md)을 따른다.
