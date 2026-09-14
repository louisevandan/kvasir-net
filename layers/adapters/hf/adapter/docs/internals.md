# internals

in-flight Input과 held Completion 모두 claim을 유지한다. worker 실행 전 completion을 예약하며 capacity listener로 재개한다. façade와 child의 수명을 분리한다.

wire·예산·배포와 오류 의미는 [통합 명세](../../docs/integration/README.md)가 소유한다.
