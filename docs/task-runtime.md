# Agent task runtime

The Agent runtime has one lower-layer contract for all Controller and Node traffic. Upper handlers never wait for a reply. They validate local state, enqueue follow-up work, and return. Socket readers, socket writers, local handlers, and response forwarding are queue producers or consumers; none owns protocol policy.

## Envelope

Every queued item is a `TaskEnvelope` with `task_id`, transport `route_id`, `deadline_unix_ms`, business `correlation_id`, optional `causation_id`, source and target participants, one of the four P4 directions, request/response kind, queue class, and the complete P4 message. `Participant` carries `agent_id`, role, and logical instance ID. Equal non-empty source/target Agent IDs select the automatic in-process bypass; different IDs select a transport boundary.

`AgentInternal` exists only for adapter bootstrap messages and is not a fifth public P4 direction.

| P4 direction | Source → target | Representative messages |
| --- | --- | --- |
| `ExternalController` | external ↔ controller | `INGRESS_SUBMIT`, `INGRESS_ACCEPTED`, `INVENTORY_QUERY`, `HARDWARE_REPORT`, streamed external output |
| `ControllerNode` | controller → node | `NODE_CREATE`, `MODEL_LOAD`, initial prefill `EXECUTE`, `HEALTH_CHECK` |
| `NodeNode` | node → node | later prefill/decode `EXECUTE`; the concrete hidden-state payload remains adapter-owned |
| `NodeController` | node → controller | lifecycle/progress reports, `TOKEN`, `DONE`, `ERROR` |

## Complete message catalog

[`catalog/mod.rs`](../layers/protocol/src/catalog/mod.rs) exhaustively maps every `Message` variant. Adding a wire variant without adding its kind, correlation field, class, queue, terminal status, and allowed direction is a compile-time non-exhaustive-match failure.

| Class | Messages | Queue / terminal rule |
| --- | --- | --- |
| Request | `INGRESS_SUBMIT`, `INVENTORY_QUERY`, `ADAPTER_REGISTER`, `NODE_CREATE`, `MODEL_LOAD`, `MODEL_UNLOAD`, `EXECUTE`, `CANCEL`, `HEALTH_CHECK` | control, prefill, or decode; never terminal |
| Acknowledgement | `INGRESS_ACCEPTED` | response; non-terminal |
| Progress | `LOAD_PROGRESS`, `DRAFT_REPORT` | response; non-terminal |
| Event | `TOKEN` | response; non-terminal |
| Terminal | `HARDWARE_REPORT`, `ADAPTER_REGISTERED`, `NODE_CREATED`, `MODEL_BOUND`, `MODEL_UNBOUND`, `DONE`, `HEALTH` | response; closes the correlation route |
| Error | `ERROR` | response; terminal |

P4B1 v6 routes responses by `route_id`; business correlation IDs remain visible to controllers and adapters but never key a multiplexed socket's pending map.

## Generic workers

[`foundation/task_queue/mod.rs`](../layers/runtime/src/foundation/task_queue/mod.rs) owns four bounded lanes: control, prefill, decode, and response. Every lane is bounded by both item count and encoded bytes. Prefill therefore cannot retain an unbounded number of large prompts. The default Agent prefill budget is 1024 items / 64 MiB; control, decode, and response each use 4096 items / 16 MiB.

Each lane is one shared ready set with multiple competing generic workers. There is no correlation shard and no queue-order contract: any available worker may claim an independent task, including a task with the same `correlation_id` as work executing elsewhere. Ordering is expressed only as causality. The handler that finishes an ordered step registers its successor with that task's ID as `causation_id`. `INGRESS_ACCEPTED` therefore registers prefill `EXECUTE` only after acceptance is handled, and an adapter response chain registers the next `TOKEN` or `DONE` only after the previous response reaches its final local recipient. Independent requests never wait on that chain.

The Tokio runtime defaults to two worker threads per physical CPU core. `p4-agent LISTEN_ENDPOINT --workers N` overrides the count with `N` in `1..1024`. This value sizes the competing worker counts; it does not alter connection admission, NodeSlot permits, or concrete inference parallelism.

`TaskHandler::handle` is deliberately synchronous and non-blocking: it may update local state and call `submit`, `response`, or `follow_up`, but must not perform I/O or wait. Remote execution is launched as a Tokio I/O task; compatibility adapter calls run outside Tokio workers. Their outputs re-enter the response lane as new tasks.

## Current upper-layer boundary

[`application/dispatch/mod.rs`](../layers/runtime/src/application/dispatch/mod.rs) is the first upper layer. It translates ingress to controller/node tasks, retains NodeSlot admission through terminal output, routes every adapter output back through the response queue, and forwards controller output to the registered external connection. Co-resident adapter execution uses the same queue and handler contract without encoding or a loopback socket.

The Node.js client shares one persistent socket per Agent endpoint across ControllerInstances. The Agent host reads many routed requests from that socket and owns response routes independently. Remote execution similarly uses one persistent socket per adapter endpoint and was tested with 1024 multiplexed routes. Standalone adapter lifecycle calls may still use a compatibility one-shot socket; co-resident adapters use direct invocation.
