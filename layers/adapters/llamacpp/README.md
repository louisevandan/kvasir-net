# llama.cpp adapter

| Path | Purpose |
| --- | --- |
| `src/application/adapter/` | Startup, registration, NodeSlot and binding dispatch. |
| `src/application/inference/` | OpenAI-compatible request and SSE response translation. |
| `src/application/scheduler/` | Explicit pending/ready/active batch state machine: default 256 active, 1024 waiting, and max batch 256. |
| `src/application/response/` | Route-preserving response sink for one persistent P4 connection. |
| `src/domain/config/` | Adapter-owned process and binding state. |
| `src/infrastructure/http/` | Endpoint and chunked-body primitives. |

The scheduler immediately dispatches a full `P4_LLAMACPP_BATCH_MAX` batch. A backend-cycle hint dispatches a waiting partial batch; stock HTTP currently supplies that hint when one request finishes. Other partial batches consult the replaceable `BatchHeuristic`, whose current fixed wait is `P4_LLAMACPP_BATCH_LINGER_MS=0`. Future controller load and finer backend cycle telemetry belong in that heuristic/signal boundary, not in P4 messages.

The adapter supplies the released jobs as concurrent HTTP streams so stock `llama-server` can perform continuous batching. It does not invent a request-level batch option and does not override llama-server `--parallel`, `--batch-size`, or `--ubatch-size`; launch policy owns those values.
