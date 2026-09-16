# Protocol layer

> Document status (2026-09-06): **Component guide**. This is an API and structure guide for this path. The legacy service path and the current event path are distinguished by their actual callers.
> Current goals, status and ordering follow the [execution roadmap](../../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../../docs/document-map.md).

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
