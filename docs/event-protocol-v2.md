# P4 self-describing event contract

> 문서 지위 (2026-09-06): **분야 계약·구현과 구별**. 소유 분야의 계약/목표를 읽되 구현 완료로 간주하지 않는다. 현재 개발 순서와 충돌하면 로드맵의 명시적 이관을 따른다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

This is the implementation contract for the replacement P4 path. The branch
`codex/p4-pre-event-architecture-reference` preserves the previous Chain/Hop
implementation. No compatibility requirement justifies leaking that design
into this path.

## Authority

OUTER is the only topology and placement authority. It knows every agent,
node, model placement, adapter choice and inference pipeline. An agent reports
machine facts; it never turns those facts into placement or topology.

An agent has one durable kind of local state: nodes materialised by OUTER. A
node is an identity, a bounded event queue and one concrete adapter instance.
The adapter creates its backend runtime on model load and destroys it on
unload. For llama.cpp that runtime is a rank-local server object or process.

Model placement and an inference pipeline are different lifecycles:

- load is one independent command per node and contains no predecessor,
  successor or Chain;
- an inference session is created after its nodes are loaded and gives each
  participant only the routes it needs for that session;
- unloading a node invalidates every adapter session bound to that load
  generation before the backend runtime is destroyed.

## Event envelope

Every event is self-describing and routable without decoding its payload.

```text
EventEnvelope {
  protocol_version
  event_id                 // unique immutable event identity
  correlation_id           // lifecycle/request identity
  causation_id?            // event that caused this event
  source: Endpoint         // logical producer of this event
  target: Endpoint         // final consumer of this event
  return_route: Outer?     // stable output/telemetry destination
  class                    // control | data | output | telemetry
  sequence                 // monotonic within correlation + source
  deadline_unix_ms?        // admission fence, never an interrupt promise
  adapter_kind?            // selects a concrete adapter, payload stays opaque
  payload_content_type
}
```

Endpoints are values, not registry keys:

```text
Agent { address }
Node  { agent_address, node_id }
Outer { ingress_agent_address, channel_id, connection_generation }
```

`source` cannot double as a return address. A tail node receives data from the
previous node, but output belongs to the OUTER that originated the inference.
The stable `channel_id` identifies that OUTER stream; the connection generation
prevents an old socket from receiving a new stream after reconnect.

An intermediate agent never rewrites `source`, `target` or `return_route`.
When an adapter completes work it creates a new event with itself as source,
the exact next endpoint as target, the triggering event as `causation_id`, and
the original correlation and return route.

The payload is opaque bytes. P4 does not define prompt, token, max-token,
Prefill, Decode, KV, layer, tensor, batch or backend fields. The
`payload_content_type` and `adapter_kind` identify the concrete adapter
protocol that may interpret those bytes.

## Agent event loop

All participants publish by a non-blocking offer to the local AgentQueue. No
participant calls a peer, node or OUTER directly.

The agent worker reads only `target`:

1. local Agent: enqueue to the local agent-control handler;
2. local Node: non-blocking offer to that NodeQueue and finish;
3. any remote endpoint: non-blocking offer to the outbound queue and finish.

The outbound I/O pump may wait for socket readiness. Waiting is confined to
transport ownership; it never pins an agent worker or a node/adapter callback.
When a bounded queue has no credit, `offer` returns a typed overflow result.
Temporary Full returns the original event for bounded retention and retry; it
is not an inference rejection. Permanent refusal also returns ownership for an
explicit failure disposition. Sender-declared event class does not grant extra
capacity. Neither case falls back to a blocking send in the broker.

There are no response futures, paired continuations, synchronous ACK waits or
callback re-entry in P4 business code. Event loops may sleep waiting for their
own next event. A response is always another event.

Per-source sequence plus event identity provides ordering and duplicate
suppression. It is not valid to hash a transport route and infer logical
ordering from it.

## Node and adapter contract

A node reacts to two inputs: a new NodeQueue event and an adapter completion
event. It classifies the envelope and delegates payload interpretation to its
adapter. It is not a topology planner and does not advance a Chain.

The concrete adapter is stateful and owns:

