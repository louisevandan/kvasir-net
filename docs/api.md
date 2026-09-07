# The wire

> 문서 지위 (2026-09-06): **경로별 참고·재감사 필요**. 기존 Chain/Hop 설명과 당시 결정을 포함한다. event 경로의 현재 보장은 코드 및 새 검증 규약으로 확인한다.
> 현재 목표·상태·순서는 [실행 로드맵](distributed-batching-roadmap.md), 문서 권위와 읽기 경로는 [문서 안내도](document-map.md)를 따른다.

P4B1 v6. Two things travel: an envelope every hop reads, and a body only its
destination does.

That split is the design. A relay forwards by copying bytes it never decoded,
so forwarding costs the same whatever the message is and a new kind of message
cannot make a relay heavier.

## Frame

```
 0      4   5      8         12        16
 +------+---+------+---------+---------+----------------+---------------+
 | P4B1 | 6 | zero | env_len | body_len|   envelope     |     body      |
 +------+---+------+---------+---------+----------------+---------------+
```

Sixteen-byte header, little-endian lengths. A reader takes the header, learns
the total length, and knows it has a whole frame without decoding any of it.

Envelope at most 256 KiB, body at most 1 MiB. A version that is not 6 is
refused rather than read — a stale binary on a benchmark host must fail loudly
instead of misreading a v6 payload.

## Envelope

| Field | Meaning |
| --- | --- |
| `target` | `tcp://host:port`. Where this goes. |
| `recipient` | `Agent` or `Node(id)`. Who consumes it once the address matched. |
| `lane` | `Control`, `Prefill`, `Decode`, `Response`. Which queue it waits in. |
| `route` | Transport correlation for one exchange. |
| `deadline_unix_ms` | Absolute; zero disables. Checked at hop boundaries. |
| `reply_to` | Where an answer goes. Absent when nobody is listening. |
| `chain` | The ordered nodes an inference travels. Absent on control messages. |

**The address is the identity.** There is no agent id to resolve, which is what
lets a relay decide without consulting anything — the decision that has to stay
stateless for relaying to be free.

**The lane is carried, not derived.** A receiver places a frame without opening
it, which is what lets a socket reader do nothing but enqueue.

## Chain

A chain is an ordered list of links and a position. Each link names one
executable target completely:

```
(address, node, binding, generation)
```

All four are needed. Without the generation a request can reach a node that has
since been rebound and execute against the wrong materialisation.

The **whole** chain travels, not the remainder. That costs frame size and buys
two things: a node can say where in the order it sat, and a failed request can
be retried without reconstructing what it was meant to visit.

A chain of one is valid and is how a backend that spreads a model internally
participates — vLLM and SGLang expose one entry point, so a chain over one is
one link.

Advancing stops at the end rather than wrapping. A decode lap restarts
explicitly, so finishing a pass and continuing generation stay distinguishable
events.

## Two decisions

Every frame that reaches an agent is decided twice, and never more:

```
is the target mine?
  no  → forward it whole; a peer and OUTER are the same case
  yes → agent or node?
          agent → node create and delete, inference intake, hardware
          node  → model load and unload, prefill and decode
```

The second decision has exactly two answers because an agent owns exactly one
kind of internal entity. A third would mean something acquired state it should
not have.

## Bodies

Opaque here. What a body means belongs to whoever sends and receives it; the
core reads a body only through the `Payload` seam a deployment supplies. The
standard vocabulary below lives in [`layers/service`](../layers/service) and
can be replaced wholesale without the core noticing.

Each message is a tag byte and length-prefixed fields. Tags are explicit
constants rather than declaration order, so reordering a variant cannot
silently change what a peer reads.

### To an agent

