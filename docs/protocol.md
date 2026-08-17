# P4 protocol contract and open decisions

This document audits the current P4 protocol. It separates behavior proved by
code and tests from behavior that is only intended or still needs a policy
decision.

## Audit status

| Area | Current status | Decision required |
| --- | --- | --- |
| Frame routing | Implemented for routable TCP addresses | Define connection/session identity for multiple OUTER clients |
| Reply return path | Direct reply_to works; chain is one-hop fallback | Make origin agent and return channel explicit |
| Per-route ordering | Implemented by route-hashed workers | Separate transport request identity from backend sequence identity |
| Pipeline overlap | Local node windows and independent node hops exist | Define global admission/credit and feed guarantees |
| Queueing | Agent lanes, peer queues, and node ingress are bounded; adapter event/outbox channels remain unbounded | Bound remaining channels and expose spill/rejection policy |
| Monitoring | Human-readable status and backend report are relayed | Define typed, correlated snapshots and event sequencing |
| Generation options | Opaque request/options text is carried to the adapter | Keep semantics outside P4; define only preservation and size/error policy |
| KV persistence | Persist/restore/fork/discard verbs exist in the mock | Define multi-stage atomicity, ownership, and request-to-KV mapping |

## 1. Frame and return topology

### Current identity fields

| Field | Current meaning | Source |
| --- | --- | --- |
| target | Address of the next consuming agent | layers/protocol/src/envelope/mod.rs |
| recipient | Agent or node name at that address | layers/protocol/src/envelope/mod.rs |
| route | Correlation key for continuations and stream replies | layers/protocol/src/envelope/mod.rs, layers/agent/src/continuation/mod.rs |
| reply_to | Address to which reply frames are sent | layers/protocol/src/envelope/mod.rs |
| chain | Ordered stage addresses and node identities | layers/protocol/src/envelope/chain/ |
| deadline_unix_ms | Hop-boundary deadline | layers/protocol/src/envelope/mod.rs |

reply_to is the normal return target. A node targets reply_to, and an agent
forwards the frame if the target is not local. The chain is retained on the
response so the outbound pump can use the first chain link as a single fallback
when the direct target cannot be reached. It is not the normal return route.

Relevant code: [Envelope::to_reply](../layers/protocol/src/envelope/mod.rs),
[Peers::hand_on](../layers/agent/src/transport/outbound/mod.rs), and
[Envelope::relay_home](../layers/protocol/src/envelope/mod.rs).

### Unresolved OUTER connection identity

An address identifies a TCP listener, not an individual OUTER connection or
subscription. Continuations is keyed only by route. The current code therefore
assumes:

1. reply_to is stable and reachable for the intended OUTER.
2. The OUTER listener demultiplexes every route arriving at that address.
3. A route is globally unique for the lifetime of all possible returners.
4. Reconnection retains the listener identity or creates a new route.

Multiple OUTER clients behind one listener, NAT, connection migration, or route
reuse can therefore cause misdelivery or an apparently orphaned reply.

The protocol needs distinct identities:

    request_id       stable identity of one inference request
    origin_agent     agent that accepted the request from OUTER
    return_channel   address plus OUTER subscription/connection identifier
    sequence_id      backend/KV identity for one live conversation
    hop_id           one execution pass at one node

Minimum policy: origin_agent is the return anchor; return_channel is a logical
channel rather than an inferred socket; request_id is immutable; route is
transport-only; responses carry request/sequence/hop identity; and disconnect
behavior is explicitly buffer, redirect, or cancel.

## 2. Inference lifecycle

The current path is:

    OUTER -> origin agent -> route worker -> destination agent/node
          -> node window -> adapter hop -> event sink
          -> next node or reply_to -> origin agent/direct OUTER -> OUTER

The worker does not wait for adapter completion. A node starts its next hop
after an adapter event. This is implemented by
[Adapter::start](../layers/adapters/adapter/src/lib.rs) and
[Node::on_event](../layers/agent/src/node/runner/events.rs).

The missing contract is the state machine:

    accepted -> queued -> running -> yielded -> queued
                             \-> completed
                             \-> failed
                             \-> cancelled
                             \-> expired

Every transition needs request ID, stage, hop ID, timestamp, and a monotonic
event sequence. Duplicate events must be idempotent; events from an old hop or
generation must be rejected.

## 3. Pipeline-parallel scheduling

