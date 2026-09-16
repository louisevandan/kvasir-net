# P4 implementation layers

> Document status (2026-09-06): **Component guide**. This is an API and structure guide for this path. The legacy service path and the current event path are distinguished by their actual callers.
> Current goals, status and ordering follow the [execution roadmap](../docs/distributed-batching-roadmap.md); document authority and reading paths follow the [document map](../docs/document-map.md).

| Layer | Dependency direction | Contract |
| --- | --- | --- |
| `protocol/` | depends on nothing P4-specific | Wire records, codec, semantic catalog, task envelope. |
| `runtime/` | depends on `protocol/` | Agent policy, routing, task dispatch, transport abstractions. |
| `adapters/` | depends on `protocol/`; concrete runtime APIs | Backend translation only; never defines P4 policy. |

Dependencies point downward to `protocol/`; the protocol never imports runtime or adapter code.
