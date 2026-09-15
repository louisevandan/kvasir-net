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
  return_route: Outer      // required stable output/telemetry destination
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

### Reception agent and an OUTER reachable only through a gateway

OUTER knows the agents, nodes and placement; it selects a reception agent for
each originating request. The agent contacted for that request is its
**reception agent** (`ingress_agent`), not a topology authority or mandatory
transit for node-to-node traffic.
It can be the only network entry point into a private cluster. Other agents send
return traffic to this agent, never to an OUTER host/port. The reception agent
writes to the already accepted OUTER connection; it does not dial OUTER back.

`Endpoint::Outer` is a delivery instruction **at the reception agent**, not a
network address for the external client. `target.agent_address()` resolves to
`target.ingress_agent`. `channel` and `connection_generation` select the local
connection only after that agent receives the event. For example:

```text
OUTER C --existing connection--> reception agent A --> worker agent B
OUTER C <--same connection----- reception agent A <-- worker agent B

response.source = Node { agent: B, node, generation }
response.target = Outer { ingress_agent: A, channel: c, connection_generation: g }
response.return_route = { ingress_agent: A, channel: c, connection_generation: g }
network destination = A; local recipient at A = connection (c, g)
```

A forwarded request retains its OUTER source. Its receiving worker/relay MUST
NOT register that source as its own external socket. Only an agent whose canonical
address equals `source.ingress_agent` binds an OUTER source to a connection.
For an OUTER target, a non-reception broker selects outbound-to-reception; only
the reception broker selects its local OUTER mailbox. Changing the target to
plain `Agent(A)` would instead invoke A's agent-control handler and omit the
OUTER delivery instruction. The existing P4E3 wire layout is unchanged.

The declared reception address must identify the same agent inside the cluster;
an externally forwarded dial address is not a new logical reception identity.
Address alias discovery and peer authentication are separate contracts. This
ownership check does not authenticate a source field or authorize a reconnect.
A missing or failed reception connection retains undelivered output; another
agent must not invent a direct OUTER route, acknowledge native completion, or
replay an uncertain output. FINISH retirement is a separate, unaccepted candidate.

An intermediate agent never rewrites `source`, `target` or `return_route`.
When an adapter completes work it creates a new event with itself as source,
the exact next endpoint as target, the triggering event as `causation_id`, and
the original correlation and return route.

### Required request return context

Every valid P4 event carries `return_route`, including Agent-to-Agent and
Node-to-Node events. OUTER sets it on the initiating event; it is never inferred
from a socket, a previous message, a correlation registry or the current source.
`source=Outer` and `target=Outer`, when present, must each equal that route in
all three fields. Validation runs in the wire codec, broker admission and both
concrete adapter admission paths before request or backend effects.

The Rust field remains `Option` only to preserve P4E3 framing and represent an
unchanged refused input. `None` is invalid, including a legacy wire presence flag
of zero. Existing valid P4E3 bytes are unchanged; older senders that omitted the
route must be updated. This is stricter acceptance, not universal old-peer
compatibility. The older Chain/Hop service protocol is a separate runtime.

`p4_protocol::event::ReturnContext` owns return-route, correlation and deadline
validation and reply-envelope construction. `Envelope::next` preserves context
for a normal derivation; `ReturnContext::reply` selects one request's context
while retaining the actual causal event ID. Missing context has no source fallback.
Validation of already borrowed routes does not clone their strings.

A mixed adapter batch carries each request's original return context in its
opaque owner metadata. Its carrier envelope routes that one aggregate event;
it is not authority to replace every owner's return identity. llama.cpp keeps
the existing `ReplySpec` wire fields as an adapter codec for the common context,
validates each owner before effect commitment, and selects that owner when
publishing OUTPUT or RELEASE. Stream-level batch observations retain their
existing grouping by complete OUTER route and explicitly list the owned requests;
they do not become per-request completion events. P4 does not parse batch payloads.

HF forwards the input context to the next declared node and uses the same common
reply construction for OUTER responses. The Python OUTER reader checks target
and return route together before accepting a response into its trace.

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

### Agent inspection

OUTER sends `application/vnd.p4.agent.inspect-v1+json` to an Agent endpoint.
The agent returns `application/vnd.p4.agent.snapshot-v1+json` to the declared
return route without mutating node state. Snapshot schema `1` contains:

