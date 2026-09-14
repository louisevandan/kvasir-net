# testing

tests/retained.rs는 실제 OS Python pipe를 소비한다. 모델 conformance는 HF scripts/verification/event_qwen/run.py가 P4 TCP를 통해 실행한다.

wire·예산·배포와 오류 의미는 [통합 명세](../../docs/integration/README.md)가 소유한다.

workspace 기본 병렬 시험에서는 fixture LOAD의 Python 기동 구간만 process-local mutex로 격리한다.
300ms fixture timeout은 유지한다. 준비 완료 이후 서로 다른 시험의 실행·큐 포화·취소/회수는 병렬로 진행한다.
startup_isolation 제거 변이는 기본 test thread 구성에서 기존 LOAD timeout 반례를 검출한다.
