# api

HfNodeAdapter::new(endpoint,input_capacity,completion_capacity,retained_capacity,retained_bytes)는 Arc<dyn RetainedNodeAdapter>로 소비된다. 공개 content type은 COMMAND/RESULT다.

wire·예산·배포와 오류 의미는 [통합 명세](../../docs/integration/README.md)가 소유한다.