```text
schema, protocol_version, generated_at_unix_ms
machine {
  capability {
    os, arch
    cpu { physical_cores, logical_cores }
    memory { total_bytes }
    gpus[] { index, provider_index, uuid, vendor, name, backend, memory_kind,
             pci_bus_id, driver_version, architecture, compute_units,
             memory_total_bytes, vram_total_bytes }
    adapters[]
  }
  occupancy {
    memory { available_bytes, used_bytes }
    gpus[] { uuid, memory_used_bytes, memory_free_bytes,
             vram_used_bytes, vram_free_bytes,
             utilization_gpu_percent, temperature_c, power_draw_w }
  }
  probes {
    memory { source, state, detail }
    gpus { source, state, detail, sources[] { source, state, detail, devices } }
  }
}
nodes[] { node_id, generation, adapter_kind, state }
broker {
  sampled_at_unix_ms, state
  receipts {
    duplicate_window
    indexed, retired, allocated { events, event_bytes, payload_capacity_bytes, unmeasured_events }
    peak_allocated_event_bytes, committed_events, evicted_events, freed_events
    event_index_capacity, order_capacity, sequence_entries, sequence_capacity
  }
}
```

The node list is the agent's live registry, sorted by `node_id`. `state` is the
selected adapter's opaque, cheap snapshot; P4 and Studio must display but not
interpret backend-specific vocabulary. Hardware capability is separated from
process-local occupancy: placement may use totals, while observed free RAM,
free device memory, activity, temperature and power are diagnostic values only.
GPU discovery aggregates provider probes instead of treating `nvidia-smi` as a
vendor-neutral inventory: NVIDIA uses `nvidia-smi`, Linux AMD uses DRM/amdgpu
sysfs, and Apple uses `system_profiler`. Each source independently distinguishes
unavailable from failed. `memory_kind` distinguishes dedicated VRAM from unified
system memory. Unified devices report `vram_* = null`; their `memory_*` values are
the shared system pool and must not be added to host RAM as another capacity.
`index` is the dense backend LOAD ordinal. `provider_index` preserves a provider
identifier when it differs, such as Linux DRM `cardN`. NVIDIA GB10 devices whose
driver deliberately reports VRAM as `N/A` use the OS shared-memory observation
and remain `memory_kind=unified`. `architecture` and `compute_units` are nullable
provider facts; they are never inferred from marketing throughput tables.
Utilization, temperature, power and driver fields are nullable when the provider
cannot report them without privilege. `utilization_gpu_percent` is an activity
sample and is not SM occupancy. NVIDIA power is retained only when the same
sample supplies a positive power limit and the draw is within a small telemetry
tolerance of that limit.
Inspection does not claim model readiness, protocol-wide health, or fleet
atomicity. It supplies capacity and backend identity for placement admission;
the placement policy still needs exact model PLAN bytes, usable-memory reserves,
per-layer service calibration, selected-link latency/bandwidth, model-file
reachability and operator exclusions. A hardware snapshot alone cannot approve
an optimal distributed cut.

`broker.receipts` is an additive, backend-neutral schema-1 observation. `indexed`
means exact duplicate receipts in the count window; `retired` means evicted from
that index but still pinned by a completion-front ticket. Neither word describes
native execution, KV completion, or a request's lifecycle. `allocated` is their
sum. Dequeuing/mutating the destination Event does not retire its independent
receipt. The last receipt reference drops the Event buffers before reporting
them freed; high-water bytes include the transient insertion before count eviction.

`event_bytes` counts each exact receipt Event's inline storage and owned buffer
capacities, including payload capacity; it excludes the separately delivered
Event, indexes/keys, Arc/receipt bookkeeping, allocator overhead, native buffers,
and RSS. Index capacities are entry counts, not bytes. Unmeasurable byte totals
and counters outside the machine's numeric range are `null`, never a false zero.
The sample includes the admitted INSPECT input and precedes dispatch of its own
reply. Broker failure reports `state=failed`, `receipts=null`, and `detail`.
This O(1) snapshot leaves full Event equality, sequence scope, and count eviction
unchanged. It adds no byte admission limit, early expiry, receiver reservation,
or causal return credit. Older agents may omit `broker`.

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