Each node has one active hop at a time. A hop can contain multiple sequences up
to the declared ceiling. compose chooses one lane and forms a window; the node
calls the adapter and waits for HopComplete before draining again.

This permits overlap across nodes:

    time ->
    stage 0: request A | request B | request C
    stage 1:           request A | request B | request C
    stage 2:                     request A | request B | request C

Current tests prove batching, per-node single-hop execution, sustained mock
arrivals, and overlapping mock stages:
[queues.rs](../layers/agent/tests/queues.rs) and
[pipelining.rs](../layers/agent/tests/pipelining.rs).

The wire does not define stage credits, readiness, min/max batch width,
prefill/decode service guarantees, queue admission time, downstream
backpressure, continuous-batching ownership, or backend reorder rules.
Event-driven feeding avoids polling but does not itself prove GPU utilization.

Required invariants:

1. One hop_id executes at most once at one node.
2. A node never exceeds adapter-admitted concurrency or batch ceiling.
3. Different requests may occupy different stages simultaneously.
4. A request holding KV cannot be starved indefinitely by fresh prefill.
5. Downstream saturation creates backpressure or explicit rejection, never an
   unobserved unbounded buffer.
6. A stage reports idle, queued, running, and blocked distinctly.
7. Utilization claims include adapter/device activity, not queue depth alone.

## 4. Queueing and backpressure

Agent lanes, per-peer queues, node work ingress, and NodeQueue are bounded.
Node ingress uses the agent `Budget.depth` and returns an explicit refusal when
its capacity is exhausted. The adapter event channel and node outbox remain
unbounded in [Node::spawn](../layers/agent/src/node/runner/mod.rs), so the
protocol is not yet allowed to claim that every intermediate buffer is bounded.

The policy must choose:

| Policy | Meaning |
| --- | --- |
| Reject | Return queue_full with capacity and retry guidance |
| Wait | Keep the sender under bounded backpressure |
| Spill | Persist an append-only FIFO segment under a byte/quota limit |
| Drop by deadline | Remove expired work before adapter admission |

The safe default is bounded RAM + bounded optional spill + explicit refusal.
Spill must expose RAM depth, spill depth, oldest enqueue time, quota, and state.

## 5. Monitoring contract

The current Status reply is a human-readable string with lanes, peers, node
depth, routes, and an opaque backend report. It is not a stable machine
protocol. It omits intermediate queue depths, hop identity, lifecycle state,
event loss, and queue refusals.

The replacement status schema should contain:

    agent:
      id, address, uptime
      accepted, forwarded, refused, completed, failed
      queue depths/capacities by lane
      peer count and peer queue depths
    nodes[]:
      node_id, adapter_kind, distribution
      deployment_id, generation
      phase: loading|ready|prefill|decode|unloading|failed
      capacity, in_flight, batch_width
      queue_ram, queue_spill, oldest_wait_ms
      active request_id/sequence_id/hop_id
      last_progress, adapter_report, event_loss
    requests[]:
      request_id, origin_agent, return_channel
      sequence_id, stage, hop_id, phase
      state, enqueue_time, start_time, last_progress, deadline

Snapshots need snapshot_seq and generated_at. Consumers must distinguish zero
from not reported. Backend text may remain opaque, but structured adapter
counters must be separate fields.

## 6. Generation options and portability

Generation semantics do not belong to the abstract P4 protocol. OUTER owns the
backend-specific knowledge and serializes the prompt, sampling/decoding
parameters, grammar, structured-output request, MTP/speculative settings, and
other launch or request switches into the request body. P4 carries that content
opaque through the chain. The adapter receives the same opaque content and
translates it to the concrete llama-server/API invocation.

The abstract contract is therefore only:

1. preserve the opaque generation content byte-for-byte unless the protocol
   explicitly defines a transformation;
2. preserve it across every hop and decode lap;
3. enforce frame/body size and transport validity limits;
4. never silently reinterpret or drop fields;
5. return an adapter-owned error when the concrete backend cannot parse or
   apply the content.

The current standard service vocabulary exposes prompt, max_tokens, and options
as separate fields, while the served adapter merges options into its
OpenAI-compatible request. That is a service/adapter implementation choice, not
an abstract P4 requirement. If OUTER sends a single serialized generation
request, the service layer must preserve that representation instead of
inventing a second P4-owned schema.