| Tag | Message | Meaning |
| ---: | --- | --- |
| 1 | `CreateNode { node, adapter }` | Creates the id. Nothing is materialised until a load arrives. A name whose adapter this build does not carry is refused rather than falling back. |
| 2 | `DeleteNode { node }` | Removes it. Answers `Released`, or `Failed` if there was no such node. |
| 3 | `Inspect` | Facts about the machine, for whoever is composing placements. Does not change while the process runs. |
| 4 | `Cancel { route }` | Stops one request. Work already inside a backend runs to its hop boundary — there is no way to interrupt a hop — so this means the next one never starts. Distinguishes having stopped something from there having been nothing to stop. |
| 5 | `Status` | What the agent is doing *now*: lanes, traffic, peers, replies outstanding, and every node with the routes it is holding. |

### To a node

| Tag | Message | Meaning |
| ---: | --- | --- |
| 16 | `Load { plan, artifact, ceiling }` | Materialise this node's share. `plan` is opaque above the adapter; `ceiling` is the concurrency the deployment admits, declared and never derived. |
| 17 | `Unload` | Release it. |
| 18 | `Execute { prompt, max_tokens, options }` | One sequence's work. Never a window — batching is the node's decision. |
| 19 | `Persist { sequence }` | Write that request's cached state somewhere durable and free the memory. One verb, because persisting without freeing saves nothing and freeing without persisting is what already happens when a request ends. |
| 20 | `Restore { sequence }` | Bring it back under the same id. |
| 21 | `Fork { sequence, into }` | Copy it under a new id, leaving the original. The branch case; a copy rather than an alias, because two continuations sharing state would each corrupt the other. |
| 22 | `Discard { sequence }` | Delete the durable copy. State nothing ever deletes is a disk filling up on a schedule nobody set. |

Load and the four cache verbs are *lifecycle*: one instruction about one thing,
run alone rather than batched into a window. That a load is about a deployment
and a cache verb about a sequence makes no difference to the node, which cares
only that they do not batch. A cache verb does not touch the binding —
persisting a conversation says nothing about which model is loaded.

Each stage of a chain holds its own shard of a sequence's state, so persisting
a distributed conversation is one instruction per stage, exactly as a load is.

### Replies

| Tag | Reply | Meaning |
| ---: | --- | --- |
| 32 | `Accepted { detail }` | The instruction was taken. |
| 33 | `Progress { stage, percent }` | A load moved. Reported per stage: a model spread over layer ranges finishes when its slowest piece does, and one total hides which piece that was. |
| 34 | `Bound { generation }` | Executable, and which materialisation. The generation is the one identifier an adapter issues rather than receives. |
| 35 | `Released` | Unloaded, or a node deleted. |
| 36 | `Token { index, text }` | One token, and where in the stream it belongs. The backend counts the position — a request does not carry its progress back down. |
| 37 | `Done { reason, generated }` | Terminal for a route, with the backend's own reason. |
| 38 | `Failed { detail }` | Terminal, with why. |
| 39 | `Machine { snapshot }` | Answer to `Inspect`. |
| 40 | `Status { snapshot }` | Answer to `Status`. One line of lanes and traffic, then a line per node: `node=<id> depth=<queued> running=<in the adapter> routes=[…]`. `running` is a count rather than a flag, because the ceiling bounds how many go over at once and "something is running" is equally true at one and at a hundred. |
| 41 | `Cached { sequence, bytes, detail }` | A cache verb finished. `sequence` is the id the state now lives under — the new one after a fork — and `bytes` is what the durable copy occupies, which only the backend knows. |

## The transaction a load is

A distributed load is a transaction across machines that no single machine sees
the whole of, and nothing coordinates it: there is nowhere to put a coordinator
that would not become a controller. So each stage enforces its own half.

A node serves only the generation it bound. A node whose load *failed* serves
nothing until one succeeds. A node never loaded serves anything, because a
backend that needs no load is legitimate.

The case that matters: three machines loaded, one failed. Without this the
chain runs and answers from two thirds of a model — which looks like a working
deployment and is the worst outcome available. With it, the failed stage
refuses and the caller is told.