`EventNode::run` returns `EventNodeFailure { error, held_input, held_output, completion_at_failure }`
on terminal failure. Every already-held direction remains owned by that result,
including a held input when output dispatch fails. Full retries keep the same
value. A completed production node task retains this result in `NodeOwner`'s
join handle instead of logging and immediately discarding it. Operational
logging reads the reason only, not the user payload in the returned value.

The raw API above remains available to legacy callers. The event entrypoint now
selects `RetainedEventBroker`, `RetainedEventNode` and an explicit
`RetainedNodeAdapter`. Input, control reply, node-held output, socket queue and
active write retain the original allocation and its count/byte claim.
Execution status and tests belong to the roadmap/evidence, not this contract.

### Event runtime storage and failure ownership

`P4_EVENT_RETAINED_BYTES` is a positive byte limit **per store**, default 256 MiB.
The root agent/OUTER/outbound stores retain at most 65536 Events each, including
dequeued owners; queue capacity remains 65536. Node CREATE accepts optional
`retained_capacity` and `retained_bytes`, defaulting to those root limits, for
its input and completion stores separately. Existing `queue_capacity` and
`completion_capacity` remain delivery-slot limits. An individually oversized
Event is refused even if its frame is below the unchanged 2 GiB wire ceiling.
Footprints include Event/buffer capacities and entry overhead; this is not an
aggregate process RSS limit or a reservation for future native results.

Control Full retains the current input and reply and retries without admitting
another control command. Permanent failure returns input, reply, unread input
store and node owners into the root-owned task result. TCP ingress Full keeps
one decoded original per connection and stops reading until admission is possible.
Pre-admission frame/decode buffers and serialization temporaries still need
separate allocation budgets; the new Event-store limits do not cover them.

Connection queues move the same owned delivery into the writer. A successful
socket write retires only that local allocation: it is not remote acceptance,
native completion, or KV retirement. A failed write retains the current original
as uncertain and keeps the closed receiver with its unstarted queue. Encoding
failure is not-started. Missing/closed routes retain unsent originals. Failed
peer connections are not silently reconnected; a failed OUTER generation stays
blocked rather than sending later events past an uncertain predecessor. Input
EOF alone keeps the OUTER writer because a peer may half-close submissions and
continue reading output. Failed owners are bounded by their store claims and
connection admission slots; the connection semaphore has 256 slots.

DELETE temporarily fences node ingress, then requires unloaded/empty/closed
semantic state, a healthy node task, and zero retained input/output counts.
Unknown completion storage is not zero. Refusal resumes the same registration;
successful deletion removes it while fenced. Queued and node-held completions
are included. This is local node deletion, not transport-wide graceful drain.

Explicit runtime/process teardown abandons local owners. Restart persistence,
remote acknowledgement/grants, native/effect output pre-reservation, independent
receipt byte caps and complete cyclic-network progress remain separate work.

