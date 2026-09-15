# Protocol layer

> 문서 지위 (2026-09-06): **구성요소 안내**. 해당 경로의 API·구조 안내다. 과거 service 경로와 현재 event 경로는 실제 호출자로 구분한다.
> 현재 목표·상태·순서는 [실행 로드맵](../../docs/distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](../../docs/document-map.md)를 따른다.

| Path | Purpose |
| --- | --- |
| `src/event/` | Current P4E3 endpoint/envelope, required `ReturnContext`, codec and validation. |
| `src/envelope/`, `src/frame/` | Older service protocol identities and framing; separate from P4E3. |
| `src/lane/`, `src/return_channel.rs` | Service queue classes and return-channel records. |
| `src/error/` | Protocol errors. |

This crate is runtime-neutral and must not import Agent or adapter concepts.

The [event contract](../../docs/event-protocol-v2.md#required-request-return-context)
defines OUTER-selected reception and per-request return identity. `Envelope::next`
inherits it; `ReturnContext::reply` selects a request owner for a reply. P4E3
retains its wire layout but refuses absent or contradictory return routes.