MTP/speculative decoding, grammar, sampling, stop rules, tokenizer/template
selection, and launch switches are consequently OUTER-to-adapter contracts.
P4 may transport their opaque text and report an opaque adapter result, but must
not claim semantic support or define a competing common option schema.

## 7. KV cache and request mapping

The adapter contract has SequenceId, but the service currently derives it from
Envelope.route in [Bodies::sequence](../layers/service/src/payload/mod.rs).
Cache operations carry a sequence string and support persist, restore, fork,
and discard.

Proven by current tests:

- the mock can persist and restore progress;
- fork creates an independent persisted identity;
- cache operations can be sent to every stage in a deployment test;
- caching does not change model binding.

Not specified or proven:

- route reuse protection after restart;
- request-to-sequence mapping when a request forks;
- stage-local versus global conversation identity;
- model/generation/tokenizer compatibility on restore;
- atomicity across all pipeline stages;
- crash recovery between freeing resident KV and durable commit;
- eviction, quota, checksum, encryption, and cache format version;
- movement of a sequence between nodes;
- persistence of MTP/speculative state.

Use an explicit immutable sequence_id in inference and a separate request_id for
each execution. A cache manifest should bind:

    sequence_id, deployment_id, generation, model_fingerprint
    tokenizer_fingerprint, adapter_kind, stage_id, cache_format_version
    created_at, last_used_at, bytes, checksum

Restore must validate the full manifest before publishing the sequence as
available. Multi-stage restore needs an all-stage success barrier; partial
restore must remain hidden and be cleaned up or marked recoverable.

The served llama.cpp-compatible adapter cannot persist sequence state through
its HTTP surface and returns failure. That limitation should be reported as an
adapter capability, not discovered only after a cache command.

## 8. Decisions required before implementation

1. Is origin_agent mandatory, or is reply_to formally the origin return channel?
2. Can one OUTER address contain multiple independently identified connections?
3. Are request_id, route, and sequence_id distinct mandatory fields?
4. Is event ordering per request, per hop, or best effort?
5. Is queue overflow rejected, blocked, or spilled, and who owns quota?
6. Is continuous batching a P4 guarantee or only an adapter capability?
7. What runtime evidence qualifies a pipeline stage as non-idle?
8. Which generation options are common fields versus opaque extensions?
9. Is MTP/speculative decoding negotiated and persisted?
10. Is KV restore transactional across stages, and which compatibility fields are mandatory?

Until these decisions are recorded and tested, P4 should be described as a
working framed routing and mock-pipeline implementation, not a complete
durable multi-OUTER inference protocol.

## 9. Four-agent cross-check synthesis

The independent reviews are recorded in:

- [procotolcheck1.md](procotolcheck1.md): routing, return path, OUTER identity,
  and streaming continuation.
- [procotolcheck2.md](procotolcheck2.md): pipeline scheduling, CPS, fairness,
  queueing, and backpressure.
- [procotolcheck3.md](procotolcheck3.md): generation options, sampling,
  structured output, MTP/speculative decoding, and served/staged boundaries.
- [procotolcheck4.md](procotolcheck4.md): KV identity, persistence,
  multi-stage atomicity, event correlation, and monitoring.

The reviews agree on the following implementation contradictions that override
the more optimistic parts of this document.

### 9.1 Return anchor is not guaranteed

relay_home selects the first link in the inference chain, but that link is not
necessarily the agent that accepted the request from OUTER. A topology with an
ingress/door agent in front of the chain disproves that assumption. The
protocol must carry an explicit origin return anchor; the forward chain cannot
also be assumed to be the return chain.

The current direct reply path is only proven for a stable, directly routable
reply_to address. It is not proven for multiple OUTER clients, connection
migration, or an OUTER disconnect during generation. The outbound send result
also means queue acceptance/write attempt, not end-to-end delivery
acknowledgement.

### 9.2 Streaming continuation is one-shot

Continuations stores one FnOnce handler per route and removes it on the first
response. Inference produces Token frames followed by Done. A continuation
registered for a streaming inference can therefore consume the first Token and
leave later Token/Done frames without the registered continuation.

The protocol must distinguish terminal one-shot reply from a streaming
subscription. A stream needs its own stream_id, ordered event sequence, bounded
response queue, idle timeout, and cancellation policy.

### 9.3 Route is overloaded

