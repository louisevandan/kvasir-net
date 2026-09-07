# P4 implementation layers

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../docs/document-map.md)를 따른다.

| Layer | Dependency direction | Contract |
| --- | --- | --- |
| `protocol/` | depends on nothing P4-specific | Wire records, codec, semantic catalog, task envelope. |
| `runtime/` | depends on `protocol/` | Agent policy, routing, task dispatch, transport abstractions. |
| `adapters/` | depends on `protocol/`; concrete runtime APIs | Backend translation only; never defines P4 policy. |

Dependencies point downward to `protocol/`; the protocol never imports runtime or adapter code.
