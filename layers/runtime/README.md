# Agent runtime layer

| Path | Purpose |
| --- | --- |
| `src/application/` | Startup, routing, and task-dispatch use cases. |
| `src/domain/` | Agent registry, NodeSlot, binding, admission, and lifecycle invariants. |
| `src/foundation/` | Handler/response contracts, direct/TCP transport, and bounded queues shared by every upper layer. |
| `src/infrastructure/` | Persistent remote-peer multiplexing. |

Application may coordinate domain and infrastructure; domain depends only on protocol and foundation, never application or peer infrastructure.