The raw success path moves the original allocation to the destination and
stores an independent Event copy for exact duplicate comparison. Receiver
mutation cannot alter that receipt. This does not establish either store's
retained-byte budget. In particular, moving a producer's storage claim into
the duplicate ledger would incorrectly retain producer capacity until receipt
eviction. The owned handoff and notification/commit requirements are owned by
the [batching boundary contract](adapter-batching-layers.md#broker-책임-이전과-정확한-중복-보관--연결-시-지켜야-할-목표-계약).

### Independent completion progress

Status (2026-09-07): **unverified working candidate, implementation work paused**.
The paths below are written but have not been compiled or executed. This section
does not certify actor progress; current status is owned by the roadmap §0.

A blocked output does not impose ordering on a different `(source, correlation)`.
The node may inspect the ordinary completion front's Envelope, reserve its actual
destination slot, then remove that front only if its entire Envelope still matches.
Full or a changed front leaves the original untouched. This adds neither another
ordinary queue nor capacity, payload inspection, SESSION special cases, or scanning.
The same source/correlation cannot bypass, even when its destination differs.

The reservation is synchronous and never survives an await. Envelope validity and
sequence are checked before destination pressure. An existing ID pins an independent
exact receipt, so Duplicate/ConflictingDuplicate precede Full/Closed even if that
receipt is concurrently evicted. Commit validates the actual Event and rechecks the
ledger, current destination generation and channel. No broker lock spans mailbox
dequeue or its capacity callback; the existing raw send notification under ledger
lock is unchanged. A terminal candidate is returned as `completion_at_failure`
alongside the held input/output. A reserved front cannot be unwrapped through this
ordinary API. This removes independent-stream head-of-line blocking only: it does
not prove same-stream cycle progress, byte budgets, remote acceptance or shutdown.

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

The optional llama.cpp service policy sends
`application/vnd.p4.llamacpp.service-sample-v1+json` from a declared downstream
stage to the session head. Its load/session/execution membership and monotonic
local Frame duration are adapter-owned prediction hints, never a retirement,
remote acceptance or output receipt. P4 routes this opaque payload unchanged.
The adapter batching contract owns validation, bounded history and late-load
handling. All participating agents must support this opt-in event; no native
wire or backend capability change is implied.

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

<a id="connection-finish-candidate"></a>

## Connection FINISH — local acceptance (2026-09-15)

[Initial validation stopped after three failures](../tests/reports/release-a/20260915_044735.md).
[Deterministic review and acceptance](../tests/reports/release-a/20260915_142237.md) completed the local contract.
This additive transport candidate reserves a zero u32-LE frame length as a
connection-scoped FINISH, not an Event. The caller must consume its expected
outputs first. The agent detaches only that socket's live OUTER bindings, drains
previously acquired sender clones and queued Events, then sends a zero-length ACK.
Input EOF alone still preserves a half-closed output route. Replacement bindings
and failed-generation tombstones survive FINISH. Later outputs remain retained
as undelivered; FINISH neither cancels work nor settles requests, receipts or KV.

Old agents do not implement this ACK and old clients do not send FINISH. Do not
claim fleet repair from an agent-only upgrade. For a valid nonzero length prefix,
Rust and HF Python clients read and retain the whole first unexpected frame before
returning an error. EOF and timeout retain the received prefix/body; an oversized
declaration is refused before body allocation. The run cleanup error records the
buffered/frame size in Rust; HF retains the bytes in the client's
`finish_unexpected_output` diagnostic, including when the socket timeout itself is
returned. These diagnostic bytes are client-owned and do not approve or settle the
Event. Local actual llama.cpp/HF generation and cleanup pass. Fleet
binary replacement, uncertain-result reconciliation and multi-host acceptance remain.

<a id="acknowledged-hop-transport"></a>

## Acknowledged hop transport — Release A candidate (2026-09-15)

`P4H1` is a backend-neutral envelope inside the existing u32 little-endian
socket frame. A connection starts with `Hello`/`HelloAck`; no Event is sent
before the peer confirms protocol version, sender identity, connection
generation and bounded outstanding capacity. A legacy `P4E3` connection is
still readable during the migration window, while a new sender fails closed
against a peer that does not acknowledge `P4H1`.

Each `Data` frame contains a connection-local attempt number, SHA-256 of the
canonical `P4E3` bytes and those exact bytes. The receiver reserves a bounded
receipt record before broker dispatch. Count or byte exhaustion therefore
returns `Rejected` without broker, node, adapter or output effects. A successful
broker commit pins `AcceptedExact`; duplicate attempt plus equal digest returns
the pinned result, while a changed digest returns `Conflict` without dispatch.

The sender retains the original Event until `AcceptedExact`, then retires it
and sends `ReceiptAck`. The ACK carries the original sender identity and
connection generation so it can release a pin after reconnect. ACK loss is
idempotent. `Query` names that same identity, generation, attempt and digest.
Only `AcceptedExact` permits retirement and queue resumption; `Unknown`,
`Conflict` and rejected results remain quarantined and visible in INSPECT.
Receipt pins are bounded in memory and are not durable across agent process
restart, so a restart can yield `Unknown` and never authorizes replay.

Agent INSPECT exposes receipt count/bytes/status/age plus transport failure
count, retained Event bytes, state, age and stable `transport-N` identifiers.
`application/vnd.p4.transport.reconcile-v1+json` accepts
`{"failure_id":"transport-N"}`. It retries `not_started` originals or queries
an uncertain acknowledged generation before reconnecting. No transport receipt
is native completion, request release, output approval or KV settlement.
