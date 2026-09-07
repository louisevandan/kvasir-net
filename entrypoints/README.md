# Process entrypoints

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../docs/document-map.md)를 따른다.

`agent/`, `controller/`, and `node/` are thin launchers. They parse no backend configuration and contain no protocol, lifecycle, routing, or transport policy beyond calling the runtime's public entrypoints.