- load generation and backend runtime lifetime;
- validated load configuration, capacity and backend device selection;
- inference-session routes supplied by OUTER;
- native sequence/KV ownership;
- compatibility grouping, admission, fairness and physical batching;
- concrete node-to-node protocol and its flow-control credits;
- output decoding and adapter telemetry.

P4 may provide a generic fair queue primitive, but the adapter supplies the
compatibility key, row cost and maximum physical cost. Only the adapter may
decide that two events can share a physical batch.

Adapter entry points enqueue and return:

```text
try_offer(AdapterEvent) -> Accepted | Full(original Event) | Closed(original Event)
try_take(AdapterProducedEvent) -> Empty | Event
snapshot() -> opaque, cheap, non-blocking status
```

The adapter never borrows an event sink whose implementation may block. It
owns a bounded completion queue drained by the node event loop.

### Local refusal ownership — limited implementation boundary

The canonical `EventBroker::dispatch` returns `DispatchFailure { error, event }`
on **every** refusal: invalid envelope, conflicting duplicate, sequence
regression, missing/stale route, poisoned lock, destination Full or Closed.
`event` is the original boxed value and retains its allocation capacities.
It is not a reconstructed duplicate. Registration/unregistration have no input
Event and continue to return the pure `DispatchError` reason.

`EventNode::run` returns `EventNodeFailure { error, held_input, held_output }`
on terminal failure. Both already-held directions remain owned by that result,
including a held input when output dispatch fails. Full retries keep the same
value. A completed production node task retains this result in `NodeOwner`'s
join handle instead of logging and immediately discarding it. Operational
logging reads the reason only, not the user payload in the returned value.

This is **raw Event failure ownership**, not the completed retained-byte/credit
handoff. It does not drain unread queues, acknowledge remote receipt, preserve
state across process restart, or guarantee graceful shutdown. Explicit task
abort/handle disposal can still discard local ownership. The current agent
control reply loop and remote pumps do not yet retain all terminal failures;
returning the Event from the broker alone does not close those consumers.
Execution status and tests belong to the roadmap/evidence, not this contract.