The standard service payload derives SequenceId from Envelope.route. The same
route is also the worker shard key, continuation key, cancellation key, and
response grouping key. This creates collision and stale-state risks on route
reuse, retries, reconnects, and forked conversations.

The mandatory identity split is:

    request_id      one execution request
    stream_id       one streaming response set
    sequence_id     durable backend/KV conversation state
    origin_agent    ingress return anchor
    channel_id      OUTER logical subscription or connection
    hop_id          one execution pass at one node
    event_seq       monotonic event order

route may remain a transport ordering/sharding key, but it must not be the
implicit KV identity.

### 9.4 Options are opaque outside P4

The independent reviews correctly found that the current served adapter merges
the received options into its concrete request and that malformed or
backend-incompatible options need an adapter-owned error policy. That is not a
missing abstract P4 sampling schema. It is a boundary contract between OUTER
and the selected adapter.

P4 must not validate temperature, top_p, grammar, MTP, speculative decoding, or
llama-server switches. It must preserve the serialized content, keep it tied
to the request/sequence identity, and surface an adapter rejection without
silently rewriting the content. OUTER is responsible for producing a valid
string for the selected adapter/backend and for knowing which options it asked
for.

### 9.5 Token accounting is not stable

The served path streams text chunks rather than tokenizer token IDs. A chunk
may contain multiple tokenizer tokens. The normal payload also creates
Sequence.position as zero on each hop, while the adapter session maintains its
own delivered count. A stop path can therefore report a generated count that
does not reflect the actual stream length.

The protocol must define whether position means backend token count, emitted
delta count, or chunk count. The final response should carry backend counts and
P4 sequence numbers when they differ.

### 9.6 Mock pipeline overlap is not served GPU pipeline proof

The node window and mock tests prove that different nodes can overlap different
requests and that a node does not exceed its declared window. The current
served adapter is Distribution::Internal, opens one HTTP stream per sequence,
and relies on backend batching. The staged llama.cpp Rust adapter is not present
in the workspace.

The current protocol therefore proves mock scheduling, not that P4 feeds a real
staged GPU whenever it becomes available. A staged capability must expose stage
credits, readiness, input/output batch width, KV residency, and last progress.
Evidence must include adapter/device activity rather than only queue depth.

### 9.7 KV verbs are not a durable multi-stage transaction

The mock proves local persist/restore/fork/discard behavior. The served adapter
explicitly rejects cache operations through its HTTP surface. The wire has no
cache manifest, operation ID, generation/model compatibility, checksum, quota,
TTL, or multi-stage prepare/commit/rollback protocol.

An inference request after Fork must explicitly select the returned sequence_id;
it must not rely on reusing an arbitrary route. Multi-stage restore is available
only after every stage passes the manifest compatibility barrier. Partial restore
must remain hidden and be rolled back or marked recoverable.

### 9.8 Monitoring must become typed and correlated

The current status is an opaque string. It does not show origin/channel,
request phase, hop ID, peer queue state, outbox/event backlog, delivery
acknowledgement, stale-event rejection, KV residency, applied generation
options, MTP acceptance, or device activity.

The required typed status fields are:

    snapshot_seq, generated_at
    agent_id, origin/return channel counts
    lane depth/capacity and peer queue depth
    node phase, capacity, in_flight, batch_width
    request_id, stream_id, sequence_id, stage_id, hop_id
    queue state, enqueue/start/last-progress/deadline timestamps
    resident/durable KV state
    event loss, stale/duplicate counts, delivery state
    adapter capability and applied-option state

## 10. Final protocol acceptance gate

P4 must not be called a complete general inference protocol until:

1. An ingress agent outside the pipeline chain receives every streamed Token
   and Done, including direct-reply failure and fallback.
2. Multiple OUTER clients use the same ingress agent without route or channel
   misdelivery.
3. Reconnect behavior is explicitly buffer, rebind, or cancel and is tested.
4. Streaming continuation receives all ordered events and rejects duplicates,
   gaps, late terminal events, and post-terminal tokens.
5. Hop events carry correlation and stale/duplicate events cannot consume a
   newer hop.
6. Sustained arrivals show bounded RAM, bounded spill if enabled, explicit
   refusal, and FIFO order.
7. A real staged adapter demonstrates stage overlap and feed-at-capacity
   evidence; mock overlap alone is insufficient.
8. OUTER-to-adapter opaque generation content survives every hop without
   reinterpretation, truncation, or silent discard.
