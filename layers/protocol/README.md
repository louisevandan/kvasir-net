# Protocol layer

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../docs/document-map.md)를 따른다.

| Path | Purpose |
| --- | --- |
| `src/contract/` | Message, phase, execution, error, and wire identity records. |
| `src/codec/` | Header framing, message kinds, bounded fields, payload encode/decode. |
| `src/catalog/` | Queue, terminal, correlation, and direction semantics. |
| `src/task/` | Self-describing transport-neutral task envelope. |

This crate is runtime-neutral and must not import Agent or adapter concepts.