The raw success path moves the original allocation to the destination and
stores an independent Event copy for exact duplicate comparison. Receiver
mutation cannot alter that receipt. This does not establish either store's
retained-byte budget. In particular, moving a producer's storage claim into
the duplicate ledger would incorrectly retain producer capacity until receipt
eviction. The owned handoff and notification/commit requirements are owned by
the [batching boundary contract](adapter-batching-layers.md#broker-책임-이전과-정확한-중복-보관--연결-시-지켜야-할-목표-계약).

## Node lifecycle

### Create

OUTER targets an Agent with an opaque create payload and `adapter_kind`. The
agent creates only `NodeQueue + adapter object`. Success or failure is a new
event to `return_route`.

### Load

OUTER targets each Node independently. The payload completely specifies the
concrete adapter load: artifact, backend/device policy, contiguous layer
window, context and batch capacities, cache policy and adapter protocol
version. P4 stores and forwards bytes without interpreting them.

The adapter validates the complete plan before allocation, creates the
rank-local runtime, and emits progress and one terminal Loaded/LoadFailed
event. A Loaded event carries an opaque capability document and a new load
generation. No inference event is admitted before Loaded.

### Unload and delete

Unload first closes admission for the named load generation. Already emitted
adapter events may drain, but no new physical work begins. The adapter emits
session failures, releases sequence state, stops its runtime, releases model
and context allocations, and then emits Unloaded. Delete is accepted only for
an unloaded node.

Unload completion is an event, never the return from a call waiting on process
exit.

## Inference-session projection

OUTER creates a correlation id and sends an individual opaque adapter event to
every participating node. It does not send a shared P4 Chain.

For a linear llama.cpp pipeline the adapter payload projects the topology:

| Role | Required session routes |
| --- | --- |
| first | next Node, OUTER return route |
| middle | next Node, OUTER return route |
| last | first Node for decode continuation, OUTER return route |

The adapter derives expected predecessor/first/last roles from the declared
session; an incoming source field alone is not execution authority. The tail
produces engine results, but head settlement approval owns user-visible output
authorization. P4 does not infer either from routing or a queue acknowledgement.
The exact session, settlement and output rules are owned by the
[adapter batching contract](adapter-batching-layers.md). Monitoring remains
separate from user output.

Prefill input and decode continuation are distinct operations in the llama.cpp
adapter protocol. They are not P4 queue classes. Another adapter may use a
different vocabulary without changing P4.

## llama.cpp boundary

### Backend neutrality

The adapter selects devices through `ggml_backend_dev_*`,
`llama_model_params.devices`, buffer types and stock llama.cpp backend
registration. P4 and the pipeline scheduler must not include or call CUDA,
HIP, Vulkan, Metal, OpenCL or RPC implementation headers/symbols.

A platform pack may link or dynamically load any stock backend. Backend
selection is data in the load payload and a device capability result, never a
compile-time branch in the P4 adapter. Backend-specific diagnostics are
allowed only behind a backend plugin interface.

### Partial model loading

Stock llama.cpp owns architecture interpretation. The compatibility layer may
add an architecture-neutral contiguous layer window to `llama_model_params`
and expose exact graph cut tensors, but it must not add model-name switches or
duplicate transformer graph construction.

For stage `[begin,end)`:

- input tensors are resident only when `begin == 0`;
- repeating-layer weights are resident only for block ids in the window;
- output/terminal tensors are resident only when `end == n_layer`;
- non-resident tensors may keep shape/type metadata only and have no backend
  buffer or GGUF data mapping;
- the compute graph contains only owned layer operations plus exact cut inputs,
  cut outputs and required stage-local memory side effects;
- KV, recurrent, hybrid and encoder memory ownership is derived from the stock
  memory object and graph, not tensor-name guesses;
- aliases and shared storage cross a cut as one component or the plan is
  rejected;
- cut descriptors preserve type, dimensions, strides, bytes and alias
  relationships exactly;
- unsupported memory or graph shapes fail load before readiness.

The public extension must be per-model/per-context parameters. A mutable
process-global configuration keyed by model path is forbidden: it is not
re-entrant and cannot represent two runtimes loading the same artifact with
different windows.

Every unavoidable private dependency lives in the versioned compatibility
patch set. MoE expert dispatch, transport, scheduling and backend-specific
registration are separate features and cannot be bundled into the minimal
partial-load ABI.

### Physical mixed batching

llama.cpp's official server is the scheduling reference:

1. collect one sampled decode row for every compatible generating slot;
2. when continuous batching is enabled, fill remaining `n_batch` capacity
   with pending prompt rows;
3. compatibility includes task mode, token-versus-embedding shape and equal
   LoRA configuration;
4. each row carries its position, sequence memberships and output mask;
5. submit one logical `llama_batch`; let llama.cpp split it into physical
   `n_ubatch` invocations;
6. forward the exact physical invocation metadata and complete cut-set, not a
   reconstruction from the logical plan;
7. downstream stages execute that same physical capsule; the tail samples only
   rows whose output mask is set;
8. update request cursors and credits only after the corresponding completion
   event returns.

Decode rows have reservation priority so active generation cannot starve
behind long Prefill work. Residual capacity is water-filled across Prefill
requests with a rotating cursor. A compatibility group is never split or
merged by P4.

The unit of node-to-node flow control is one physical capsule. A credit is
released only by its terminal adapter event. Queue acceptance is not compute
completion.

The first adapter emits a telemetry event for every accepted logical batch.
Its payload records every physical callback `execution_id`, row count,
Prefill/Decode row count, request count, sequence count and per-request row
ownership. `mixed_physical_batches > 0` therefore proves an actual llama.cpp
physical UBATCH contained both phases; a scheduler plan or aggregate request
overlap is not accepted as that proof. OUTER de-duplicates observations by
`observation_id` and derives each exact prompt-token count by summing that
request's observed Prefill rows.

## Deterministic failure conditions

The implementation must reject rather than guess when any of these is true:

- unknown endpoint/protocol/content type or stale connection generation;
- duplicate event with different bytes or non-monotonic source sequence;
- node absent, not loaded, wrong load generation or session absent;
- incomplete session projection or target not equal to its declared route;
- adapter queue/capsule credit exhausted;
- incompatible rows proposed for one batch;
- prompt plus generation budget exceeds the per-sequence context;
- invalid physical invocation metadata or logical row without one exact owner;
- cut descriptor, tensor bytes, alias component or physical row mismatch;
- non-owned tensor has a backend buffer or remains reachable by the stage graph;
- stage-local memory cannot represent the stock model's memory kind;
- backend runtime reports load/decode/transport failure.

Semantic adapter rejection owes a telemetry/failure event addressed to the
valid declared OUTER return route. This is not a promise that a full or closed
transport can deliver such an event: its storage and failure disposition must
be preserved independently. Temporary queue Full must not be converted into a
new semantic failure event, and a transport callback must not interpret payloads.

## Proof order

Static proof precedes execution:

1. unchanged upstream commit and compatibility patch hash;
2. no model/architecture/backend-name branches outside stock llama.cpp;
3. full graph versus one-stage cut equivalence for descriptors and logits;
4. adjacent stage cut descriptor and state-transition equivalence;
5. stock versus staged model/context allocation ownership audit;
6. mixed logical batch to physical UBATCH bijection audit.

Runtime gates then apply in order:

1. one request, one-item buffer, the same P4/adapter/physical-UBATCH/terminal
   path used under load; retain and manually judge the complete response;
2. concurrent correctness on that proven path;
3. mixed-arrival performance with prompt/response artifacts and per-stage
   failures retained.

The requested performance experiment is Gate 3: context 1.2k, parallel 10,
approximately 500 input tokens and maximum 500 output tokens, 20 simultaneous
starts followed by 10 more every 30 seconds to 50 total. It is not run until
Gates 1 and 2 prove semantic correctness. Throughput never converts an
incorrect response into a pass.

`tools/event-drive` accepts either one prompt/template or an exact prompt list.
Each request artifact retains its submitted prompt, decoded response, physical
Prefill/Decode row totals, arrival/completion time and token outcomes. The
reproducible local wrapper at `../test/benchmarks/p4-event-gate/run.mjs` derives
the agent address from the run config, waits for READY, samples every NVIDIA
device at 250 ms, preserves raw and summarized GPU evidence, and cleans up the
agent on both success and failure.

### 2026-08-25 local proof snapshot

The backend-neutral source built with the stock CUDA backend and passed all 11
native tests. The same two-stage `[0,18) -> [18,28)` path then passed:

- Gate 1: one request, exact coherent answer, 16 Prefill rows and 7 Decode rows;
- Gate 2: 10 simultaneous requests, 10 complete and released exact answers;
- Gate 3: 50 requests at the required 20/10/10/10 arrivals, every prompt
  exactly 500 model tokens, all responses manually correct and EOG-complete,
  and every sequence released;
- two physical UBATCHes contained Prefill and Decode together: `63 + 1` rows
  in a 64-row UBATCH and `53 + 3` rows in a 56-row UBATCH;
- peak observed VRAM was 1,599 MiB on the RTX 3090 and 3,210 MiB on the RTX
  4080, below the declared local limits.

The exact final artifacts are `target/p4-event-v2/gate1-cuda-artifact.json`,
`gate2-cuda-artifact.json`, and `gate3-cuda-final-artifact.json`, with adjacent
resolved config, GPU CSV/JSON and process logs. Their immutable hashes and the
two mixed physical UBATCH row maps are retained in
[`test/benchmarks/p4-event-gate/proof-2026-08-25.json`](../test/benchmarks/p4-event-gate/proof-2026-08-25.json).
These ignored runtime artifacts prove local mixed-batch ability; they do not by
themselves prove remote/TUF operation, every llama.cpp backend, or a globally
optimal placement.

## Replacement map

The following legacy concepts have no representation in this contract:

- P4 `Chain`, `Link`, `advance`, `restart`, Hop and Lap;
- P4 `Execute { prompt, max_tokens }` and Prefill/Decode queue semantics;
- relay-side submission-route reconstruction;
- a blocking adapter event sink or blocking queue fallback;
- model Load encoded as a one-link inference chain;
- a concrete runtime that directly links CUDA/HIP symbols in its core.

They remain available only on the reference branch and must be removed from
the active implementation rather than wrapped.