9. Adapter rejection of an opaque generation request is returned with its
   correlation identity; P4 does not manufacture backend capability semantics.
10. Multi-stage KV restore uses a compatibility manifest and an all-stage commit
    barrier.

Until these gates pass, the accurate description remains: P4 is a framed,
route-hashed CPS layer with mock-proven pipeline scheduling and partial
backend-option/KV transport, not a complete durable multi-OUTER generation
protocol.

## 11. Discovery is required before distributed loading

The current `Inspect` and `Load` messages do not provide a complete planning
loop. `Inspect` returns a machine snapshot containing OS, architecture, CPU
cores, and registered adapter names. It does not inspect a model. `Load`
receives an already-composed opaque plan, artifact name, and concurrency
ceiling. Therefore P4 currently assumes that OUTER already knows the model
architecture and has already decided the placement.

That assumption is invalid when OUTER has no model or backend knowledge. A
distributed load needs a discovery phase before any node receives `Load`:

    OUTER
      -> agent: discover model artifact and local capability
      <- agent: model profile + artifact identity + hardware/adapter facts
      -> OUTER planner: compose a global placement plan
      -> each agent/node: opaque Load(plan, artifact, ceiling)

The discovery phase is now present as a transport contract:
`ToAgent::InspectModel { artifact, adapter }` and `Reply::Model { artifact,
adapter, profile }` carry an opaque profile. The mock adapter implements this
hook for protocol tests, but a concrete GGUF parser and capability snapshot
binding are still required. Discovery is also not a request for the agent to
choose the global placement. The responsibilities are deliberately separated:

- The agent resolves an allowed model reference and reads the GGUF headers and
  tensor indexes available on that host.
- The agent reports local facts, current availability, and adapter limits.
- OUTER owns policy, compares all agents, chooses layer/stage placement, and
  serializes the resulting plan for the selected adapter.
- P4 transports the discovery result and the final opaque plan; it must not
  interpret llama.cpp switches or generation options.

The minimum model descriptor must include a stable model fingerprint and shard
set identity, file sizes and completeness, architecture, executable layer
count, embedding and attention dimensions, KV-head layout, context limit,
quantization/tensor inventory, per-layer weight bytes, expert count and
per-layer expert bytes when applicable, boundary tensors, and the cache layout
needed by the selected runtime. The existing llama domain code already derives
these facts from GGUF metadata and tensor indexes; the P4 service does not
currently expose them.

The minimum local capability descriptor must include agent identity and
snapshot time, model availability, device identity and backend, total and
currently available VRAM/RAM, adapter/runtime/ABI identity, supported
distribution modes, maximum stage or batch capacity, load concurrency limits,
and any already-resident artifact generation. A capacity value without a
timestamp and model fingerprint is not safe for planning.

Discovery responses need `request_id`, `model_fingerprint`,
`capability_snapshot_id`, `generated_at`, and an expiry or invalidation rule.
They also need explicit partial/error results for missing shards, unreadable
GGUF files, unsupported architectures, insufficient memory, and an adapter
that cannot realize the requested distribution mode. Raw absolute paths should
not cross the boundary; an artifact reference and fingerprint should identify
the files while the agent resolves the permitted local path.

This also separates two cases that are currently conflated. An adapter that
supports only internal loading can return a valid model profile and report
that staged placement is unsupported; OUTER may then choose one internal node
or reject the request. A staged adapter must additionally report the stage
constraints and memory overhead needed to turn the per-layer byte profile into
an executable plan. GGUF metadata alone cannot prove that a backend can load a
stage.

The protocol consequently needs distinct discovery operations, even if their
payloads remain strings for transport compatibility:

    InspectAgent      -> typed machine, device, adapter, and resource facts
    InspectModel      -> typed GGUF model profile and artifact manifest
    Plan/Load         -> OUTER-produced opaque adapter plan

The acceptance gate must therefore include: the same `model_fingerprint` is
observed across all selected agents; incomplete or divergent shard sets are
rejected; the plan records which capability snapshot it used; and a load is
refused when the snapshot has expired or the adapter cannot realize the
selected distribution mode. Until a concrete adapter returns the required
GGUF profile and capability snapshot is bound to the resulting plan, the
existing `InspectModel` transport plus opaque `Load` pair is still insufficient
to claim that an unknown model can be safely or optimally distributed.
