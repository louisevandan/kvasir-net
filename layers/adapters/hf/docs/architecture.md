# 책임과 연결

`entrypoints/agent` factory → `adapter/`의 `HfNodeAdapter: RetainedNodeAdapter` → bounded IPC → 모델별 Python worker.
노드 사이에는 P4 event/broker를 사용한다. 공통 core는 모델 payload를 해석하지 않는다.
Python OUTER controller의 모델 스케줄링과 stage별 연산·cache를 Rust에서 중복 구현하지 않는다.
bridge는 용량/결과 귀속/출력 승인/프로세스 수명을 담당한다.
Python bundle 교체는 동일 agent에서 UNLOAD/DELETE 후 새 LOAD로 수행한다.
wire·소유권은 [통합 계약](integration/README.md), 코드 역할은 [구조](structure/README.md)를 따른다.
